//! Read-only Git snapshots. Never refresh the index, run hooks, or invoke external diff tools.
use anyhow::{Result, ensure};
use std::{path::{Path, PathBuf}, process::{Command, Stdio}, io::Read, time::{Duration, Instant}};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStatus { pub path: PathBuf, pub old_path: Option<PathBuf>, pub index: char, pub worktree: char }
#[derive(Clone, Debug)]
pub struct Snapshot { pub root: PathBuf, pub branch: String, pub files: Vec<FileStatus> }
const LIMIT: u64 = 8 * 1024 * 1024;
fn run(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut child = Command::new("git").arg("--no-optional-locks").args(["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"]).arg("-C").arg(root)
        .args(args).env("GIT_TERMINAL_PROMPT", "0").env("GIT_PAGER", "cat")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let stdout = child.stdout.take().unwrap(); let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || { let mut b = vec![]; stdout.take(LIMIT + 1).read_to_end(&mut b).map(|_| b) });
    let err = std::thread::spawn(move || { let mut b = vec![]; stderr.take(64 * 1024).read_to_end(&mut b).map(|_| b) });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(s) = child.try_wait()? { break Some(s); }
        if Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); break None; }
        std::thread::sleep(Duration::from_millis(15));
    };
    let bytes = out.join().map_err(|_| anyhow::anyhow!("Git reader failed"))??;
    let errors = err.join().map_err(|_| anyhow::anyhow!("Git reader failed"))??;
    ensure!(status.is_some(), "Git query timed out");
    ensure!(status.unwrap().success(), "{}", String::from_utf8_lossy(&errors).trim());
    ensure!(bytes.len() as u64 <= LIMIT, "Git result exceeds 8 MiB; status is unknown");
    Ok(bytes)
}
pub fn discover(folder: &Path) -> Result<Option<PathBuf>> {
    match run(folder, &["rev-parse", "--show-toplevel"]) {
        Ok(bytes) => {
            let mut output = String::from_utf8(bytes)?;
            ensure!(output.pop() == Some('\n'), "Invalid Git root output");
            Ok(Some(PathBuf::from(output).canonicalize()?))
        },
        Err(e) if e.to_string().contains("not a git repository") => Ok(None),
        Err(e) => Err(e),
    }
}
pub fn snapshot(folder: &Path) -> Result<Option<Snapshot>> {
    let Some(root) = discover(folder)? else { return Ok(None); };
    let branch = run(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .or_else(|_| run(&root, &["rev-parse", "--short", "HEAD"]))?;
    let files = parse(&run(&root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?)?;
    Ok(Some(Snapshot { root, branch: String::from_utf8(branch)?.trim().to_string(), files }))
}
pub fn parse(bytes: &[u8]) -> Result<Vec<FileStatus>> {
    ensure!(bytes.is_empty() || bytes.last() == Some(&0), "Incomplete Git status");
    let mut parts = bytes.split(|b| *b == 0); let mut files = vec![];
    while let Some(record) = parts.next() {
        if record.is_empty() { continue; }
        ensure!(record.len() >= 4 && record[2] == b' ', "Invalid Git status");
        let path = PathBuf::from(String::from_utf8(record[3..].to_vec())?);
        ensure!(!path.is_absolute() && !path.components().any(|c| c == std::path::Component::ParentDir), "Invalid Git path");
        let (index, worktree) = (record[0] as char, record[1] as char);
        ensure!(" MADRCU?!".contains(index) && " MADRCU?!".contains(worktree), "Invalid Git status code");
        let old_path = if matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C') {
            let old = PathBuf::from(String::from_utf8(parts.next().ok_or_else(|| anyhow::anyhow!("Incomplete rename"))?.to_vec())?);
            ensure!(!old.as_os_str().is_empty() && !old.is_absolute() && !old.components().any(|c| c == std::path::Component::ParentDir), "Invalid Git rename path");
            Some(old)
        } else { None };
        files.push(FileStatus { path, old_path, index, worktree });
    }
    Ok(files)
}
pub fn diff(root: &Path, path: &Path, staged: bool) -> Result<String> {
    let path = path.strip_prefix(root)?;
    let path = path.to_str().ok_or_else(|| anyhow::anyhow!("Non-UTF-8 Git path"))?;
    let mut args = vec!["--literal-pathspecs", "diff", "--no-ext-diff", "--no-textconv", "--no-color", "--no-renames"];
    if staged { args.push("--cached"); }
    args.extend(["--", path]);
    let output = String::from_utf8(run(root, &args)?)?;
    if !staged && output.is_empty() {
        let status = run(root, &["--literal-pathspecs", "status", "--porcelain=v1", "-z", "--untracked-files=all", "--", path])?;
        if status.starts_with(b"?? ") {
            let content = crate::workspace_files::read(&root.join(path))?;
            let mut diff = format!("--- /dev/null\n+++ b/{path}\n");
            for line in content.split_inclusive('\n') { diff.push('+'); diff.push_str(line); }
            if !content.is_empty() && !content.ends_with('\n') { diff.push_str("\n\\ No newline at end of file\n"); }
            return Ok(diff);
        }
    }
    Ok(output)
}
impl Snapshot {
    pub fn decoration(&self, path: &Path, directory: bool) -> String {
        let statuses = self.files.iter().filter(|f| {
            let full = self.root.join(&f.path);
            full == path || (directory && (full.starts_with(path) || f.old_path.as_ref().is_some_and(|p| self.root.join(p).starts_with(path))))
        }).collect::<Vec<_>>();
        if !directory { return statuses.first().map(|f| format!("{}{}", f.index, f.worktree)).unwrap_or_default(); }
        if statuses.is_empty() { String::new() } else { format!("{}", statuses.len()) }
    }
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
        assert!(diff(&root, &path, true).unwrap().contains("+two")); assert!(diff(&root, &path, false).unwrap().contains("+three"));
        let untracked = root.join("new.md"); std::fs::write(&untracked, "new file").unwrap();
        assert!(diff(&root, &untracked, false).unwrap().contains("+new file\n\\ No newline"));
        let worktree = root.join("checkout"); run(&root, &["worktree", "add", "-b", "test", worktree.to_str().unwrap()]).unwrap();
        assert!(worktree.join(".git").is_file()); assert_eq!(snapshot(&worktree).unwrap().unwrap().branch, "test");
        assert!(parse(b" M missing-null").is_err());
        let rows = parse(b"R  new name\0old\nname\0?? -literal.md\0").unwrap(); assert_eq!(rows[0].old_path.as_deref(), Some(Path::new("old\nname")));
        std::fs::remove_dir_all(root).unwrap();
    }
}
