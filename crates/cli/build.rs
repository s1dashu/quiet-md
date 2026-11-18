//! Embeds built-in cover gallery images (`assets/covers/*.jpg`) for `quiet_cli`.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let covers_dir = Path::new(&manifest).join("../../assets/covers");
    println!("cargo:rerun-if-changed={}", covers_dir.display());
    let mut entries = vec![];
    for e in std::fs::read_dir(&covers_dir).unwrap().flatten() {
        let p = e.path();
        if p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg")) {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let abs = p.canonicalize().unwrap_or(p).to_string_lossy().into_owned();
            entries.push((name, abs));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::from("pub static COVERS: &[(&str, &[u8])] = &[\n");
    for (name, abs) in entries {
        writeln!(out, "    ({name:?}, include_bytes!({abs:?})),").unwrap();
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("covers.rs");
    std::fs::write(dest, out).unwrap();
}
