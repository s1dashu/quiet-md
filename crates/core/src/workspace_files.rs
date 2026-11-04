//! Filesystem workspaces never initialize or import into a notebook library.
use anyhow::{Context, Result, bail, ensure};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}};

pub const MAX_SOURCE: u64 = 2 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf, pub directory: bool,
    /// A symlink. Inside the folder it can be opened; pointing outside it stays listed but closed.
    pub link: bool, pub outside: bool,
}
/// One directory's rows, and what was left out so the tree can say so.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing { pub entries: Vec<Entry>, pub other_files: usize, pub truncated: bool }
const MAX_ROWS: usize = 5_000;

pub fn root(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    ensure!(path.is_dir(), "Select a folder");
    Ok(path)
}
pub fn contained(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let path = path.canonicalize()?;
    ensure!(path.starts_with(root), "Path leaves the open folder");
    Ok(path)
}
pub fn supported(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdx" | "txt"))
}
/// A real directory listing: `.git` never; dotfiles and whatever Git ignores only
/// when asked; no recursion, so a huge tree costs nothing until it is expanded.
pub fn list(root: &Path, path: &Path, show_hidden: bool) -> Result<Listing> {
    let dir = contained(root, path)?;
    let mut out = Listing::default();
    let walk = ignore::WalkBuilder::new(&dir).max_depth(Some(1)).hidden(!show_hidden).git_ignore(!show_hidden).git_global(!show_hidden).git_exclude(!show_hidden).ignore(!show_hidden).parents(!show_hidden).require_git(true).follow_links(false).build();
    for entry in walk {
        let Ok(entry) = entry else { continue };
        if entry.depth() == 0 || entry.file_name() == ".git" { continue; }
        let Some(kind) = entry.file_type() else { continue };
        let path = entry.path().to_path_buf();
        let row = if kind.is_symlink() {
            let target = fs::canonicalize(&path);
            let outside = target.as_ref().map_or(true, |t| !t.starts_with(root));
            let directory = target.as_ref().is_ok_and(|t| t.is_dir());
            if !directory && !supported(&path) { out.other_files += 1; continue; }
            Entry { path, directory, link: true, outside }
        } else if kind.is_dir() { Entry { path, directory: true, link: false, outside: false } }
        else if kind.is_file() && supported(&path) { Entry { path, directory: false, link: false, outside: false } }
        else { out.other_files += 1; continue; };
        if out.entries.len() >= MAX_ROWS { out.truncated = true; continue; }
        out.entries.push(row);
    }
    out.entries.sort_by(|a, b| b.directory.cmp(&a.directory).then_with(|| a.path.to_string_lossy().to_lowercase().cmp(&b.path.to_string_lossy().to_lowercase())));
    Ok(out)
}
/// Every document under `root` for quick open, on a background thread.
pub fn index(root: &Path, limit: usize) -> (Vec<PathBuf>, bool) {
    let (mut out, mut truncated) = (vec![], false);
    for entry in ignore::WalkBuilder::new(root).follow_links(false).require_git(true).build().flatten() {
        if entry.file_type().is_some_and(|t| t.is_file()) && supported(entry.path()) {
            if out.len() >= limit { truncated = true; break; }
            out.push(entry.into_path());
        }
    }
    out.sort();
    (out, truncated)
}
pub fn read(path: &Path) -> Result<String> {
    ensure!(fs::metadata(path)?.is_file(), "Not a regular file");
    let mut bytes = vec![];
    fs::File::open(path)?.take(MAX_SOURCE + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_SOURCE, "File exceeds the 2 MiB editing limit");
    String::from_utf8(bytes).context("This file is not UTF-8; it was not changed")
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub path: PathBuf,
    pub text: String,
    pub baseline: String,
    pub revision: u64,
    #[serde(skip)] undo: Vec<String>,
    #[serde(skip)] redo: Vec<String>,
}
impl Source {
    pub fn open(root: &Path, path: &Path) -> Result<Self> {
        let path = contained(root, path)?;
        let text = read(&path)?;
        Ok(Self { path, baseline: text.clone(), text, revision: 0, undo: vec![], redo: vec![] })
    }
    pub fn dirty(&self) -> bool { self.text != self.baseline }
    pub fn edit(&mut self, text: String) {
        if self.text == text { return; }
        self.undo.push(std::mem::replace(&mut self.text, text));
        let mut bytes: usize = self.undo.iter().map(String::len).sum();
        while self.undo.len() > 200 || (bytes > 32 * 1024 * 1024 && self.undo.len() > 1) { bytes -= self.undo.remove(0).len(); }
        self.redo.clear(); self.revision += 1;
    }
    pub fn undo(&mut self, redo: bool) -> bool {
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        if let Some(text) = from.pop() { to.push(std::mem::replace(&mut self.text, text)); self.revision += 1; true } else { false }
    }
    pub fn reconcile(&mut self) -> Result<bool> {
        let disk = read(&self.path)?;
        if disk == self.baseline { return Ok(false); }
        if disk == self.text { self.baseline = disk; return Ok(false); }
        ensure!(!self.dirty(), "File changed on disk. Your edits are retained; save a copy or reload explicitly.");
        self.text = disk.clone(); self.baseline = disk; self.undo.clear(); self.redo.clear(); self.revision += 1;
        Ok(true)
    }
    pub fn save(&mut self, root: &Path) -> Result<()> {
        if !self.dirty() { return Ok(()); }
        ensure!(self.text.len() as u64 <= MAX_SOURCE, "File exceeds the 2 MiB editing limit; your edits are retained");
        ensure!(contained(root, &self.path)? == self.path, "File target changed");
        let disk = read(&self.path)?;
        ensure!(disk == self.baseline || disk == self.text, "File changed on disk. Your edits are retained; save a copy or reload explicitly.");
        let permissions = fs::metadata(&self.path)?.permissions();
        ensure!(!permissions.readonly(), "File is read-only");
        let temp = self.path.with_file_name(format!(".quiet-save-{}", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
            f.write_all(self.text.as_bytes())?; f.set_permissions(permissions)?; f.sync_all()?;
            // Recheck after writing the temporary file; do not overwrite a known concurrent edit.
            ensure!(read(&self.path)? == disk, "File changed during save; retry after reviewing it");
            fs::rename(&temp, &self.path)?;
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(&temp); }
        result?; self.baseline = self.text.clone(); Ok(())
    }
    /// Write the retained text back to a path that no longer exists.
    pub fn restore(&mut self) -> Result<()> {
        if let Some(parent) = self.path.parent() { fs::create_dir_all(parent)?; }
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&self.path)?;
        f.write_all(self.text.as_bytes())?; f.sync_all()?;
        self.baseline = self.text.clone(); Ok(())
    }
    /// Replace the content from disk: one undoable step back to what was here.
    pub fn replace(&mut self, disk: String) { self.edit(disk.clone()); self.baseline = disk; }
    pub fn recover(&self, directory: &Path) -> Result<PathBuf> {
        fs::create_dir_all(directory)?;
        let path = directory.join(recovery_name(&self.path));
        let temp = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let bytes = serde_json::to_vec(self)?;
        ensure!(bytes.len() <= 64 * 1024 * 1024, "Recovery file is too large");
        fs::write(&temp, bytes)?;
        fs::rename(temp, &path)?; Ok(path)
    }
    pub fn recovery(root: &Path, path: &Path, directory: &Path) -> Result<Option<Self>> {
        let path = match contained(root, path) {
            Ok(path) => path,
            Err(e) if !path.exists() => {
                let mut parent = path; let mut suffix = vec![];
                while !parent.exists() {
                    suffix.push(parent.file_name().ok_or_else(|| anyhow::anyhow!("Invalid recovery path"))?.to_os_string());
                    parent = parent.parent().ok_or_else(|| anyhow::anyhow!("Invalid recovery path"))?;
                }
                let mut resolved = contained(root, parent).map_err(|_| e)?;
                for part in suffix.into_iter().rev() { resolved.push(part); }
                resolved
            }
            Err(e) => return Err(e),
        };
        let recovery = directory.join(recovery_name(&path));
        if !recovery.exists() { return Ok(None); }
        ensure!(fs::metadata(&recovery)?.len() <= 64 * 1024 * 1024, "Recovery file is too large");
        let source: Self = serde_json::from_slice(&fs::read(recovery)?)?;
        ensure!(source.path == path, "Recovery path mismatch");
        Ok(source.dirty().then_some(source))
    }
    pub fn clear_recovery(&self, directory: &Path) { let _ = fs::remove_file(directory.join(recovery_name(&self.path))); }
}
fn recovery_name(path: &Path) -> String {
    // Stable across application versions; no process-random hasher seed.
    #[cfg(unix)]
    let bytes = { use std::os::unix::ffi::OsStrExt; path.as_os_str().as_bytes().to_vec() };
    #[cfg(windows)]
    let bytes = { use std::os::windows::ffi::OsStrExt; path.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect::<Vec<_>>() };
    #[cfg(not(any(unix, windows)))]
    let bytes = path.to_string_lossy().as_bytes().to_vec();
    let hash = bytes.into_iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("{hash:016x}.json")
}
pub fn relative_link(root: &Path, file: &Path, link: &str) -> Result<(PathBuf, Option<String>)> {
    let (path, anchor) = link.split_once('#').map_or((link, None), |(p, a)| (p, Some(a.to_string())));
    ensure!(!path.contains(':') && !path.starts_with('/') && !path.starts_with('\\'), "Not a relative file link");
    let decoded = percent_decode(path)?;
    let path = if decoded.is_empty() { file.to_path_buf() } else { file.parent().unwrap_or(root).join(decoded) };
    Ok((contained(root, &path)?, anchor))
}
fn percent_decode(s: &str) -> Result<String> {
    let mut bytes = vec![]; let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let (Some(a), Some(b)) = (iter.next(), iter.next()) else { bail!("Invalid URL escape") };
            let s = [a, b]; bytes.push(u8::from_str_radix(std::str::from_utf8(&s)?, 16)?);
        } else { bytes.push(b); }
    }
    Ok(String::from_utf8(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_files_preserve_conflicts_undo_and_recovery() {
        let root = std::env::temp_dir().join(format!("quiet-files-{}", uuid::Uuid::new_v4())); fs::create_dir(&root).unwrap();
        let path = root.join("a.mdx"); let text = "\u{feff}---\r\ntitle: Test\r\n---\r\nimport X from './x';\r\n<X x={2}/>\r\n";
        fs::write(&path, text).unwrap(); let mut source = Source::open(&root, &path).unwrap();
        source.save(&root).unwrap(); assert_eq!(fs::read(&path).unwrap(), text.as_bytes());
        source.edit(format!("{text}new")); assert!(source.undo(false)); assert_eq!(source.text, text); assert!(source.undo(true));
        let recovery = root.join("recovery"); source.recover(&recovery).unwrap();
        assert_eq!(Source::recovery(&root, &path, &recovery).unwrap().unwrap().text, source.text);
        fs::write(&path, "external").unwrap(); assert!(source.save(&root).is_err()); assert!(source.reconcile().is_err());
        assert_eq!(read(&path).unwrap(), "external");
        source.baseline = "external".into(); source.save(&root).unwrap(); assert_eq!(read(&path).unwrap(), source.text);
        fs::write(&path, "clean reload").unwrap(); assert!(source.reconcile().unwrap()); assert_eq!(source.text, "clean reload");
        fs::write(root.join("bad.md"), [0xff]).unwrap(); assert!(Source::open(&root, &root.join("bad.md")).is_err());
        assert!(relative_link(&root, &path, "../outside.md").is_err());
        assert!(relative_link(&root, &path, "a.mdx#heading").is_ok());
        fs::write(root.join("a space.md"), "# Heading").unwrap();
        let (target, anchor) = relative_link(&root, &path, "a%20space.md#heading").unwrap();
        assert_eq!(target, root.join("a space.md").canonicalize().unwrap()); assert_eq!(anchor.as_deref(), Some("heading"));
        assert!(relative_link(&root, &path, "https://example.com").is_err());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(std::env::temp_dir(), root.join("escape")).unwrap();
            assert!(contained(&root, &root.join("escape")).is_err());
        }
        source.edit("x".repeat(MAX_SOURCE as usize + 1));
        assert!(source.save(&root).is_err()); assert_eq!(read(&path).unwrap(), "clean reload");
        source.edit("recover missing file".into()); source.recover(&recovery).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(Source::recovery(&root, &path, &recovery).unwrap().unwrap().text, "recover missing file");
        let nested = root.join("removed/sub"); fs::create_dir_all(&nested).unwrap();
        let nested_path = nested.join("note.md"); fs::write(&nested_path, "original").unwrap();
        let mut nested_source = Source::open(&root, &nested_path).unwrap(); nested_source.edit("retained".into()); nested_source.recover(&recovery).unwrap();
        fs::remove_dir_all(root.join("removed")).unwrap();
        assert_eq!(Source::recovery(&root, &nested_path, &recovery).unwrap().unwrap().text, "retained");
        let listed = root.join("tree"); fs::create_dir(&listed).unwrap();
        for name in ["b.md", "A.mdx", "photo.png", ".dot.md"] { fs::write(listed.join(name), "x").unwrap(); }
        fs::create_dir(listed.join("sub")).unwrap();
        let listing = list(&root.canonicalize().unwrap(), &listed, false).unwrap();
        let names = listing.entries.iter().map(|e| e.path.file_name().unwrap().to_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names, ["sub", "A.mdx", "b.md"]); assert_eq!(listing.other_files, 1);
        assert!(list(&root.canonicalize().unwrap(), &listed, true).unwrap().entries.iter().any(|e| e.path.ends_with(".dot.md")));
        assert_eq!(index(&listed, 10).0.len(), 2);
        #[cfg(unix)] {
            std::os::unix::fs::symlink(std::env::temp_dir(), listed.join("away")).unwrap();
            std::os::unix::fs::symlink("b.md", listed.join("near.md")).unwrap();
            let rows = list(&root.canonicalize().unwrap(), &listed, false).unwrap().entries;
            assert!(rows.iter().find(|e| e.path.ends_with("away")).unwrap().outside);
            let near = rows.iter().find(|e| e.path.ends_with("near.md")).unwrap(); assert!(near.link && !near.outside);
        }
        let gone = listed.join("restored/x.md"); let mut lost = Source { path: gone.clone(), text: "kept".into(), baseline: "old".into(), revision: 0, undo: vec![], redo: vec![] };
        lost.restore().unwrap(); assert_eq!(read(&gone).unwrap(), "kept"); assert!(!lost.dirty()); assert!(lost.restore().is_err());
        assert!(!root.join("docs").exists()); assert!(!root.join("format.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
