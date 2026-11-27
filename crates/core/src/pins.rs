//! 置顶's own order, across every top-level folder: each pin's `pin_order` is a fractional
//! index ("a0V", as the hand order's, sync.md 3.3), so placing one writes that pin alone and two
//! devices placing two never undo each other. A pin without one (made before 置顶 had an order,
//! or by an older build) comes after the placed, newest first (`Meta::pinned_at`), then as 0.1.14
//! listed them (last changed first). A new pin goes on top, as in the Finder's sidebar and Notes:
//! what was just pinned is what is about to be opened.

use crate::board::fractional_index::{generate_key_between, generate_n_keys_between, validate_order_key};
use crate::store::{Entry, Meta};
use std::cmp::Ordering;

fn key(m: &Meta) -> Option<&str> { m.pin_order.as_deref().filter(|k| validate_order_key(k).is_ok()) }

/// 置顶's order: the placed by key, then the rest newest first.
pub fn cmp(a: &Entry, b: &Entry) -> Ordering {
    match (key(&a.meta), key(&b.meta)) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => b.meta.pinned_at.cmp(&a.meta.pinned_at).then(b.meta.modified.cmp(&a.meta.modified)),
    }.then_with(|| a.id.cmp(&b.id))
}

/// The pinned of `entries`, in 置顶's order.
pub fn sorted<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Vec<&'a Entry> {
    let mut v: Vec<&Entry> = entries.into_iter().filter(|e| e.meta.pinned).collect();
    v.sort_by(|a, b| cmp(a, b));
    v
}

/// The key that puts a new pin above `pins` (in 置顶's order).
pub fn top_key(pins: &[&Entry]) -> String {
    let first = pins.iter().find_map(|e| key(&e.meta));
    generate_key_between(None, first).or_else(|_| generate_key_between(None, None)).unwrap_or_else(|_| "a0".into())
}

/// The pins `(id, key)` in the order wanted: the keys to write, (id, key) for each that changes.
/// The longest run already in order keeps its keys, the others get keys between their
/// neighbours (a pin moved: that pin alone; the first move among unplaced pins places them all).
pub fn keys_for(order: &[(&str, Option<&str>)]) -> Vec<(String, String)> {
    let old: Vec<Option<&str>> = order.iter().map(|(_, k)| k.filter(|k| validate_order_key(k).is_ok())).collect();
    let keep = longest_run(order, &old);
    let mut new: Vec<Option<String>> = old.iter().zip(&keep).map(|(k, keep)| if *keep { k.map(str::to_string) } else { None }).collect();
    let filled = (|| -> Option<()> {
        let mut i = 0;
        while i < new.len() {
            if new[i].is_some() { i += 1; continue; }
            let end = (i..new.len()).find(|&j| new[j].is_some()).unwrap_or(new.len());
            let (a, b) = (i.checked_sub(1).and_then(|p| new[p].clone()), new.get(end).cloned().flatten());
            if a.is_some() && a == b { return None; }
            for (j, k) in generate_n_keys_between(a.as_deref(), b.as_deref(), end - i).ok()?.into_iter().enumerate() { new[i + j] = Some(k); }
            i = end;
        }
        Some(())
    })();
    if filled.is_none() { new = generate_n_keys_between(None, None, order.len()).unwrap_or_default().into_iter().map(Some).collect(); }
    order.iter().zip(new).filter_map(|((id, was), k)| { let k = k?; (*was != Some(k.as_str())).then(|| (id.to_string(), k)) }).collect()
}

/// Which keys to keep: a longest run increasing by (key, id), the order `cmp` reads them in.
fn longest_run(order: &[(&str, Option<&str>)], old: &[Option<&str>]) -> Vec<bool> {
    let at: Vec<usize> = (0..old.len()).filter(|&i| old[i].is_some()).collect();
    let less = |a: usize, b: usize| (old[a], order[a].0) < (old[b], order[b].0);
    let (mut tails, mut prev): (Vec<usize>, Vec<Option<usize>>) = (vec![], vec![None; old.len()]);
    for &i in &at {
        let l = tails.partition_point(|&t| less(t, i));
        prev[i] = l.checked_sub(1).map(|p| tails[p]);
        if l == tails.len() { tails.push(i) } else { tails[l] = i }
    }
    let mut keep = vec![false; old.len()];
    let mut cur = tails.last().copied();
    while let Some(i) = cur { keep[i] = true; cur = prev[i]; }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(id: &str, key: Option<&str>, at: u64, modified: u64) -> Entry {
        let mut e = Entry { id: id.into(), meta: Meta::default(), title: String::new(), icon: None, body: Default::default(), reminders: vec![], links: vec![], note: None, top: String::new() };
        e.meta.pinned = true; e.meta.pin_order = key.map(str::to_string); e.meta.pinned_at = at; e.meta.modified = modified;
        e
    }
    fn ids(v: &[&Entry]) -> Vec<String> { v.iter().map(|e| e.id.clone()).collect() }

    #[test]
    fn placed_pins_first_then_the_newest() {
        let v = [pin("old", None, 10, 5), pin("new", None, 20, 1), pin("b", Some("a2"), 0, 0), pin("a", Some("a1"), 0, 0),
            // 0.1.14's pins share the upgrade's stamp: the last changed first, as 0.1.14 listed them
            pin("x", None, 10, 9)];
        assert_eq!(ids(&sorted(&v)), ["a", "b", "new", "x", "old"]);
        // a new pin goes above them all
        let s = sorted(&v);
        let k = top_key(&s);
        assert!(k.as_str() < "a1");
    }

    #[test]
    fn a_move_writes_the_moved_pin_alone() {
        // c dragged between a and b
        let w = keys_for(&[("a", Some("a1")), ("c", Some("a3")), ("b", Some("a2"))]);
        assert_eq!(w.len(), 1);
        assert!(w[0].0 == "c" || w[0].0 == "b");
        // the first move among unplaced pins places every one, in the order shown
        let w = keys_for(&[("y", None), ("x", None), ("z", None)]);
        assert_eq!(w.len(), 3);
        assert!(w[0].1 < w[1].1 && w[1].1 < w[2].1);
        // an unplaced pin moved above placed ones: that one alone
        let w = keys_for(&[("n", None), ("a", Some("a1")), ("b", Some("a2"))]);
        assert_eq!(w.len(), 1);
        assert!(w[0].0 == "n" && w[0].1.as_str() < "a1");
        // equal keys (two devices placed two at once): fresh ones between
        let w = keys_for(&[("a", Some("a1")), ("n", None), ("b", Some("a1"))]);
        let mut got: std::collections::HashMap<&str, &str> = [("a", "a1"), ("b", "a1")].into_iter().collect();
        for (id, k) in &w { got.insert(id, k); }
        got.insert("n", w.iter().find(|x| x.0 == "n").map(|x| x.1.as_str()).unwrap());
        assert!(got["a"] < got["n"] && got["n"] < got["b"]);
    }
}
