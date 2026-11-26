//! What the board keeps of Excalidraw's UI tokens (`packages/excalidraw/css/
//! theme.scss`, `variables.module.scss` @ 3a317b5), light or dark by the app's
//! look: the canvas's own marks (the selection, a frame's name) and where the
//! chrome sits. The chrome's looks are the app's design system
//! (`crate::design`). Sizes are px at upstream's 16px rem.

use gpui::{Hsla, Rgba};

fn dark() -> bool { crate::theme::dark() }
pub fn hex(c: u32) -> Hsla { Rgba { r: (c >> 16 & 0xff) as f32 / 255., g: (c >> 8 & 0xff) as f32 / 255., b: (c & 0xff) as f32 / 255., a: 1. }.into() }
fn pick(light: u32, dark_: u32) -> Hsla { hex(if dark() { dark_ } else { light }) }

/// `--ui-font` (agent B registers the bundled Assistant under this name; GPUI
/// falls back to the system face until then): a frame's name on the canvas.
pub const UI_FONT: &str = "Assistant";

/// `--editor-container-padding`: the chrome's distance from the board's edges.
pub const PADDING: f32 = 16.;
/// Above the top row (menu, toolbar): ours, as the board sits under the
/// app's header, which already leaves room (upstream: `PADDING`).
pub const TOP_PADDING: f32 = 8.;

/// `--color-primary`.
pub fn primary() -> Hsla { pick(0x6965db, 0xa8a5ff) }
/// `--color-selection`: the interactive canvas's selection color.
pub fn selection() -> Hsla { pick(0x6965db, 0xb4b0ff) }
