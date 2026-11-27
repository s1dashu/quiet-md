//! Time the core steps of 添加文件夹 on a big folder (docs/PERFORMANCE.md, "Adding a code folder"):
//!   TMPDIR=<dir's parent> cargo run --release -p fdl_core --example add_folder_bench -- <dir> [rounds]
//! The folder must be under `$TMPDIR`, so what this computer keeps about it goes there too.
use std::time::Instant;
use fdl_core::store::Store;

fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap_or(0.) / 1024.
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("dir"));
    let rounds: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    println!("rss at start {:.0} MB", rss_mb());
    let t = Instant::now();
    let shape = Store::shape_at(&dir);
    println!("shape_at (layout::scan)        {:>8.0} ms  hash {shape:x}", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let mut store = Store::open_top_folder_as(&dir, "add-folder-bench").expect("open");
    println!("open_top_folder_as              {:>8.0} ms  rss {:.0} MB", t.elapsed().as_secs_f64() * 1e3, rss_mb());
    for r in 0..rounds {
        let t = Instant::now();
        let entries = store.list();
        let docs = entries.iter().filter(|e| e.meta.folder.is_none()).count();
        println!("list() round {r}                  {:>8.0} ms  {} entries ({docs} docs, {} folders)  rss {:.0} MB", t.elapsed().as_secs_f64() * 1e3, entries.len(), entries.len() - docs, rss_mb());
    }
    let t = Instant::now();
    let shape = Store::shape_at(&dir);
    println!("shape_at again (2 s poll)       {:>8.0} ms  hash {shape:x}", t.elapsed().as_secs_f64() * 1e3);
}
