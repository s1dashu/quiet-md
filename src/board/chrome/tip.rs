//! Tooltips: the app's (`ui::Tooltip`, shadcn's), with upstream's labels
//! ("Rectangle — R or 2", Excalidraw `Tooltip.tsx` / the buttons' `title` @
//! 3a317b5): below a control at the top, above one at the bottom, flipped
//! when there's no room.

use crate::ui::{TipExt, TipSide};

pub trait ExTip: TipExt {
    /// The tooltip `label`, with ` — keys` when there's a shortcut, below.
    fn ex_tip(self, label: &str, keys: Option<&str>) -> Self { self.tip_label(text(label, keys), TipSide::Below) }
    /// The same, above (the footer's buttons).
    fn ex_tip_up(self, label: &str, keys: Option<&str>) -> Self { self.tip_label(text(label, keys), TipSide::Above) }
}

impl<T: TipExt> ExTip for T {}

fn text(label: &str, keys: Option<&str>) -> String { match keys { Some(k) => format!("{label} — {k}"), None => label.to_string() } }
