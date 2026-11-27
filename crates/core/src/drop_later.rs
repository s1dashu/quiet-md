//! Freeing big structures off the thread that lets go of them. A library's
//! index, metadata and entries are hundreds of thousands of small allocations
//! (15,000 notes: about 20 ms to free); the window replacing them with ones
//! listed in the background must not spend a frame's time freeing the old.

/// Drops `value` on a background thread (in place on the web, which has none).
pub fn drop_later<T: Send + 'static>(value: T) {
    #[cfg(not(target_family = "wasm"))]
    {
        use std::sync::{Mutex, OnceLock, mpsc::Sender};
        type Garbage = Box<dyn Send>;
        static TX: OnceLock<Mutex<Sender<Garbage>>> = OnceLock::new();
        let tx = TX.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Garbage>();
            // One thread for the life of the process, freeing what it is handed in turn.
            let _ = std::thread::Builder::new().name("drop-later".into()).spawn(move || for g in rx { drop(g) });
            Mutex::new(tx)
        });
        // No thread (it could not start): freed here after all.
        if let Err(back) = tx.lock().unwrap().send(Box::new(value)) { drop(back.0); }
    }
    #[cfg(target_family = "wasm")]
    drop(value);
}

#[cfg(test)]
mod tests {
    #[test]
    fn what_is_handed_over_is_freed() {
        let alive = std::sync::Arc::new(());
        let held = alive.clone();
        super::drop_later(vec![held]);
        for _ in 0..200 {
            if std::sync::Arc::strong_count(&alive) == 1 { return }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("not freed");
    }
}
