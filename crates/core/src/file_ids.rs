//! Which file on this computer's disk each listed page and folder is (docs/FORMAT.md,
//! `.quiet/local/`): a file renamed or moved where the library did not do it (the Finder, a
//! top-level folder nested in or holding it), while the app ran or while it was closed, is
//! known by it where it turns up, and keeps its id and all it had (pins, order …).
//!
//! The system's number for the file (the inode; the NTFS file index on Windows) is used
//! again once a file is deleted, so a match needs more: the time the file was made where the
//! system keeps it (macOS, Windows, most Linux disks), else its size. A file moved to
//! another disk is a new file there (another `dev`), as is one written anew in its place
//! (an editor that saves by replacing the file), which only its path tells.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileId {
    /// The disk (volume serial number on Windows) and the file's number on it.
    pub dev: u64,
    pub ino: u64,
    /// When the file was made, in nanoseconds since 1970, where the system says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub born: Option<u64>,
    #[serde(default)]
    pub size: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dir: bool,
    /// Its modification time when looked at: unchanged at the same path, it is not looked at
    /// again (on Windows that means opening the file).
    #[serde(default)]
    pub at: u64,
}

impl FileId {
    /// Whether `now` is the file this was: the same number on the same disk, made at the
    /// same time (or, where the system does not keep that, a file of the same size).
    pub fn same_file(&self, now: &FileId) -> bool {
        self.dev == now.dev && self.ino == now.ino && self.dir == now.dir
            && match (self.born, now.born) { (Some(a), Some(b)) => a == b, (None, None) => !self.dir && self.size == now.size, _ => false }
    }
}

fn nanos(t: std::io::Result<web_time::SystemTime>) -> Option<u64> {
    t.ok()?.duration_since(web_time::UNIX_EPOCH).ok().map(|d| d.as_nanos() as u64)
}

/// The file at `path` (with its metadata `m`), where the system numbers its files.
pub fn of(path: &Path, m: &crate::vfs::Metadata) -> Option<FileId> {
    let (dev, ino) = number(path, m)?;
    Some(FileId { dev, ino, born: nanos(m.created()), size: if m.is_dir() { 0 } else { m.len() }, dir: m.is_dir(), at: nanos(m.modified()).unwrap_or_default() })
}

/// `of`, taking what was seen before when the file is unchanged at the same path (`at`, size).
pub fn look(before: Option<&Seen>, rel: &str, path: &Path, m: &crate::vfs::Metadata) -> Option<Seen> {
    let at = nanos(m.modified()).unwrap_or_default();
    if let Some(b) = before.filter(|b| b.path == rel && at != 0 && b.id.at == at && b.id.dir == m.is_dir() && (b.id.dir || b.id.size == m.len())) { return Some(b.clone()) }
    of(path, m).map(|id| Seen { path: rel.to_string(), id })
}

#[cfg(unix)]
fn number(_: &Path, m: &crate::vfs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((m.dev(), m.ino()))
}

#[cfg(windows)]
fn number(path: &Path, _: &crate::vfs::Metadata) -> Option<(u64, u64)> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle};
    // Opened for its attributes only, not to read: a folder needs the backup flag.
    let f = std::fs::OpenOptions::new().access_mode(0).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(path).ok()?;
    // SAFETY: a plain C struct of integers and times, for which all zeroes is a value.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: a handle this function owns while the call runs, and a struct of the right size.
    if unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut info) } == 0 { return None }
    Some((info.dwVolumeSerialNumber as u64, ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64))
}

#[cfg(not(any(unix, windows)))]
fn number(_: &Path, _: &crate::vfs::Metadata) -> Option<(u64, u64)> { None }

/// One page's or folder's file as last seen: its path in the library and which file it was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub path: String,
    #[serde(flatten)]
    pub id: FileId,
}

/// `.quiet/local/<device>/files.json`: this computer's, never merged or read by another device
/// (the numbers are this disk's); a build that does not know it leaves it alone.
#[derive(Default, Serialize, Deserialize)]
pub struct Table {
    #[serde(default)]
    pub v: u32,
    /// By id.
    #[serde(default)]
    pub files: BTreeMap<String, Seen>,
}

pub fn file(internal: &Path, device: &str) -> std::path::PathBuf { internal.join("local").join(device).join("files.json") }

pub fn load(path: &Path) -> BTreeMap<String, Seen> {
    crate::vfs::read(path).ok().and_then(|b| serde_json::from_slice::<Table>(&b).ok()).map(|t| t.files).unwrap_or_default()
}

pub fn save(path: &Path, files: &BTreeMap<String, Seen>) -> anyhow::Result<()> {
    #[derive(Serialize)]
    struct Out<'a> { v: u32, files: &'a BTreeMap<String, Seen> }
    let bytes = serde_json::to_vec(&Out { v: 1, files })?;
    if crate::vfs::read(path).is_ok_and(|b| b == bytes) { return Ok(()) }
    if let Some(d) = path.parent() { crate::vfs::create_dir_all(d)?; }
    let tmp = path.with_extension("json.tmp");
    crate::vfs::write(&tmp, &bytes)?;
    crate::vfs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(ino: u64, born: Option<u64>, size: u64) -> FileId { FileId { dev: 1, ino, born, size, dir: false, at: 0 } }

    #[test]
    fn a_number_used_again_is_another_file() {
        assert!(id(7, Some(100), 3).same_file(&id(7, Some(100), 9)), "edited since: the same file");
        assert!(!id(7, Some(100), 3).same_file(&id(7, Some(200), 3)), "deleted, its number given to a new one");
        assert!(!id(7, Some(100), 3).same_file(&id(8, Some(100), 3)));
        assert!(!id(7, Some(100), 3).same_file(&FileId { dev: 2, ..id(7, Some(100), 3) }), "another disk");
        assert!(id(7, None, 3).same_file(&id(7, None, 3)) && !id(7, None, 3).same_file(&id(7, None, 4)), "no birth time: the size");
        assert!(!id(7, Some(1), 3).same_file(&id(7, None, 3)));
        let d = FileId { dir: true, ..id(7, None, 0) };
        assert!(!d.same_file(&d), "a folder with no birth time is never matched by its number alone");
    }

    #[test]
    fn the_table_reads_back_and_ignores_what_it_does_not_know() {
        let dir = std::env::temp_dir().join(format!("quiet-fileids-{}", uuid::Uuid::new_v4()));
        let p = file(&dir, "dev1");
        let mut files = BTreeMap::new();
        files.insert("a".to_string(), Seen { path: "x/a.md".into(), id: id(5, Some(9), 2) });
        save(&p, &files).unwrap();
        assert_eq!(load(&p), files);
        crate::vfs::write(&p, br#"{"v":9,"files":{"a":{"path":"a.md","dev":1,"ino":2,"later":true}},"more":1}"#).unwrap();
        assert_eq!(load(&p)["a"].id.ino, 2);
        assert!(load(&dir.join("none.json")).is_empty());
    }
}
