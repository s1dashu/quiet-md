//! Pie charts (`pie`): slices clockwise from twelve o'clock in the order
//! written, each with its share inside, a legend beside (with the values
//! under `showData`), the title on top.

use crate::svg::{Anchor, Doc, Paint, n};
use crate::text::{self, Block};
use crate::{Error, Line, Measure, Style, Svg, err};

#[derive(Clone, Debug, PartialEq)]
pub struct Pie {
    pub title: Option<String>,
    pub show_data: bool,
    pub slices: Vec<(String, f64)>,
}

pub fn parse(lines: &[Line]) -> Result<Pie, Error> {
    let Some(first) = lines.first() else { return err(0, "The diagram is empty") };
    let mut pie = Pie { title: None, show_data: false, slices: Vec::new() };
    // `pie showData title …` on the first line
    let mut head = first.text["pie".len().min(first.text.len())..].trim();
    if let Some(r) = head.strip_prefix("showData") { pie.show_data = true; head = r.trim(); }
    if let Some(r) = head.strip_prefix("title") { pie.title = Some(text::label(r)).filter(|t| !t.is_empty()); } else if !head.is_empty() { return err(first.no, format!("“{head}” isn't understood after “pie”")) }
    let mut acc_block = false;
    for l in &lines[1..] {
        let t = l.text;
        if acc_block { if t.ends_with('}') { acc_block = false; } continue }
        if t.starts_with("accTitle") || t.starts_with("accDescr") { acc_block = t.starts_with("accDescr") && t.contains('{') && !t.ends_with('}'); continue }
        if t == "showData" { pie.show_data = true; continue }
        if let Some(r) = t.strip_prefix("title") && (r.is_empty() || r.starts_with(char::is_whitespace)) { pie.title = Some(text::label(r)).filter(|t| !t.is_empty()); continue }
        let Some(q) = t.strip_prefix('"').or_else(|| t.strip_prefix('\'')) else { return err(l.no, "Write a slice as “\"Label\" : value”") };
        let quote = if t.starts_with('"') { '"' } else { '\'' };
        let Some(end) = q.find(quote) else { return err(l.no, "A slice's label is missing its closing quote") };
        let label = text::label(&q[..end]);
        let Some(v) = q[end + 1..].trim().strip_prefix(':') else { return err(l.no, format!("“{label}” needs a value: “\"{label}\" : 10”")) };
        let v = v.trim().trim_end_matches(';').trim();
        let Ok(x) = v.parse::<f64>() else { return err(l.no, format!("“{v}” isn't a number")) };
        if !x.is_finite() || x < 0. { return err(l.no, format!("A slice's value can't be negative: “{label}” is {v}")) }
        pie.slices.push((label, x));
    }
    if pie.slices.is_empty() { return err(0, "A pie needs at least one slice") }
    Ok(pie)
}

/// A number as Mermaid shows it: up to two decimals, no trailing zeros.
fn num(x: f64) -> String {
    let s = format!("{:.2}", x);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn render(lines: &[Line], title: Option<String>, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let pie = parse(lines)?;
    let fs = style.font_size;
    let col = &style.colors;
    let total: f64 = pie.slices.iter().map(|s| s.1).sum();
    if total <= 0. { return err(0, "A pie's values add up to nothing") }
    let series = |i: usize| -> &str { if col.series.is_empty() { &col.node_fill } else { &col.series[i % col.series.len()] } };

    let r = 130.;
    let margin = 12.;
    let title = pie.title.clone().or(title);
    let title_block = title.as_deref().map(|t| Block::new(t, 0., fs * 1.3, true, m)).filter(|b| !b.is_empty());
    let top = margin + title_block.as_ref().map_or(0., |b| b.h + 12.);
    let legend: Vec<Block> = pie.slices.iter().map(|(l, v)| {
        let t = if pie.show_data { format!("{l} [{}]", num(*v)) } else { l.clone() };
        Block::new(&t, 260., fs, false, m)
    }).collect();
    let sw = 14.;
    let row = |b: &Block| b.h.max(sw) + 8.;
    let legend_h: f32 = legend.iter().map(row).sum::<f32>() - 8.;
    let legend_w = legend.iter().map(|b| b.w).fold(0., f32::max) + sw + 8.;
    let (cx, cy) = (margin + r, top + r.max(legend_h / 2.));
    let lx = cx + r + 28.;
    let width = (lx + legend_w + margin).max(title_block.as_ref().map_or(0., |b| b.w + 2. * margin));
    let height = top + (2. * r).max(legend_h) + margin;
    let mut doc = Doc::new(&style.font);
    if let Some(b) = &title_block { doc.text(b, width / 2., margin + b.h / 2., Anchor::Middle, &col.text); }

    // slices from twelve o'clock, clockwise
    let edge = Paint::new("none", &col.background, 2.);
    let shown: Vec<(usize, f64)> = pie.slices.iter().enumerate().filter(|(_, s)| s.1 > 0.).map(|(i, s)| (i, s.1)).collect();
    let at = |a: f64| ((cx as f64 + r as f64 * a.sin()) as f32, (cy as f64 - r as f64 * a.cos()) as f32);
    let mut a0 = 0f64;
    let mut labels = Vec::new();
    for &(i, v) in &shown {
        let share = v / total;
        let a1 = a0 + share * std::f64::consts::TAU;
        let fill = Paint::new(series(i), "none", 0.);
        if shown.len() == 1 { doc.circle(cx, cy, r, &fill); } else {
            let (p0, p1) = (at(a0), at(a1));
            let large = if a1 - a0 > std::f64::consts::PI { 1 } else { 0 };
            doc.path(&format!("M{},{} L{},{} A{},{} 0 {large} 1 {},{} Z", n(cx), n(cy), n(p0.0), n(p0.1), n(r), n(r), n(p1.0), n(p1.1)), &fill);
        }
        let mid = (a0 + a1) / 2.;
        let (lx, ly) = ((cx as f64 + 0.7 * r as f64 * mid.sin()) as f32, (cy as f64 - 0.7 * r as f64 * mid.cos()) as f32);
        // too thin a slice for its share to fit: no label
        if share >= 0.03 { labels.push((Block::new(&format!("{}%", num((share * 100.).round())), 0., fs * 0.92, false, m), lx, ly)); }
        a0 = a1;
    }
    if shown.len() > 1 {
        let mut a = 0f64;
        for &(_, v) in &shown { doc.line((cx, cy), at(a), &edge); a += v / total * std::f64::consts::TAU; }
    }
    doc.circle(cx, cy, r, &Paint::new("none", &col.line, 1.));
    for (b, x, y) in &labels { doc.text(b, *x, *y, Anchor::Middle, &col.text); }

    // the legend, centred on the pie
    let mut y = cy - legend_h / 2.;
    for (i, b) in legend.iter().enumerate() {
        let h = b.h.max(sw);
        doc.rect(lx, y + (h - sw) / 2., sw, sw, 2., &Paint::new(series(i), "none", 0.));
        doc.text(b, lx + sw + 8., y + h / 2., Anchor::Start, &col.text);
        y += row(b);
    }
    Ok(Svg { svg: doc.finish(width, height, ""), width, height })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> Result<Pie, Error> { let (l, _) = crate::lines(src); parse(&l) }

    #[test]
    fn slices_title_and_data() {
        let x = p("pie showData title Key elements\n  \"Calcium\" : 42.96\n  \"Potassium\" : 50.05\n  'Iron' : 5").unwrap();
        assert_eq!(x.title.as_deref(), Some("Key elements"));
        assert!(x.show_data);
        assert_eq!(x.slices, [("Calcium".into(), 42.96), ("Potassium".into(), 50.05), ("Iron".into(), 5.)]);
        let y = p("pie\ntitle Pets\naccTitle: a\n\"Dogs\" : 386\n\"Cats\": 85.").unwrap();
        assert_eq!(y.title.as_deref(), Some("Pets"));
        assert_eq!(y.slices.len(), 2);
        assert_eq!(num(85.), "85");
        assert_eq!(num(42.956), "42.96");
    }

    #[test]
    fn mistakes_say_where() {
        assert_eq!(p("pie\n\"A\" : -1").unwrap_err().line, 2);
        assert_eq!(p("pie\n\"A\" 3").unwrap_err().line, 2);
        assert_eq!(p("pie\nA : 3").unwrap_err().line, 2);
        assert_eq!(p("pie\n\"A : 3").unwrap_err().line, 2);
        assert_eq!(p("pie\n\"A\" : x").unwrap_err().line, 2);
        assert_eq!(p("pie title T").unwrap_err().line, 0);
        assert!(crate::render("pie\n\"A\" : 0", &Style::default(), &crate::Approx).is_err());
        assert!(crate::render("pie\n\"A\" : 0\n\"B\" : 2", &Style::default(), &crate::Approx).is_ok());
    }
}
