//! 匿名使用统计 for the app: per UTC day, how often Quiet.md was opened (launched
//! or brought to the front), under a random id for this computer, for the
//! stats.quiet.md dashboard (sent to telemetry.quiet.md/v1/app). Only counts:
//! nothing about the library, its documents, file names or the user.
//!
//! Kept in `<data dir>/quiet-md/app-usage.json`; finished days go out with
//! `curl` (as the update check fetches) soon after launch and every few hours,
//! and are forgotten once accepted. Off with 设置 › 通用 › 匿名使用统计 (untouched,
//! it follows 自动检查更新), `DO_NOT_TRACK` or `QUIET_NO_TELEMETRY`; turning it
//! off forgets the id and the days. Scripted, test and development runs never count.
//! The `quiet` command line tool counts its own runs the same way (crates/cli/src/usage.rs).

use gpui::App;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

const URL: &str = "https://telemetry.quiet.md/v1/app";
/// Days kept waiting to be sent (a computer offline for longer loses the oldest).
const KEEP: usize = 31;

fn file() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("QUIET_APP_TELEMETRY_FILE") { return Some(PathBuf::from(p)); }
    Some(dirs::data_local_dir()?.join("quiet-md").join("app-usage.json"))
}

fn url() -> String { std::env::var("QUIET_TELEMETRY_URL").map(|u| u.replace("/v1/cli", "/v1/app")).unwrap_or_else(|_| URL.to_string()) }

/// Whether this run may count: the switch on, and a real copy in real use.
pub(crate) fn on() -> bool {
    let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"));
    if set("DO_NOT_TRACK") || set("QUIET_NO_TELEMETRY") || set("CI") { return false }
    // Scripted runs and test libraries (harness, self-test, screenshots), development
    // and preview builds; a test that names its own file (QUIET_APP_TELEMETRY_FILE) counts.
    if std::env::var_os("QUIET_APP_TELEMETRY_FILE").is_none() {
        if std::env::var_os("FDL_SCRIPT").is_some() || std::env::var_os("QUIET_MD_HOME").is_some() || cfg!(debug_assertions) || cfg!(feature = "devtools") { return false }
        if std::env::current_exe().is_ok_and(|e| e.to_string_lossy().contains("dist-preview")) { return false }
    }
    let p = crate::settings::prefs();
    p.usage_stats.unwrap_or(p.auto_update)
}

fn read() -> Value {
    file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn write(v: &Value) {
    let Some(path) = file() else { return };
    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, serde_json::to_vec_pretty(v).unwrap_or_default()).is_ok() { let _ = std::fs::rename(&tmp, &path); }
}

/// `YYYY-MM-DD` in UTC.
fn today() -> String {
    let secs = fdl_core::time::now();
    let z = (secs / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

/// When an open was last counted: coming back within half an hour is the same use.
static LAST: std::sync::Mutex<Option<web_time::Instant>> = std::sync::Mutex::new(None);

/// Quiet.md was launched or came to the front: one more open today, unless
/// it was counted less than half an hour ago.
pub fn opened() {
    if !on() { return }
    {
        let mut last = LAST.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(1800)) { return }
        *last = Some(web_time::Instant::now());
    }
    let mut v = read();
    let o = v.as_object_mut().unwrap();
    if !o.get("id").is_some_and(Value::is_string) { o.insert("id".into(), json!(uuid::Uuid::new_v4().to_string())); }
    let days = o.entry("days").or_insert_with(|| json!({})).as_object_mut().unwrap();
    let d = days.entry(today()).or_insert_with(|| json!({ "opens": 0 }));
    d["opens"] = json!(d["opens"].as_u64().unwrap_or(0) + 1);
    let mut keys: Vec<String> = days.keys().cloned().collect();
    keys.sort();
    for k in keys.iter().take(keys.len().saturating_sub(KEEP)) { days.remove(k); }
    write(&v);
}

/// The switch turned off: the id and the waiting days go.
pub fn forget() {
    if let Some(f) = file() { let _ = std::fs::remove_file(f); }
}

/// Post the finished days; forget the ones accepted. Runs off the main thread.
fn send() {
    if !on() { return }
    let v = read();
    let (Some(id), Some(days)) = (v.get("id").and_then(Value::as_str), v.get("days").and_then(Value::as_object)) else { return };
    let today = today();
    let done: Vec<Value> = days.iter().filter(|(k, _)| k.as_str() < today.as_str()).map(|(k, d)| json!({ "day": k, "opens": d["opens"] })).collect();
    if done.is_empty() { return }
    let body = json!({ "v": 1, "id": id, "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "days": done });
    let testing = std::env::var_os("QUIET_TELEMETRY_URL").is_some();
    let mut c = crate::update::curl_command(testing);
    c.args(["--max-time", "20", "-X", "POST", "-H", "content-type: application/json", "--data-binary", "@-", &url()])
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let Ok(mut child) = c.spawn() else { return };
    if let Some(mut stdin) = child.stdin.take() { use std::io::Write; let _ = stdin.write_all(body.to_string().as_bytes()); }
    if !child.wait().is_ok_and(|s| s.success()) { return }
    // Accepted: drop what was sent (a day counted meanwhile stays).
    let mut now = read();
    if let Some(days) = now.get_mut("days").and_then(Value::as_object_mut) { days.retain(|k, _| k.as_str() >= today.as_str()); }
    write(&now);
}

/// Counts this launch; sends finished days a minute after launch, then every six hours.
pub fn init(cx: &mut App) {
    opened();
    cx.spawn(async |cx| {
        cx.background_executor().timer(Duration::from_secs(60)).await;
        loop {
            let _ = cx.background_executor().spawn(async { send() }).await;
            cx.background_executor().timer(Duration::from_secs(6 * 3600)).await;
        }
    }).detach();
}
