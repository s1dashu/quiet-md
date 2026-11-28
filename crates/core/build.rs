//! Embed the 开始指南 (shared-mock/library, built by shared-mock/guides/build.py)
//! into `guides` (src/guides.rs): the app and `quiet` both bring them along.

use std::fmt::Write;
use std::path::Path;

fn main() {
    embed("../../shared-mock/library", "guides.rs");
    embed("../../shared-mock/library-zh", "guides_zh.rs");
}

fn embed(source: &str, output: &str) {
    // Read at run time: a compile-time path goes stale when the checkout moves.
    let root = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join(source);
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = vec![];
    collect(&root, &root, &mut files);
    files.sort();
    let mut out = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (rel, abs) in files { writeln!(out, "    ({rel:?}, include_bytes!({abs:?})),").unwrap(); }
    out.push_str("];\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join(output), out).unwrap();
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() { collect(root, &p, out); continue; }
        out.push((p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), p.to_string_lossy().into_owned()));
    }
}
