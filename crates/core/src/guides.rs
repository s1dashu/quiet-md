//! The 开始指南 built into the binaries (shared-mock/library, built by
//! shared-mock/guides/build.py; crates/core/build.rs embeds it): the app seeds
//! a new library with them (its `guides.rs`), and 导入最新的开始指南 in the
//! app and `quiet guide import` put them into a folder of their own
//! (`Store::import_guides`).

include!(concat!(env!("OUT_DIR"), "/guides.rs"));
/// The Simplified Chinese guides (`build.py --lang zh`): documents and index
/// only, the images are the English library's.
pub mod zh { include!(concat!(env!("OUT_DIR"), "/guides_zh.rs")); }

/// Where the guides keep their pictures and attachments, from the library's top.
pub const ASSETS: &str = ".assets/guides/";

/// The guides for the interface language: Simplified Chinese gets the
/// Chinese ones, every other language the English ones.
pub fn files(chinese: bool) -> Vec<(&'static str, &'static [u8])> {
    if !chinese { return FILES.to_vec(); }
    zh::FILES.iter().chain(FILES.iter().filter(|(n, _)| n.starts_with(".assets/"))).copied().collect()
}

/// The folder an import makes, by the guides' language (the app's other
/// languages name it through their own tables).
pub fn folder_name(chinese: bool) -> &'static str { if chinese { "Quiet.md 指南" } else { "Quiet.md Guide" } }
