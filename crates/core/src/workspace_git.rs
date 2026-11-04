//! Read-only Git snapshots. Never refresh the index, run hooks, or invoke external diff tools.
use anyhow::{Result, ensure};
use std::{collections::HashMap, path::{Path, PathBuf}, process::{Command, Stdio}, io::Read, time::{Duration, Instant}};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStatus { pub path: PathBuf, pub old_path: Option<PathBuf>, pub index: char, pub worktree: char }
impl FileStatus {
    pub fn conflicted(&self) -> bool { self.index == 'U' || self.worktree == 'U' || (self.index == 'A' && self.worktree == 'A') || (self.index == 'D' && self.worktree == 'D') }
    pub fn untracked(&self) -> bool { self.index == '?' }
    pub fn staged(&self) -> bool { !self.conflicted() && !matches!(self.index, ' ' | '?' | '!') }
    pub fn unstaged(&self) -> bool { !self.conflicted() && !matches!(self.worktree, ' ' | '?' | '!') }
}
/// How a path shows in the tree. A directory counts the changes under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decoration { pub code: char, pub conflicted: bool, pub count: u32 }
#[derive(Clone, Debug)]
pub struct Snapshot { pub root: PathBuf, pub branch: String, pub files: Vec<FileStatus>, pub decorations: HashMap<PathBuf, Decoration> }
#[derive(Clone, Debug)]
pub struct Repository {
    pub root: PathBuf, pub branch: String, pub detached: bool,
    /// A linked worktree (its `.git` is a file) rather than the main checkout.
    pub worktree: bool,
    /// Where `HEAD` and `index` live, which may be outside the opened folder.
    pub git_dir: PathBuf,
}
#[derive(Clone, Debug, Default)]
pub struct Probe { pub repository: Option<Repository>, pub snapshot: Option<Snapshot>, pub error: Option<String> }
const LIMIT: u64 = 16 * 1024 * 1024;
// A status on a large LFS repository can legitimately take a while.
const TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(test)]
fn run(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    run_with_path(root, args, std::env::var_os("PATH"))
}
/// What a Finder or Dock launch lacks: its PATH is only `/usr/bin:/bin:...`, so
/// `git-lfs` and other filter helpers would not be found. Where the user keeps
/// their tools comes first, so `git` and its helpers are a matched pair. This
/// only affects our own child processes.
fn tool_path(path: Option<std::ffi::OsString>) -> Result<std::ffi::OsString> {
    let inherited = path.as_deref().map(std::env::split_paths).map(Iterator::collect::<Vec<_>>).unwrap_or_default();
    let mut paths: Vec<PathBuf> = vec![];
    #[cfg(unix)] {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let mut extra: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "/run/current-system/sw/bin"].iter().map(PathBuf::from).collect();
        extra.extend([".local/bin", ".nix-profile/bin", ".local/share/mise/shims", ".asdf/shims", ".cargo/bin"].iter().map(|d| home.join(d)));
        paths.extend(extra.into_iter().filter(|d| d.is_dir()));
    }
    for dir in inherited { if !paths.contains(&dir) { paths.push(dir); } }
    Ok(std::env::join_paths(paths)?)
}
fn run_with_path(root: &Path, args: &[&str], path: Option<std::ffi::OsString>) -> Result<Vec<u8>> {
    let mut child = Command::new("git").arg("--no-optional-locks").args(["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"]).arg("-C").arg(root)
        .args(args).env("PATH", tool_path(path)?).env("GIT_TERMINAL_PROMPT", "0").env("GIT_PAGER", "cat").env("LC_ALL", "C").env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| anyhow::anyhow!("Could not run git: {e}"))?;
    let stdout = child.stdout.take().unwrap(); let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || { let mut b = vec![]; stdout.take(LIMIT + 1).read_to_end(&mut b).map(|_| b) });
    let err = std::thread::spawn(move || { let mut b = vec![]; stderr.take(64 * 1024).read_to_end(&mut b).map(|_| b) });
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(s) = child.try_wait()? { break Some(s); }
        if Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); break None; }
        std::thread::sleep(Duration::from_millis(15));
    };
    let bytes = out.join().map_err(|_| anyhow::anyhow!("Git reader failed"))??;
    let errors = err.join().map_err(|_| anyhow::anyhow!("Git reader failed"))??;
    ensure!(status.is_some(), "Git query timed out");
    ensure!(status.unwrap().success(), "{}", String::from_utf8_lossy(&errors).trim());
    ensure!(bytes.len() as u64 <= LIMIT, "Git result exceeds 16 MiB; status is unknown");
    Ok(bytes)
}
pub fn discover(folder: &Path) -> Result<Option<PathBuf>> { Ok(repository(folder, std::env::var_os("PATH"))?.map(|r| r.root)) }
fn repository(folder: &Path, path: Option<std::ffi::OsString>) -> Result<Option<Repository>> {
    let output = match run_with_path(folder, &["rev-parse", "--show-toplevel", "--absolute-git-dir", "--git-common-dir"], path.clone()) {
        Ok(bytes) => String::from_utf8(bytes)?,
        Err(e) if e.to_string().contains("not a git repository") => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut lines = output.lines();
    let (Some(root), Some(git_dir), Some(common)) = (lines.next(), lines.next(), lines.next()) else { anyhow::bail!("Invalid Git root output") };
    let root = PathBuf::from(root).canonicalize()?;
    let git_dir = PathBuf::from(git_dir);
    let common = if Path::new(common).is_absolute() { PathBuf::from(common) } else { folder.join(common) };
    let worktree = common.canonicalize().ok() != git_dir.canonicalize().ok();
    let (branch, detached) = match run_with_path(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"], path.clone()) {
        Ok(name) => (String::from_utf8(name)?.trim().to_string(), false),
        Err(_) => (String::from_utf8(run_with_path(&root, &["rev-parse", "--short", "HEAD"], path)?)?.trim().to_string(), true),
    };
    Ok(Some(Repository { root, branch, detached, worktree, git_dir }))
}
pub fn snapshot(folder: &Path) -> Result<Option<Snapshot>> {
    let probe = probe(folder);
    if let Some(error) = probe.error { anyhow::bail!("{error}"); }
    Ok(probe.snapshot)
}
pub fn probe(folder: &Path) -> Probe { probe_with_path(folder, std::env::var_os("PATH")) }
fn probe_with_path(folder: &Path, path: Option<std::ffi::OsString>) -> Probe {
    let repository = match repository(folder, path.clone()) { Ok(r) => r, Err(e) => return Probe { error: Some(e.to_string()), ..Probe::default() } };
    let Some(repo) = repository.as_ref() else { return Probe::default(); };
    // Only what is inside the opened folder; paths are still repository-relative.
    let result = run_with_path(folder, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."], path).and_then(|bytes| parse(&bytes));
    match result {
        Ok(files) => { let decorations = decorations(&repo.root, folder, &files); Probe { snapshot: Some(Snapshot { root: repo.root.clone(), branch: repo.branch.clone(), files, decorations }), repository, error: None } },
        Err(e) => Probe { repository, snapshot: None, error: Some(e.to_string()) },
    }
}
fn decorations(root: &Path, folder: &Path, files: &[FileStatus]) -> HashMap<PathBuf, Decoration> {
    let folder = folder.canonicalize().unwrap_or_else(|_| folder.to_path_buf());
    let mut map: HashMap<PathBuf, Decoration> = HashMap::with_capacity(files.len());
    for f in files {
        let conflicted = f.conflicted();
        let code = if conflicted { 'U' } else if f.worktree != ' ' { f.worktree } else { f.index };
        let full = root.join(&f.path);
        let mut dir = full.parent();
        map.insert(full.clone(), Decoration { code, conflicted, count: 1 });
        while let Some(d) = dir.filter(|d| d.starts_with(&folder)) {
            let entry = map.entry(d.to_path_buf()).or_default();
            entry.count += 1; entry.conflicted |= conflicted;
            dir = d.parent();
        }
    }
    map
}
fn git_path(bytes: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    let path = { use std::os::unix::ffi::OsStrExt; PathBuf::from(std::ffi::OsStr::from_bytes(bytes)) };
    #[cfg(not(unix))]
    let path = PathBuf::from(String::from_utf8(bytes.to_vec())?);
    ensure!(!path.as_os_str().is_empty() && !path.is_absolute() && !path.components().any(|c| c == std::path::Component::ParentDir), "Invalid Git path");
    Ok(path)
}
/// Porcelain -z. Anything truncated or unreadable is an error, never a clean row.
pub fn parse(bytes: &[u8]) -> Result<Vec<FileStatus>> {
    ensure!(bytes.is_empty() || bytes.last() == Some(&0), "Incomplete Git status");
    let mut parts = bytes.split(|b| *b == 0); let mut files: Vec<FileStatus> = vec![];
    while let Some(record) = parts.next() {
        if record.is_empty() { continue; }
        ensure!(record.len() >= 4 && record[2] == b' ', "Invalid Git status");
        let path = git_path(&record[3..])?;
        let (index, worktree) = (record[0] as char, record[1] as char);
        ensure!(" MTADRCU?!".contains(index) && " MTADRCU?!".contains(worktree), "Invalid Git status code");
        let old_path = if matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C') { Some(git_path(parts.next().ok_or_else(|| anyhow::anyhow!("Incomplete rename"))?)?) } else { None };
        files.push(FileStatus { path, old_path, index, worktree });
    }
    // A staged deletion plus an untracked replacement are two records for one path.
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut merged: Vec<FileStatus> = Vec::with_capacity(files.len());
    for file in files {
        match merged.last_mut().filter(|p| p.path == file.path) {
            Some(prev) => {
                if !matches!(file.index, ' ' | '?') { prev.index = file.index; }
                if file.worktree != ' ' && file.index != '?' { prev.worktree = file.worktree; } else if file.index == '?' { prev.worktree = '?'; }
                prev.old_path = prev.old_path.take().or(file.old_path);
            }
            None => merged.push(file),
        }
    }
    Ok(merged)
}
pub fn diff(root: &Path, file: &FileStatus, staged: bool) -> Result<String> { diff_with_path(root, file, staged, std::env::var_os("PATH")) }
fn diff_with_path(root: &Path, file: &FileStatus, staged: bool, command_path: Option<std::ffi::OsString>) -> Result<String> {
    let text = |p: &Path| p.to_str().map(str::to_string).ok_or_else(|| anyhow::anyhow!("Non-UTF-8 Git path"));
    let path = text(&file.path)?;
    if file.untracked() {
        let content = crate::workspace_files::read(&root.join(&file.path))?;
        let mut diff = format!("--- /dev/null\n+++ b/{path}\n");
        for line in content.split_inclusive('\n') { diff.push('+'); diff.push_str(line); }
        if !content.is_empty() && !content.ends_with('\n') { diff.push_str("\n\\ No newline at end of file\n"); }
        return Ok(diff);
    }
    let mut args = vec!["--literal-pathspecs".to_string(), "diff".into(), "--no-ext-diff".into(), "--no-textconv".into(), "--no-color".into(), "-M".into()];
    if staged { args.push("--cached".into()); }
    args.push("--".into()); args.push(path);
    if let Some(old) = &file.old_path { args.push(text(old)?); }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(String::from_utf8_lossy(&run_with_path(root, &args, command_path)?).into_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_git_status_diff_worktree() {
        let root = std::env::temp_dir().join(format!("quiet-git-{}", uuid::Uuid::new_v4())); std::fs::create_dir(&root).unwrap();
        assert!(discover(&root).unwrap().is_none());
        run(&root, &["init", "-b", "main"]).unwrap(); run(&root, &["config", "user.email", "test@example.invalid"]).unwrap(); run(&root, &["config", "user.name", "Test"]).unwrap();
        let path = root.join("a.md"); std::fs::write(&path, "one\n").unwrap(); run(&root, &["add", "a.md"]).unwrap(); run(&root, &["commit", "-m", "fixture"]).unwrap();
        std::fs::write(&path, "two\n").unwrap(); run(&root, &["add", "a.md"]).unwrap(); std::fs::write(&path, "three\n").unwrap();
        let status = snapshot(&root).unwrap().unwrap(); assert_eq!((status.files[0].index, status.files[0].worktree), ('M', 'M'));
        assert!(status.files[0].staged() && status.files[0].unstaged());
        let row = status.files[0].clone();
        assert!(diff(&root, &row, true).unwrap().contains("+two")); assert!(diff(&root, &row, false).unwrap().contains("+three"));
        std::fs::write(root.join("new.md"), "new file").unwrap();
        let status = snapshot(&root).unwrap().unwrap(); let row = status.files.iter().find(|f| f.untracked()).unwrap();
        assert!(diff(&root, row, false).unwrap().contains("+new file\n\\ No newline"));
        std::fs::create_dir(root.join("sub")).unwrap(); std::fs::write(root.join("sub/in.md"), "x").unwrap();
        // Only what is inside the opened folder, and rolled up for its directories.
        let scoped = snapshot(&root.join("sub")).unwrap().unwrap();
        assert_eq!(scoped.files.len(), 1); assert_eq!(scoped.files[0].path, Path::new("sub/in.md"));
        let full = snapshot(&root).unwrap().unwrap(); let canon = root.canonicalize().unwrap();
        assert_eq!(full.decorations[&canon.join("sub")].count, 1); assert_eq!(full.decorations[&canon.join("sub/in.md")].code, '?');
        std::fs::remove_dir_all(root.join("sub")).unwrap();
        let worktree = root.join("checkout"); run(&root, &["worktree", "add", "-b", "test", worktree.to_str().unwrap()]).unwrap();
        assert!(worktree.join(".git").is_file()); assert_eq!(snapshot(&worktree).unwrap().unwrap().branch, "test");
        let linked = probe(&worktree).repository.unwrap(); assert!(linked.worktree && !linked.detached);
        assert!(!probe(&root).repository.unwrap().worktree);
        run(&root, &["stash", "--include-untracked"]).unwrap();
        run(&root, &["checkout", "--detach"]).unwrap();
        let detached = probe(&root).repository.unwrap(); assert!(detached.detached && !detached.branch.is_empty());
        let (conflict, deleted) = (FileStatus { path: "a".into(), old_path: None, index: 'U', worktree: 'U' }, FileStatus { path: "a".into(), old_path: None, index: ' ', worktree: 'D' });
        assert!(conflict.conflicted() && !conflict.staged() && deleted.unstaged() && !deleted.staged());
        let both = parse(b"D  x\0?? x\0").unwrap(); assert_eq!((both.len(), both[0].index, both[0].worktree), (1, 'D', '?'));
        assert!(parse(b" M missing-null").is_err());
        let rows = parse(b"R  new name\0old\nname\0?? -literal.md\0").unwrap(); assert_eq!(rows[1].old_path.as_deref(), Some(Path::new("old\nname")));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn workspace_git_finder_path_lfs_and_failure_state() {
        if !["/opt/homebrew/bin/git-lfs", "/usr/local/bin/git-lfs"].iter().any(|p| Path::new(p).is_file()) { return; }
        let root = std::env::temp_dir().join(format!("quiet-lfs-{}", uuid::Uuid::new_v4())); std::fs::create_dir(&root).unwrap(); let root = root.canonicalize().unwrap();
        run(&root, &["init", "-b", "main"]).unwrap(); run(&root, &["config", "user.email", "test@example.invalid"]).unwrap(); run(&root, &["config", "user.name", "Test"]).unwrap();
        for (key, value) in [("filter.lfs.clean", "git-lfs clean -- %f"), ("filter.lfs.smudge", "git-lfs smudge -- %f"), ("filter.lfs.process", "git-lfs filter-process"), ("filter.lfs.required", "true")] { run(&root, &["config", key, value]).unwrap(); }
        std::fs::write(root.join(".gitattributes"), "*.md filter=lfs diff=lfs merge=lfs -text\n").unwrap();
        let file = root.join("lfs.md"); std::fs::write(&file, "original content\n").unwrap();
        run(&root, &["add", ".gitattributes", "lfs.md"]).unwrap(); run(&root, &["commit", "-m", "fixture"]).unwrap();
        std::fs::write(&file, "staged content\n").unwrap(); run(&root, &["add", "lfs.md"]).unwrap();
        std::fs::write(&file, "working content\n").unwrap();
        let index = std::fs::read(root.join(".git/index")).unwrap(); let config = std::fs::read(root.join(".git/config")).unwrap();
        let finder = Some(std::ffi::OsString::from("/usr/bin:/bin:/usr/sbin:/sbin"));
        let state = probe_with_path(&root, finder.clone());
        assert!(state.error.is_none(), "{:?}", state.error);
        let snapshot = state.snapshot.unwrap(); let row = snapshot.files.iter().find(|f| f.path == Path::new("lfs.md")).unwrap();
        assert_eq!((row.index, row.worktree), ('M', 'M'));
        assert!(!diff_with_path(&root, row, true, finder.clone()).unwrap().is_empty());
        assert!(!diff_with_path(&root, row, false, finder.clone()).unwrap().is_empty());
        assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index); assert_eq!(std::fs::read(root.join(".git/config")).unwrap(), config);
        run(&root, &["config", "filter.lfs.process", "quiet-md-missing-lfs-filter"]).unwrap();
        std::fs::write(&file, "broken content\n").unwrap();
        let failed = probe_with_path(&root, finder);
        assert!(failed.error.is_some()); assert!(failed.snapshot.is_none());
        let repository = failed.repository.unwrap(); assert_eq!(repository.root, root.canonicalize().unwrap()); assert_eq!(repository.branch, "main"); assert!(!repository.detached);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn workspace_git_live_probe() {
        let folder = PathBuf::from(std::env::var_os("QUIET_MD_GIT_PROBE").expect("Set QUIET_MD_GIT_PROBE to the folder to inspect"));
        let state = probe_with_path(&folder, Some("/usr/bin:/bin:/usr/sbin:/sbin".into()));
        assert!(state.error.is_none(), "Git status failed: {:?}", state.error);
        let snapshot = state.snapshot.expect("Folder is not in a Git repository");
        println!("Git probe succeeded: {} status entries", snapshot.files.len());
    }
}
