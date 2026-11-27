//! Random edits of the sample diagrams: drawing never panics (the app is
//! built with `panic = "abort"`), and what draws is a well-formed SVG.
//! More rounds: `FDL_MERMAID_FUZZ=200000 cargo test --release -p fdl_mermaid --test fuzz`.

use fdl_mermaid::{Approx, Style, render};

const SAMPLES: [&str; 10] = [
    include_str!("samples/f01-basic.mmd"), include_str!("samples/f02-subgraph.mmd"), include_str!("samples/f03-lr-shapes.mmd"), include_str!("samples/f04-zh.mmd"),
    include_str!("samples/f05-cycle.mmd"), include_str!("samples/s01-basic.mmd"), include_str!("samples/s02-full.mmd"), include_str!("samples/s03-create.mmd"),
    include_str!("samples/st01-regions.mmd"), include_str!("samples/st02-full.mmd"),
];

/// Pieces of syntax spliced in at random.
const BITS: [&str; 56] = [
    "-->", "---", "-.->", "==>", "~~~", "--o", "--x", "<-->", "|t|", "-- t -->", "[", "]", "(", ")", "{", "}", "((", "))", "[[", "]]", "[(", ">", "\"", "`", ":::c",
    "\nsubgraph s\n", "\nend\n", "\ndirection LR\n", "&", ";", "\n", " ", "@{ shape: diamond }", "e1@", "\nclassDef c fill:#f00\n", "\nclass A c\n", "\nstyle A fill:#0f0\n", "\nlinkStyle 0 stroke:red\n",
    "->>", "-->>+", "-)", "-x", ": msg", "\nloop x\n", "\nalt y\n", "\nelse\n", "\nNote over A,B: n\n", "\nactivate A\n",
    "[*]", "\nstate S {\n", "\n--\n", "<<choice>>", "<<fork>>", "\nnote right of A\n", "\nend note\n", "\nstate \"d\" as S\n",
];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn below(&mut self, n: usize) -> usize { (self.next() % n.max(1) as u64) as usize }
}

fn mutate(src: &str, r: &mut Rng) -> String {
    let mut s: Vec<char> = src.chars().collect();
    for _ in 0..1 + r.below(6) {
        let at = r.below(s.len() + 1);
        match r.below(4) {
            0 if !s.is_empty() => { let end = (at + 1 + r.below(8)).min(s.len()); if at < end { s.drain(at..end); } }
            1 => { let b = BITS[r.below(BITS.len())]; for (k, c) in b.chars().enumerate() { s.insert(at.min(s.len()) + k, c); } }
            2 if s.len() > 2 => { let a = r.below(s.len()); let b = r.below(s.len()); s.swap(a, b); }
            _ => { let c = ['A', 'B', '中', '#', ';', '-', '>', '|', '\\', '%', '}'][r.below(11)]; s.insert(at.min(s.len()), c); }
        }
    }
    s.into_iter().collect()
}

#[test]
fn random_edits_never_panic() {
    let rounds: usize = std::env::var("FDL_MERMAID_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let style = Style::default();
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let mut drawn = 0;
    for i in 0..rounds {
        let base = SAMPLES[i % SAMPLES.len()];
        let src = if i % 3 == 0 { mutate(&mutate(base, &mut r), &mut r) } else { mutate(base, &mut r) };
        let out = std::panic::catch_unwind(|| render(&src, &style, &Approx));
        let Ok(out) = out else { panic!("panicked on:\n{src}") };
        if let Ok(svg) = out {
            drawn += 1;
            assert!(svg.width.is_finite() && svg.height.is_finite() && svg.width >= 0. && svg.height >= 0., "{src}");
            assert!(svg.svg.starts_with("<svg") && svg.svg.trim_end().ends_with("</svg>"), "{src}");
            assert!(!svg.svg.contains("NaN") && !svg.svg.contains("inf"), "{src}");
        }
    }
    eprintln!("{drawn}/{rounds} edits drew");
    assert!(drawn > rounds / 20, "most edits should still draw: {drawn}/{rounds}");
}

#[test]
fn every_sample_draws() {
    for s in SAMPLES { render(s, &Style::default(), &Approx).unwrap_or_else(|e| panic!("{e}\n{s}")); }
}

#[test]
fn big_graphs_stay_quick() {
    // 300 nodes in a tangle: layout stays well under a second
    let mut src = String::from("flowchart TD\n");
    let mut r = Rng(7);
    for i in 0..300 { src.push_str(&format!("n{} --> n{}\n", r.below(i + 1), i + 1)); }
    for _ in 0..60 { src.push_str(&format!("n{} --> n{}\n", r.below(300), r.below(300))); }
    let t = std::time::Instant::now();
    render(&src, &Style::default(), &Approx).unwrap();
    assert!(t.elapsed().as_secs_f32() < if cfg!(debug_assertions) { 20. } else { 2. }, "{:?}", t.elapsed());
}
