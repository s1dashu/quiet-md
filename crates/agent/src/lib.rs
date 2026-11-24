//! The assistant's agent layer (docs/design/ai-assistant.md, step 1 of § 8):
//! the user's own coding agents, behind one [`Backend`] interface so the
//! panel never sees whose protocol is underneath. No GPUI, no async runtime:
//! calls block the caller, and what streams back arrives as [`Event`]s on a
//! channel.
//!
//! Backends: [`backends::codex`] (`codex app-server`) is the first; ACP
//! (Gemini CLI and the rest) and Claude Code have their modules reserved.
//! Whatever the backend, a conversation runs in a read-only sandbox with an
//! empty working folder, reaches documents only through the `quiet mcp`
//! server set up for it, and every command or file change it asks to run is
//! declined.

pub mod backends;
pub mod find;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// One agent: kept running between turns and warmed up before the first.
pub trait Backend: Send {
    /// Its name for the panel ("Codex").
    fn name(&self) -> &str;
    fn status(&self) -> &Status;
    /// The agent's version, once it has said.
    fn version(&self) -> Option<&str>;
    /// What streams back, for every turn.
    fn events(&self) -> &Receiver<Event>;
    /// Starts the agent and its conversation without sending anything, so
    /// the first message doesn't wait for a cold start. Does nothing when it
    /// is already up.
    fn prewarm(&mut self) -> Result<Warmup, Error>;
    /// Sends a message (warming up first when needed); the reply streams as
    /// [`Event`]s ending in [`Event::Completed`]. Returns the turn's id.
    fn start_turn(&mut self, text: &str) -> Result<String, Error>;
    /// Stops the running turn, if any; it ends with [`TurnEnd::Interrupted`].
    fn interrupt(&mut self) -> Result<(), Error>;
}

/// Where an agent stands, for the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status { NotInstalled, NotLoggedIn, Starting, Ready, Failed(String) }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    NotInstalled,
    NotLoggedIn,
    /// Older than the backend supports: (found, needed).
    TooOld(String, String),
    Spawn(String),
    /// The agent's process went away; the next call starts a new one.
    Exited,
    Timeout(String),
    Rpc { code: i64, message: String },
    Protocol(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotInstalled => write!(f, "the agent is not installed"),
            Error::NotLoggedIn => write!(f, "the agent is not logged in"),
            Error::TooOld(v, min) => write!(f, "version {v} is too old (needs {min} or newer)"),
            Error::Spawn(e) => write!(f, "could not start the agent: {e}"),
            Error::Exited => write!(f, "the agent exited"),
            Error::Timeout(m) => write!(f, "the agent did not answer {m}"),
            Error::Rpc { code, message } => write!(f, "{message} ({code})"),
            Error::Protocol(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// How a turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnEnd { Completed, Interrupted, Failed }

/// What streams back, the same for every backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// More of the reply.
    Text { turn: String, delta: String },
    /// More of the model's reasoning (or its summary).
    Thinking { turn: String, delta: String },
    /// A tool began: `server.tool` for MCP (`quiet.quiet`), else its kind.
    ToolStarted { turn: String, id: String, name: String },
    ToolFinished { turn: String, id: String, name: String, ok: bool },
    /// The agent asked to run a command, change a file or the like…
    ApprovalRequested { turn: Option<String>, what: String },
    /// …and was refused (always, for now: docs/design/ai-assistant.md › 4).
    Declined { turn: Option<String>, what: String },
    Completed { turn: String, end: TurnEnd, error: Option<String> },
    Error { turn: Option<String>, message: String, will_retry: bool },
    /// The agent's process ended.
    Exited,
}

/// The MCP server a conversation reaches documents through: the app's own
/// `quiet` with `mcp` and its scope.
#[derive(Clone, Debug)]
pub struct Mcp { pub command: PathBuf, pub args: Vec<String> }

/// What every backend is started with.
#[derive(Clone, Debug)]
pub struct Options {
    /// The executable set in Settings, if any (see [`find::program`]).
    pub executable: Option<PathBuf>,
    /// The conversation's working folder: an empty one in the app's data
    /// folder, never the library.
    pub cwd: PathBuf,
    pub mcp: Option<Mcp>,
    pub model: Option<String>,
    /// Extra environment for the agent's process.
    pub env: Vec<(OsString, OsString)>,
    /// How long one request may take (starting a conversation waits for
    /// its MCP servers).
    pub timeout: Duration,
}

impl Options {
    pub fn new(cwd: PathBuf) -> Options { Options { executable: None, cwd, mcp: None, model: None, env: vec![], timeout: Duration::from_secs(60) } }
}

/// How long each step of [`Backend::prewarm`] took; steps a backend doesn't
/// have stay zero.
#[derive(Clone, Copy, Debug, Default)]
pub struct Warmup { pub spawn: Duration, pub initialize: Duration, pub account: Duration, pub thread: Duration }

/// `a.b.c` from the first `name/version` token of a user agent or
/// `--version` line (`codex_cli_rs/0.171.0 (macOS …)`, `1.2.3-beta`).
pub fn version_in(s: &str) -> Option<(u32, u32, u32)> {
    let v = s.split_whitespace().find(|t| t.chars().any(|c| c.is_ascii_digit()))?.rsplit('/').next()?;
    let mut n = v.trim_start_matches('v').split(['.', '-', '+']).map(|p| p.parse::<u32>());
    Some((n.next()?.ok()?, n.next()?.ok()?, n.next().and_then(Result::ok).unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(version_in("codex_cli_rs/0.171.0 (Mac OS 26.1.0; arm64) xterm"), Some((0, 171, 0)));
        assert_eq!(version_in("quiet_md/1.2.3-alpha.4 (Windows 11; x86_64)"), Some((1, 2, 3)));
        assert_eq!(version_in("2.0.31 (Claude Code)"), Some((2, 0, 31)));
        assert_eq!(version_in("nothing"), None);
    }
}
