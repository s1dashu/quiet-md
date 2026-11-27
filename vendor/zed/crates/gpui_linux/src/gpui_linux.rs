#![cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;

pub use linux::{current_platform, linux_platform};
#[cfg(any(feature = "wayland", feature = "x11"))]
pub use linux::system_ui_font_family;
