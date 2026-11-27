//! One file fetched with the system's NSURLSession, where there is no curl to
//! run (iOS: an app cannot start other programs). What `update::curl_command`
//! asks of curl holds here too: HTTPS only (`testing` also allows http, as far
//! as App Transport Security lets it), 15 s to connect, 30 min in all, an
//! HTTP error status fails; the system's proxy settings apply by themselves.

use objc2_foundation::{NSError, NSHTTPURLResponse, NSString, NSURL, NSURLResponse, NSURLSession, NSURLSessionConfiguration};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// Fetches `url` into `to` (replaced), calling `received` with the bytes so far
/// every 250 ms while it arrives. Blocks: not for the main thread.
pub fn download(url: &str, to: &Path, testing: bool, received: impl Fn(u64)) -> Result<(), String> {
    if !(url.starts_with("https://") || testing && url.starts_with("http://")) { return Err(format!("{url}: not https")); }
    objc2::rc::autoreleasepool(|_| {
        let ns_url = NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(|| format!("{url}: not a URL"))?;
        let config = NSURLSessionConfiguration::defaultSessionConfiguration();
        config.setTimeoutIntervalForRequest(15.);
        config.setTimeoutIntervalForResource(1800.);
        let session = NSURLSession::sessionWithConfiguration(&config);
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        let to = to.to_path_buf();
        // The downloaded file is the system's only until the handler returns: it is moved out there.
        let done = block2::RcBlock::new(move |file: *mut NSURL, resp: *mut NSURLResponse, err: *mut NSError| {
            let (file, resp, err) = unsafe { (file.as_ref(), resp.as_ref(), err.as_ref()) };
            let got = match (err, file) {
                (Some(e), _) => Err(e.localizedDescription().to_string()),
                (None, None) => Err("no file".into()),
                (None, Some(file)) => match resp.and_then(|r| r.downcast_ref::<NSHTTPURLResponse>()).map(|r| r.statusCode()) {
                    Some(s) if !(200..300).contains(&s) => Err(format!("HTTP {s}")),
                    _ => file.path().ok_or_else(|| "no file".to_string()).and_then(|p| {
                        let from = std::path::PathBuf::from(p.to_string());
                        std::fs::rename(&from, &to).or_else(|_| std::fs::copy(&from, &to).map(|_| ())).map_err(|e| e.to_string())
                    }),
                },
            };
            let _ = tx.send(got);
        });
        // SAFETY: the handler only moves a file and sends on a channel, from whichever thread the session picks.
        let task = unsafe { session.downloadTaskWithURL_completionHandler(&ns_url, &done) };
        task.resume();
        session.finishTasksAndInvalidate();
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(got) => return got,
                Err(mpsc::RecvTimeoutError::Timeout) => received(task.countOfBytesReceived().max(0) as u64),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("the download ended without an answer".into()),
            }
        }
    })
}
