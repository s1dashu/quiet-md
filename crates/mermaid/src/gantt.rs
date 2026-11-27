//! Gantt charts (`gantt`): tasks in sections on a time axis. Dates are read
//! in the chart's `dateFormat` (dayjs tokens), lengths as durations (`3d`,
//! `20h`, `1w`…) or end dates, starts as dates, `after` other tasks or the
//! previous task's end, ends `until` other tasks; `excludes` (weekends,
//! weekdays by name, dates) push lengths past days off. Bars are coloured
//! by state (done, active, crit), milestones are diamonds and `vert`
//! tasks lines across; the axis ticks pick a step that fits and print it in
//! `axisFormat` (strftime). Times are seconds from 1970 in no time zone.

use crate::svg::{Anchor, Doc, Paint};
use crate::text::{self, Block};
use crate::{Error, Line, Measure, Style, Svg, err};

const DAY: f64 = 86400.;

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The civil date of a day since 1970-01-01.
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// 0 = Sunday.
fn weekday(day: i64) -> i64 { (day + 4).rem_euclid(7) }

const WEEKDAYS: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// `s` read in a dayjs format (`YYYY-MM-DD`, `HH:mm`, `X`…); whole input.
fn parse_date(s: &str, fmt: &str) -> Option<f64> {
    let s = s.trim();
    if fmt == "X" { return s.parse::<f64>().ok().filter(|x| x.is_finite()) }
    if fmt == "x" { return s.parse::<f64>().ok().filter(|x| x.is_finite()).map(|x| x / 1000.) }
    let (mut y, mut mo, mut d, mut h, mut mi, mut se, mut ms) = (1970i64, 1i64, 1i64, 0i64, 0i64, 0i64, 0f64);
    let (f, b) = (fmt.as_bytes(), s.as_bytes());
    let (mut i, mut j) = (0, 0);
    let num = |j: &mut usize, max: usize| -> Option<i64> {
        let st = *j;
        while *j < b.len() && *j - st < max && b[*j].is_ascii_digit() { *j += 1; }
        if *j == st { return None }
        s[st..*j].parse().ok()
    };
    while i < f.len() {
        let rest = &fmt[i..];
        let tok = ["YYYY", "SSS", "YY", "MM", "DD", "HH", "hh", "mm", "ss", "M", "D", "H", "h", "m", "s"].into_iter().find(|t| rest.starts_with(t));
        match tok {
            Some(t) => {
                let v = num(&mut j, match t { "YYYY" => 4, "SSS" => 3, "M" | "D" | "H" | "h" | "m" | "s" => 2, _ => 2 })?;
                match t { "YYYY" => y = v, "YY" => y = 2000 + v, "MM" | "M" => mo = v, "DD" | "D" => d = v, "HH" | "H" | "hh" | "h" => h = v, "mm" | "m" => mi = v, "ss" | "s" => se = v, _ => ms = v as f64 }
                i += t.len();
            }
            None => {
                if b.get(j) != Some(&f[i]) { return None }
                i += 1;
                j += 1;
            }
        }
    }
    if j != b.len() || !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 24 || mi > 59 || se > 60 { return None }
    Some(days_from_civil(y, mo, d) as f64 * DAY + (h * 3600 + mi * 60 + se) as f64 + ms / 1000.)
}

/// A date in the chart's format, else ISO (`2014-01-06`, `2014-01-06T10:00`).
fn date(s: &str, fmt: &str) -> Option<f64> {
    parse_date(s, fmt).or_else(|| parse_date(s, "YYYY-MM-DD")).or_else(|| parse_date(s, "YYYY-MM-DDTHH:mm")).or_else(|| parse_date(s, "YYYY-MM-DD HH:mm"))
}

/// `3d`, `20h`, `1.5w`, `250ms`: seconds.
fn duration(s: &str) -> Option<f64> {
    let s = s.trim();
    let k = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let v: f64 = s[..k].parse().ok()?;
    let unit = match &s[k..] { "ms" => 0.001, "s" => 1., "m" => 60., "h" => 3600., "d" => DAY, "w" => 7. * DAY, "M" => 30. * DAY, "y" => 365. * DAY, _ => return None };
    Some(v * unit)
}

#[derive(Clone, Debug, PartialEq)]
enum Start { At(f64), After(Vec<String>), Prev }
#[derive(Clone, Debug, PartialEq)]
enum End { At(f64), For(f64), Until(Vec<String>) }

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub name: String,
    pub id: Option<String>,
    pub section: usize,
    pub done: bool,
    pub active: bool,
    pub crit: bool,
    pub milestone: bool,
    pub vert: bool,
    pub start: f64,
    pub end: f64,
    line: usize,
    spec: (Start, End),
}

#[derive(Clone, Debug, Default)]
pub struct Gantt {
    pub title: Option<String>,
    pub axis_format: Option<String>,
    pub tick: Option<(f64, &'static str)>,
    pub sections: Vec<String>,
    pub tasks: Vec<Task>,
}

/// Days off: by weekday and by date.
struct Off { weekdays: [bool; 7], days: Vec<i64> }
impl Off {
    fn is(&self, day: i64) -> bool { self.weekdays[weekday(day) as usize] || self.days.contains(&day) }
    fn any(&self) -> bool { self.weekdays.iter().any(|&b| b) || !self.days.is_empty() }
    /// `len` seconds of work from `start`: every day off on the way adds a day.
    fn end(&self, start: f64, len: f64) -> f64 {
        if !self.any() || len <= 0. || self.weekdays.iter().all(|&b| b) { return start + len }
        let mut end = start + len;
        let mut day = (start / DAY).floor() as i64;
        let mut guard = 0;
        while (day as f64) * DAY < end && guard < 20000 {
            if self.is(day) { end += DAY; }
            day += 1;
            guard += 1;
        }
        end
    }
}

pub fn parse(lines: &[Line]) -> Result<Gantt, Error> {
    let mut g = Gantt::default();
    let mut fmt = "YYYY-MM-DD".to_string();
    let mut off = Off { weekdays: [false; 7], days: Vec::new() };
    let mut weekend = [0usize, 6];
    let mut weekends = false;
    let mut excludes: Vec<(String, usize)> = Vec::new();
    let mut inclusive = false;
    let mut acc_block = false;
    for l in &lines[1..] {
        let t = l.text.find("%%").map_or(l.text, |i| l.text[..i].trim());
        if t.is_empty() { continue }
        if acc_block { if t.ends_with('}') { acc_block = false; } continue }
        let word = t.split(|c: char| c.is_whitespace() || c == ':').next().unwrap_or("");
        let rest = t[word.len()..].trim();
        match word {
            "title" => { g.title = Some(text::label(rest)).filter(|x| !x.is_empty()); continue }
            "dateFormat" => { fmt = rest.to_string(); continue }
            "axisFormat" => { g.axis_format = Some(rest.to_string()); continue }
            "tickInterval" => {
                let k = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
                let v: f64 = rest[..k].parse().unwrap_or(1.);
                g.tick = match &rest[k..] { "millisecond" => Some((v * 0.001, "s")), "second" => Some((v, "s")), "minute" => Some((v * 60., "s")), "hour" => Some((v * 3600., "s")), "day" => Some((v * DAY, "s")), "week" => Some((v * 7. * DAY, "s")), "month" => Some((v, "M")), _ => None };
                continue
            }
            "excludes" => { for x in rest.split([',', ' ']).map(str::trim).filter(|x| !x.is_empty()) { excludes.push((x.to_lowercase(), l.no)); } continue }
            "weekend" => { weekend = if rest.eq_ignore_ascii_case("friday") { [5, 6] } else { [0, 6] }; continue }
            "inclusiveEndDates" => { inclusive = true; continue }
            "includes" | "todayMarker" | "weekday" | "displayMode" | "topAxis" | "click" => continue,
            "accTitle" | "accDescr" => { acc_block = word == "accDescr" && rest.starts_with('{') && !rest.ends_with('}'); continue }
            "section" => { g.sections.push(text::label(rest)); continue }
            _ => {}
        }
        let Some((name, meta)) = t.split_once(':') else { return err(l.no, format!("Couldn't read “{t}”: a task is “Name : start, length”")) };
        let mut items: Vec<&str> = meta.split(',').map(str::trim).collect();
        let mut task = Task { name: text::label(name), id: None, section: g.sections.len().saturating_sub(1), done: false, active: false, crit: false, milestone: false, vert: false, start: 0., end: 0., line: l.no, spec: (Start::Prev, End::For(DAY)) };
        while let Some(&tag) = items.first() {
            match tag { "done" => task.done = true, "active" => task.active = true, "crit" => task.crit = true, "milestone" => task.milestone = true, "vert" => task.vert = true, _ => break }
            items.remove(0);
        }
        // (a tag Mermaid doesn't know, as in custom CSS tricks: skipped)
        if items.len() > 3 { items.remove(0); }
        let (id, start, end) = match items.len() {
            0 => return err(l.no, format!("Task “{}” needs a length or an end", task.name)),
            1 => (None, None, items[0]),
            2 => (None, Some(items[0]), items[1]),
            _ => (Some(items[0]), Some(items[1]), items[2]),
        };
        task.id = id.filter(|x| !x.is_empty()).map(String::from);
        let start = match start {
            None => Start::Prev,
            Some(s) if s.starts_with("after ") => Start::After(s[6..].split_whitespace().map(String::from).collect()),
            Some(s) => match date(s, &fmt) { Some(x) => Start::At(x), None => return err(l.no, format!("“{s}” isn't a date in {fmt}")) },
        };
        let end = if let Some(u) = end.strip_prefix("until ") { End::Until(u.split_whitespace().map(String::from).collect()) } else if let Some(d) = duration(end) { End::For(d) } else {
            match date(end, &fmt) { Some(x) => End::At(x + if inclusive { DAY } else { 0. }), None => return err(l.no, format!("“{end}” isn't a length (like 3d) or a date in {fmt}")) }
        };
        task.spec = (start, end);
        g.tasks.push(task);
    }
    for (x, no) in excludes {
        if x == "weekends" { weekends = true; }
        else if let Some(k) = WEEKDAYS.iter().position(|w| *w == x) { off.weekdays[k] = true; }
        else if let Some(d) = date(&x, &fmt) { off.days.push((d / DAY).floor() as i64); }
        else { return err(no, format!("“{x}” isn't a day to exclude (weekends, a weekday or a date)")) }
    }
    if weekends { for k in weekend { off.weekdays[k] = true; } }
    if g.tasks.is_empty() { return err(0, "A Gantt chart needs at least one task") }

    // place the tasks: as many passes as it takes (after / until may point forward)
    let n = g.tasks.len();
    let mut placed = vec![false; n];
    let find = |g: &Gantt, placed: &[bool], id: &str| g.tasks.iter().position(|t| t.id.as_deref() == Some(id)).map(|i| (i, placed[i]));
    for _ in 0..=n {
        let mut progress = false;
        for i in 0..n {
            if placed[i] { continue }
            let start = match &g.tasks[i].spec.0 {
                Start::At(x) => Some(*x),
                Start::Prev => if i == 0 { None } else if placed[i - 1] { Some(g.tasks[i - 1].end) } else { continue },
                Start::After(ids) => {
                    let mut best = f64::MIN;
                    let mut ready = true;
                    for id in ids { match find(&g, &placed, id) { Some((k, true)) => best = best.max(g.tasks[k].end), Some(_) => ready = false, None => return err(g.tasks[i].line, format!("No task has the id “{id}”")) } }
                    if !ready { continue }
                    Some(best)
                }
            };
            let Some(start) = start else { return err(g.tasks[i].line, "The first task needs a start date") };
            let end = match &g.tasks[i].spec.1 {
                End::At(x) => *x,
                End::For(d) => off.end(start, *d),
                End::Until(ids) => {
                    let mut best = f64::MAX;
                    let mut ready = true;
                    for id in ids {
                        match g.tasks.iter().position(|t| t.id.as_deref() == Some(id.as_str())) {
                            // (until needs only the other's start: placed, or a date of its own)
                            Some(k) => if placed[k] { best = best.min(g.tasks[k].start) } else if let Start::At(x) = g.tasks[k].spec.0 { best = best.min(x) } else { ready = false },
                            None => return err(g.tasks[i].line, format!("No task has the id “{id}”")),
                        }
                    }
                    if !ready { continue }
                    best
                }
            };
            g.tasks[i].start = start;
            g.tasks[i].end = end.max(start);
            placed[i] = true;
            progress = true;
        }
        if !progress { break }
    }
    if let Some(i) = placed.iter().position(|p| !p) { return err(g.tasks[i].line, format!("Couldn't place “{}”: its after / until go round in a circle", g.tasks[i].name)) }
    Ok(g)
}

/// `t` printed in a strftime format (the common directives).
fn strftime(t: f64, fmt: &str) -> String {
    let day = (t / DAY).floor() as i64;
    let (y, m, d) = civil(day);
    let secs = (t - day as f64 * DAY).floor() as i64;
    let (hh, mm, ss) = (secs / 3600, secs / 60 % 60, secs % 60);
    let mut out = String::new();
    let mut it = fmt.chars();
    while let Some(c) = it.next() {
        if c != '%' { out.push(c); continue }
        let Some(k) = it.next() else { break };
        let month = MONTHS[(m - 1).clamp(0, 11) as usize];
        let wd = WEEKDAYS[weekday(day) as usize];
        match k {
            'Y' => out.push_str(&y.to_string()),
            'y' => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            'm' => out.push_str(&format!("{m:02}")),
            'd' => out.push_str(&format!("{d:02}")),
            'e' => out.push_str(&d.to_string()),
            'H' => out.push_str(&format!("{hh:02}")),
            'I' => out.push_str(&format!("{:02}", (hh + 11) % 12 + 1)),
            'p' => out.push_str(if hh < 12 { "AM" } else { "PM" }),
            'M' => out.push_str(&format!("{mm:02}")),
            'S' => out.push_str(&format!("{ss:02}")),
            'L' => out.push_str(&format!("{:03}", ((t - t.floor()) * 1000.).round() as i64)),
            'b' => out.push_str(&month[..3]),
            'B' => out.push_str(month),
            'a' => { let mut s = wd[..3].to_string(); s[..1].make_ascii_uppercase(); out.push_str(&s) }
            'A' => { let mut s = wd.to_string(); s[..1].make_ascii_uppercase(); out.push_str(&s) }
            'j' => out.push_str(&format!("{:03}", day - days_from_civil(y, 1, 1) + 1)),
            's' => out.push_str(&format!("{}", t.floor() as i64)),
            'U' | 'W' => out.push_str(&format!("{:02}", (day - days_from_civil(y, 1, 1) + weekday(days_from_civil(y, 1, 1))) / 7)),
            '%' => out.push('%'),
            x => { out.push('%'); out.push(x); }
        }
    }
    out
}

/// Ticks from `t0` to `t1` about `step` apart: seconds (fixed) or months
/// (on the 1st).
fn ticks(t0: f64, t1: f64, step: (f64, &str)) -> Vec<f64> {
    let mut out = Vec::new();
    if step.1 == "M" {
        let k = step.0.max(1.) as i64;
        let (y, m, _) = civil((t0 / DAY).floor() as i64);
        let mut mi = y * 12 + m - 1;
        mi -= mi.rem_euclid(k);
        loop {
            let t = days_from_civil(mi.div_euclid(12), mi.rem_euclid(12) + 1, 1) as f64 * DAY;
            if t > t1 || out.len() > 400 { break }
            if t >= t0 { out.push(t); }
            mi += k;
        }
    } else {
        let s = step.0.max(0.001);
        // days and weeks from midnight (weeks from a Sunday), the rest from round multiples
        let base = if s >= 7. * DAY { let d = (t0 / DAY).floor() as i64; (d - weekday(d)) as f64 * DAY } else { (t0 / s).floor() * s };
        let mut t = base;
        while t <= t1 && out.len() <= 400 { if t >= t0 { out.push(t); } t += s; }
    }
    out
}

pub fn render(lines: &[Line], title: Option<String>, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let g = parse(lines)?;
    let fs = style.font_size;
    let col = &style.colors;
    let small = fs * 0.86;
    let t0 = g.tasks.iter().map(|t| t.start).fold(f64::MAX, f64::min);
    let t1 = g.tasks.iter().map(|t| t.end).fold(f64::MIN, f64::max).max(t0 + 1.);
    let span = t1 - t0;
    // the step: a round one giving at most ~10 ticks
    let steps: [(f64, &str); 22] = [(1., "s"), (5., "s"), (15., "s"), (30., "s"), (60., "s"), (300., "s"), (900., "s"), (1800., "s"), (3600., "s"), (3. * 3600., "s"), (6. * 3600., "s"), (12. * 3600., "s"), (DAY, "s"), (2. * DAY, "s"), (7. * DAY, "s"), (14. * DAY, "s"), (1., "M"), (3., "M"), (6., "M"), (12., "M"), (60., "M"), (120., "M")];
    let len = |s: &(f64, &str)| if s.1 == "M" { s.0 * 30.44 * DAY } else { s.0 };
    let step = g.tick.filter(|s| span / len(s) <= 60.).unwrap_or_else(|| steps.into_iter().find(|s| span / len(s) <= 10.).unwrap_or((1200., "M")));
    let axis_fmt = g.axis_format.clone().unwrap_or_else(|| if span < 2. * DAY { "%H:%M".into() } else { "%Y-%m-%d".into() });

    let margin = 12.;
    let title = g.title.clone().or(title);
    let title_block = title.as_deref().map(|t| Block::new(t, 0., fs * 1.3, true, m)).filter(|b| !b.is_empty());
    let top = margin + title_block.as_ref().map_or(0., |b| b.h + 12.);
    let section_blocks: Vec<Block> = g.sections.iter().map(|s| Block::new(s, 160., small, true, m)).collect();
    // (room for half the first tick's label at least)
    let first_tick = Block::new(&strftime(t0, &axis_fmt), 0., small, false, m);
    let left = margin + section_blocks.iter().map(|b| b.w + 20.).fold(first_tick.w / 2. + 4., f32::max);
    let chart_w = 640f32;
    let x = |t: f64| left + ((t - t0) / span) as f32 * chart_w;
    let (row, bar) = (26f32, 18f32);
    let rows_h = row * g.tasks.len() as f32;
    let axis_h = small * 2.2;
    let width = left + chart_w + margin + 60.;
    let height = top + rows_h + axis_h + margin;
    let mut doc = Doc::new(&style.font);
    doc.paper = col.background.clone();
    if let Some(b) = &title_block { doc.text(b, width / 2., margin + b.h / 2., Anchor::Middle, &col.text); }

    // section bands, alternating, their names at the left
    let mut k = 0;
    while k < g.tasks.len() {
        let s = g.tasks[k].section;
        let from = k;
        while k < g.tasks.len() && g.tasks[k].section == s { k += 1; }
        let (y0, h) = (top + row * from as f32, row * (k - from) as f32);
        if s % 2 == 1 { doc.rect(margin, y0, width - 2. * margin, h, 0., &Paint::new(&col.frame_fill, "none", 0.)); }
        if let Some(b) = section_blocks.get(s).filter(|_| !g.sections.is_empty()) { doc.text(b, margin + 6., y0 + h / 2., Anchor::Start, &col.text); }
    }
    // grid and axis
    let grid = Paint::new("none", &col.cluster_border, 1.);
    let ts = ticks(t0, t1, step);
    let axis_y = top + rows_h;
    doc.line((left, axis_y), (left + chart_w, axis_y), &Paint::new("none", &col.line, 1.));
    let mut last_label = f32::MIN;
    for &t in &ts {
        let gx = x(t);
        doc.line((gx, top), (gx, axis_y + 4.), &grid);
        let b = Block::new(&strftime(t, &axis_fmt), 0., small, false, m);
        // labels too close to the last are left out
        if gx - b.w / 2. > last_label + 6. { doc.text(&b, gx, axis_y + 6. + b.h / 2., Anchor::Middle, &col.text); last_label = gx + b.w / 2.; }
    }
    // tasks
    let series = |k: usize| -> &str { col.series.get(k).map_or(col.node_fill.as_str(), |s| s.as_str()) };
    for (i, t) in g.tasks.iter().enumerate() {
        let cy = top + row * i as f32 + row / 2.;
        let label = Block::new(&t.name, 0., small, false, m);
        if t.vert {
            let vx = x(t.start);
            doc.line((vx, top), (vx, axis_y), &Paint { dash: Some("4 3".into()), ..Paint::new("none", &col.line, 1.5) });
            doc.text(&label, vx + 4. + label.w / 2., cy, Anchor::Middle, &col.text);
            continue
        }
        let (fill, stroke) = if t.crit { (series(4), col.node_border.as_str()) } else if t.done { (col.cluster_fill.as_str(), col.cluster_border.as_str()) } else if t.active { (col.node_fill.as_str(), col.node_border.as_str()) } else { (series(0), col.node_border.as_str()) };
        let p = Paint::new(fill, stroke, if t.crit { 1.5 } else { 1. });
        let (bx0, bx1) = (x(t.start), x(t.end));
        let lx = if t.milestone {
            let mx = (bx0 + bx1) / 2.;
            let r = bar * 0.55;
            doc.polygon(&[(mx, cy - r), (mx + r, cy), (mx, cy + r), (mx - r, cy)], &p);
            mx + r + 6.
        } else {
            doc.rect(bx0, cy - bar / 2., (bx1 - bx0).max(2.), bar, 3., &p);
            // the name inside when it fits, else after the bar
            if label.w + 12. <= bx1 - bx0 { doc.text(&label, (bx0 + bx1) / 2., cy, Anchor::Middle, &col.text); continue }
            bx1 + 6.
        };
        // (past the chart's right edge: before the bar instead)
        if lx + label.w > width - margin { doc.text(&label, bx0 - 6. - label.w / 2., cy, Anchor::Middle, &col.text); } else { doc.text(&label, lx, cy, Anchor::Start, &col.text); }
    }
    Ok(Svg { svg: doc.finish(width, height, ""), width, height })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> Result<Gantt, Error> { let (l, _) = crate::lines(src); parse(&l) }
    fn d(s: &str) -> f64 { parse_date(s, "YYYY-MM-DD").unwrap() }

    #[test]
    fn calendar() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2014, 1, 6), 16076);
        for z in [-800000, -1, 0, 59, 16076, 20000, 2932896] { let (y, m, dd) = civil(z); assert_eq!(days_from_civil(y, m, dd), z); }
        assert_eq!(weekday(days_from_civil(2014, 1, 6)), 1, "a Monday");
        assert_eq!(parse_date("17:49", "HH:mm"), Some(17. * 3600. + 49. * 60.));
        assert_eq!(parse_date("2014-1-6", "YYYY-M-D"), Some(d("2014-01-06")));
        assert_eq!(parse_date("2014-13-06", "YYYY-MM-DD"), None);
        assert_eq!(parse_date("71", "X"), Some(71.));
        assert_eq!(duration("1.5w"), Some(1.5 * 7. * DAY));
        assert_eq!(duration("20h"), Some(20. * 3600.));
        assert_eq!(duration("x"), None);
        assert_eq!(strftime(d("2014-01-06") + 3600. * 13.5, "%Y-%m-%d %H:%M %a %b %e %j"), "2014-01-06 13:30 Mon Jan 6 006");
    }

    #[test]
    fn tasks_placed() {
        let g = p("gantt\n  dateFormat YYYY-MM-DD\n  section A\n  First :a1, 2014-01-01, 30d\n  Second :after a1, 20d\n  section B\n  Third :2014-01-12, 12d\n  Fourth :24d\n  Fifth :crit, done, x, 2014-01-06, 2014-01-08\n  Until :until m\n  Mile :milestone, m, 2014-03-01, 0d").unwrap();
        let t = &g.tasks;
        assert_eq!((t[0].start, t[0].end), (d("2014-01-01"), d("2014-01-31")));
        assert_eq!((t[1].start, t[1].end), (d("2014-01-31"), d("2014-02-20")));
        assert_eq!(t[3].start, d("2014-01-24"));
        assert!(t[4].crit && t[4].done && t[4].id.as_deref() == Some("x"));
        assert_eq!((t[5].start, t[5].end), (d("2014-01-08"), d("2014-03-01")));
        assert!(t[6].milestone);
        assert_eq!(g.sections, ["A", "B"]);
        assert_eq!((t[1].section, t[2].section), (0, 1));
    }

    #[test]
    fn weekends_are_skipped() {
        // Friday 2014-01-03, 2 days of work: Friday, Monday
        let g = p("gantt\n  excludes weekends\n  T :2014-01-03, 2d").unwrap();
        assert_eq!(g.tasks[0].end, d("2014-01-07"));
        let g = p("gantt\n  excludes weekends\n  weekend friday\n  T :2014-01-02, 2d").unwrap();
        assert_eq!(g.tasks[0].end, d("2014-01-06"));
        let g = p("gantt\n  excludes 2014-01-03, sunday\n  T :2014-01-02, 3d").unwrap();
        assert_eq!(g.tasks[0].end, d("2014-01-07"));
    }

    #[test]
    fn ticks_land_on_round_times() {
        let m = ticks(d("2014-01-15"), d("2014-05-02"), (1., "M"));
        assert_eq!(m, [d("2014-02-01"), d("2014-03-01"), d("2014-04-01"), d("2014-05-01")]);
        let w = ticks(d("2014-01-01"), d("2014-01-20"), (7. * DAY, "s"));
        assert!(w.iter().all(|&t| weekday((t / DAY) as i64) == 0));
    }

    #[test]
    fn mistakes_say_where() {
        assert_eq!(p("gantt\n  T :2014-01-03").unwrap_err().line, 2);
        assert_eq!(p("gantt\n  T :a, 2014-31-03, 1d").unwrap_err().line, 2);
        assert_eq!(p("gantt\n  T :3d").unwrap_err().line, 2);
        assert_eq!(p("gantt\n  T :2014-01-01, 1d\n  U :after zz, 1d").unwrap_err().line, 3);
        assert_eq!(p("gantt\n  T :a, after b, 1d\n  U :b, after a, 1d").unwrap_err().line, 2);
        assert_eq!(p("gantt\n  just words").unwrap_err().line, 2);
        assert_eq!(p("gantt\n  title x").unwrap_err().line, 0);
        assert_eq!(p("gantt\n  excludes someday\n  T :2014-01-01, 1d").unwrap_err().line, 2);
    }
}
