//! GitHub CLI setup for the REST remote. When `gh` is already installed it
//! owns sign-in and the token; sync itself still uses [`GitHub`] and does not
//! start a process on every commit. Without `gh`, the interface uses the
//! device-flow setup instead.

use super::{Error, GitHub, Http, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhStatus {
    /// No executable on PATH or in the usual installation folders.
    Missing,
    /// Installed, but github.com has no usable login.
    SignedOut,
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct GhRepo {
    #[serde(rename = "nameWithOwner")]
    pub name: String,
    #[serde(rename = "isPrivate")]
    pub private: bool,
    pub url: String,
}

/// Process execution behind [`Gh`], kept small so setup is tested without gh.
pub trait GhCommand {
    /// Exit code (-1 when terminated by a signal), stdout and stderr.
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<(i32, Vec<u8>, Vec<u8>)>;
}

pub struct SystemGh;

impl GhCommand for SystemGh {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<(i32, Vec<u8>, Vec<u8>)> {
        let run = |program: &str| command(program, args).output().map(|o| (o.status.code().unwrap_or(-1), o.stdout, o.stderr));
        let first = run(program);
        // Apps launched from a GUI do not necessarily inherit a shell's PATH.
        if program != "gh" || !matches!(&first, Err(e) if e.kind() == std::io::ErrorKind::NotFound) { return first }
        for path in GH_PATHS {
            if Path::new(path).is_file() { return run(path) }
        }
        first
    }
}

/// gh's environment must not make setup and the REST token mean different
/// accounts. A GUI also cannot answer a terminal prompt.
fn command(program: &str, args: &[&str]) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(target_os = "windows")]
    { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }
    c.args(args)
        .env("GH_HOST", "github.com")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env_remove("GH_REPO")
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_ENTERPRISE_TOKEN")
        .env_remove("GITHUB_ENTERPRISE_TOKEN");
    c
}

#[cfg(target_os = "macos")]
const GH_PATHS: &[&str] = &["/opt/homebrew/bin/gh", "/usr/local/bin/gh"];
#[cfg(target_os = "windows")]
const GH_PATHS: &[&str] = &["C:\\Program Files\\GitHub CLI\\gh.exe", "C:\\Program Files (x86)\\GitHub CLI\\gh.exe"];
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const GH_PATHS: &[&str] = &["/usr/local/bin/gh", "/usr/bin/gh", "/snap/bin/gh"];

pub struct Gh<C = SystemGh> {
    commands: C,
    /// `QUIET_MD_GH` can name an installation the usual lookup would miss.
    program: String,
}

impl Gh<SystemGh> {
    pub fn system() -> Self {
        let program = std::env::var("QUIET_MD_GH").unwrap_or_else(|_| "gh".into());
        Gh { commands: SystemGh, program }
    }
}

impl<C: GhCommand> Gh<C> {
    pub fn with_commands(commands: C, program: &str) -> Self { Gh { commands, program: program.into() } }

    pub fn status(&self) -> Result<GhStatus> {
        match self.run(&["--version"]) {
            Err(Error::NotFound(_)) => return Ok(GhStatus::Missing),
            Err(e) => return Err(e),
            Ok(o) if o.0 != 0 => return Ok(GhStatus::Missing),
            _ => {}
        }
        let out = self.run(&["auth", "status", "--hostname", "github.com"])?;
        if out.0 == 0 { return Ok(GhStatus::Ready) }
        let msg = if out.2.is_empty() { text(out.1) } else { text(out.2) };
        if msg.contains("not logged into") { Ok(GhStatus::SignedOut) }
        else { Err(Error::Other(format!("gh auth status: {msg}"))) }
    }

    /// The account selected in gh, not a second login owned by Quiet.md.
    pub fn login(&self) -> Result<String> {
        let out = self.ok(&["api", "user", "--hostname", "github.com", "--jq", ".login"])?;
        let login = text(out.1);
        if login.is_empty() { return Err(Error::Auth("gh returned no GitHub account".into())) }
        Ok(login)
    }

    /// gh keeps the credential in its own secure storage. It is never an
    /// argument, written to the library, or included in an error here.
    pub fn token(&self) -> Result<String> {
        let out = self.run(&["auth", "token", "--hostname", "github.com"])?;
        // Never echo either output stream here: a partial credential is still sensitive.
        if out.0 != 0 { return Err(Error::Auth("gh could not provide a token; run `gh auth login`".into())) }
        let token = text(out.1);
        if token.is_empty() || token.chars().any(char::is_whitespace) { return Err(Error::Auth("gh returned no usable token; run `gh auth login`".into())) }
        Ok(token)
    }

    pub fn repositories(&self) -> Result<Vec<GhRepo>> {
        let out = self.ok(&["repo", "list", "--limit", "100", "--json", "nameWithOwner,isPrivate,url"])?;
        serde_json::from_slice(&out.1).map_err(|e| Error::Other(format!("gh repo list: {e}")))
    }

    /// A new private repository under the signed-in account. Existing names
    /// fail rather than being reused without the person's explicit choice.
    pub fn create_private(&self, name: &str) -> Result<String> {
        if !valid_repo_name(name) { return Err(Error::Other("invalid GitHub repository name".into())) }
        let login = self.login()?;
        self.ok(&["repo", "create", name, "--private"])?;
        Ok(format!("{login}/{name}"))
    }

    /// A normal REST remote using gh's current credential.
    pub fn remote<H: Http>(&self, http: H, repo: &str) -> Result<GitHub<H>> {
        Ok(GitHub::github_com(http, repo, &self.token()?))
    }

    fn run(&self, args: &[&str]) -> Result<(i32, Vec<u8>, Vec<u8>)> {
        self.commands.run(&self.program, args).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { Error::NotFound("GitHub CLI (gh) is not installed".into()) }
            else { Error::Other(format!("gh: {e}")) }
        })
    }

    fn ok(&self, args: &[&str]) -> Result<(i32, Vec<u8>, Vec<u8>)> {
        let out = self.run(args)?;
        if out.0 == 0 { return Ok(out) }
        let msg = if out.2.is_empty() { text(out.1) } else { text(out.2) };
        if args.first() == Some(&"auth") { Err(Error::Auth(format!("gh: {msg}"))) }
        else { Err(Error::Other(format!("gh: {msg}"))) }
    }
}

fn text(bytes: Vec<u8>) -> String { String::from_utf8_lossy(&bytes).trim().to_string() }

/// No option injection and no names GitHub rejects outright. gh remains the
/// authority on the rest of its naming rules.
fn valid_repo_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 100 && !matches!(name, "." | "..") && !name.starts_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    type Reply = std::io::Result<(i32, Vec<u8>, Vec<u8>)>;

    #[derive(Default)]
    struct Fake {
        replies: RefCell<VecDeque<Reply>>,
        calls: RefCell<Vec<(String, Vec<String>)>>,
    }

    impl Fake {
        fn with(replies: impl IntoIterator<Item = Reply>) -> Self { Fake { replies: RefCell::new(replies.into_iter().collect()), ..Default::default() } }
    }

    impl GhCommand for &Fake {
        fn run(&self, program: &str, args: &[&str]) -> Reply {
            self.calls.borrow_mut().push((program.into(), args.iter().map(|s| s.to_string()).collect()));
            self.replies.borrow_mut().pop_front().expect("a reply")
        }
    }

    fn ok(stdout: &str) -> Reply { Ok((0, stdout.as_bytes().to_vec(), vec![])) }
    fn fail(code: i32, stderr: &str) -> Reply { Ok((code, vec![], stderr.as_bytes().to_vec())) }

    #[test]
    fn status_separates_missing_signed_out_and_ready() {
        let f = Fake::with([Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no gh"))]);
        assert_eq!(Gh::with_commands(&f, "gh").status(), Ok(GhStatus::Missing));
        let f = Fake::with([ok("gh version 2.0"), fail(1, "You are not logged into any GitHub hosts")]);
        assert_eq!(Gh::with_commands(&f, "gh").status(), Ok(GhStatus::SignedOut));
        let f = Fake::with([ok("gh version 2.0"), ok("")]);
        assert_eq!(Gh::with_commands(&f, "gh").status(), Ok(GhStatus::Ready));
        let f = Fake::with([ok("gh version 2.0"), fail(1, "connection refused")]);
        assert!(matches!(Gh::with_commands(&f, "gh").status(), Err(Error::Other(_))));
    }

    #[test]
    fn system_commands_use_one_noninteractive_github_com_account() {
        let c = command("gh", &["repo", "list"]);
        let env = c.get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(env.get("GH_HOST").and_then(|v| v.as_deref()), Some("github.com"));
        assert_eq!(env.get("GH_PROMPT_DISABLED").and_then(|v| v.as_deref()), Some("1"));
        for name in ["GH_REPO", "GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"] { assert!(matches!(env.get(name), Some(None)), "{name}") }
    }

    #[test]
    fn token_and_login_come_from_gh_not_arguments() {
        let f = Fake::with([ok("gho_secret\n"), ok("octocat\n")]);
        let gh = Gh::with_commands(&f, "/custom/gh");
        assert_eq!(gh.token().unwrap(), "gho_secret");
        assert_eq!(gh.login().unwrap(), "octocat");
        let calls = f.calls.borrow();
        assert_eq!(calls[0], ("/custom/gh".into(), vec!["auth".into(), "token".into(), "--hostname".into(), "github.com".into()]));
        assert!(calls.iter().all(|(_, args)| args.iter().all(|a| !a.contains("gho_secret"))));
    }

    #[test]
    fn repositories_and_a_new_private_one() {
        let f = Fake::with([
            ok(r#"[{"nameWithOwner":"octocat/notes","isPrivate":true,"url":"https://github.com/octocat/notes"}]"#),
            ok("octocat\n"),
            ok(""),
        ]);
        let gh = Gh::with_commands(&f, "gh");
        assert_eq!(gh.repositories().unwrap(), vec![GhRepo { name: "octocat/notes".into(), private: true, url: "https://github.com/octocat/notes".into() }]);
        assert_eq!(gh.create_private("quiet-notes").unwrap(), "octocat/quiet-notes");
        assert_eq!(f.calls.borrow()[2].1, ["repo", "create", "quiet-notes", "--private"]);
        assert_eq!(f.calls.borrow()[1].1, ["api", "user", "--hostname", "github.com", "--jq", ".login"]);
    }

    #[test]
    fn bad_names_never_start_a_command() {
        let f = Fake::default();
        let gh = Gh::with_commands(&f, "gh");
        for name in ["", "--public", "../notes", "a/b", "notes\n--public"] { assert!(gh.create_private(name).is_err(), "{name:?}") }
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn an_empty_or_malformed_token_is_not_accepted() {
        for token in ["", "two tokens", "token\nother"] {
            let f = Fake::with([ok(token)]);
            assert!(matches!(Gh::with_commands(&f, "gh").token(), Err(Error::Auth(_))));
        }
        let f = Fake::with([Ok((1, b"partial-secret".to_vec(), vec![]))]);
        let error = Gh::with_commands(&f, "gh").token().unwrap_err().to_string();
        assert!(!error.contains("partial-secret"), "{error}");
    }
}
