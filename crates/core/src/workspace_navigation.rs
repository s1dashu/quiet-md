//! Navigation history is separate from edit undo.
#[derive(Clone, Debug)]
pub struct History<T> { entries: Vec<T>, cursor: usize }
impl<T: Clone + PartialEq> Default for History<T> { fn default() -> Self { Self { entries: vec![], cursor: 0 } } }
impl<T: Clone + PartialEq> History<T> {
    pub fn push(&mut self, value: T) {
        if self.entries.get(self.cursor) == Some(&value) { return; }
        if !self.entries.is_empty() { self.entries.truncate(self.cursor + 1); }
        self.entries.push(value);
        if self.entries.len() > 200 { self.entries.remove(0); }
        self.cursor = self.entries.len() - 1;
    }
    pub fn target(&self, forward: bool) -> Option<T> {
        let i = if forward { self.cursor.checked_add(1)? } else { self.cursor.checked_sub(1)? };
        self.entries.get(i).cloned()
    }
    pub fn travel(&mut self, forward: bool) -> Option<T> {
        let target = self.target(forward)?;
        if forward { self.cursor += 1; } else { self.cursor -= 1; }
        Some(target)
    }
}
#[derive(Default)]
pub struct Swipe { x: f32, y: f32, active: bool, fired: bool }
impl Swipe {
    pub fn start(&mut self) { *self = Self { active: true, ..Self::default() }; }
    pub fn end(&mut self) { self.active = false; }
    pub fn update(&mut self, x: f32, y: f32) -> Option<bool> {
        if !self.active || self.fired { return None; }
        self.x += x; self.y += y.abs();
        if self.y > 32. { self.fired = true; return None; }
        if self.x.abs() >= 100. && self.x.abs() > self.y * 3. {
            self.fired = true; return Some(self.x < 0.);
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_navigation_branch_and_gesture() {
        let mut h = History::default(); h.push("a"); h.push("b"); h.push("c");
        assert_eq!(h.travel(false), Some("b")); assert_eq!(h.travel(true), Some("c"));
        h.travel(false); h.push("d"); assert_eq!(h.target(true), None); assert_eq!(h.travel(false), Some("b"));
        let mut s = Swipe::default(); assert_eq!(s.update(200., 0.), None);
        s.start(); assert_eq!(s.update(60., 1.), None); assert_eq!(s.update(60., 1.), Some(false)); assert_eq!(s.update(200., 0.), None);
        s.end(); assert_eq!(s.update(-200., 0.), None);
        s.start(); s.update(10., 40.); assert_eq!(s.update(200., 0.), None);
        s.start(); assert_eq!(s.update(-120., 2.), Some(true));
    }
}
