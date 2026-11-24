//! Codex through `codex app-server` (JSON-RPC over stdio): what OpenAI's own
//! clients use, with per-conversation sandbox settings and no extra model
//! rounds (docs/design/ai-assistant.md › 1). One process kept up; when it
//! dies, the next call starts another (a new conversation).
//!
//! Protocol checked against `codex app-server generate-ts` of 0.171.0 (and
//! 0.167.2 for [`MIN_VERSION`]). Only `initialize`, `account/read`,
//! `thread/start`, `turn/start` and `turn/interrupt` are used; unknown fields
//! and notifications are ignored.

mod rpc;

use crate::{Backend, Error, Event, Options, Status, Warmup, find, version_in};
use rpc::{Conn, Shared};
use serde_json::{Value, json};
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The oldest Codex whose app-server has every message used here.
pub const MIN_VERSION: (u32, u32, u32) = (0, 167, 0);

pub struct Codex {
    opts: Options,
    status: Status,
    conn: Option<Conn>,
    shared: Arc<Mutex<Shared>>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    version: Option<String>,
}

impl Codex {
    pub fn new(opts: Options) -> Codex {
        let (tx, rx) = channel();
        let status = if find::program(opts.executable.as_deref(), "codex").is_some() { Status::Starting } else { Status::NotInstalled };
        Codex { opts, status, conn: None, shared: Default::default(), tx, rx, version: None }
    }

    /// The conversation's thread id, once started.
    pub fn thread(&self) -> Option<String> { self.shared.lock().unwrap().thread.clone() }

    fn start(&mut self) -> Result<Warmup, Error> {
        let mut w = Warmup::default();
        let codex = find::program(self.opts.executable.as_deref(), "codex").ok_or(Error::NotInstalled)?;
        let t = Instant::now();
        let mut cmd = Command::new(&codex);
        cmd.arg("app-server").current_dir(&self.opts.cwd).envs(self.opts.env.iter().map(|(k, v)| (k, v)));
        let conn = self.conn.insert(Conn::spawn(cmd, self.tx.clone(), self.shared.clone())?);
        w.spawn = t.elapsed();
        let t = Instant::now();
        let init = conn.request("initialize", json!({
            "clientInfo": { "name": "quiet_md", "title": "Quiet.md", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "experimentalApi": false, "requestAttestation": false },
        }), self.opts.timeout)?;
        // No version field: it is in the user agent (`<originator>/0.171.0 (…)`).
        let ua = init["userAgent"].as_str().unwrap_or_default();
        let v = version_in(ua).ok_or_else(|| Error::Protocol(format!("no version in user agent {ua:?}")))?;
        self.version = Some(format!("{}.{}.{}", v.0, v.1, v.2));
        if v < MIN_VERSION { return Err(Error::TooOld(self.version.clone().unwrap_or_default(), format!("{}.{}.{}", MIN_VERSION.0, MIN_VERSION.1, MIN_VERSION.2))) }
        conn.notify("initialized")?;
        w.initialize = t.elapsed();
        // No account and one is needed: not logged in. API keys and other
        // providers that need no OpenAI login pass.
        let t = Instant::now();
        let acc = conn.request("account/read", json!({ "refreshToken": false }), self.opts.timeout)?;
        if acc["account"].is_null() && acc["requiresOpenaiAuth"].as_bool().unwrap_or(false) { return Err(Error::NotLoggedIn) }
        w.account = t.elapsed();
        let t = Instant::now();
        let r = conn.request("thread/start", thread_params(&self.opts), self.opts.timeout)?;
        let id = r["thread"]["id"].as_str().ok_or_else(|| Error::Protocol("thread/start without a thread id".into()))?;
        self.shared.lock().unwrap().thread = Some(id.to_string());
        w.thread = t.elapsed();
        Ok(w)
    }
}

impl Backend for Codex {
    fn name(&self) -> &str { "Codex" }
    fn status(&self) -> &Status { &self.status }
    fn version(&self) -> Option<&str> { self.version.as_deref() }
    fn events(&self) -> &Receiver<Event> { &self.rx }

    fn prewarm(&mut self) -> Result<Warmup, Error> {
        if self.conn.as_ref().is_some_and(Conn::alive) && self.status == Status::Ready { return Ok(Warmup::default()) }
        self.conn = None;
        *self.shared.lock().unwrap() = Shared::default();
        self.status = Status::Starting;
        let r = self.start();
        self.status = match &r {
            Ok(_) => Status::Ready,
            Err(Error::NotInstalled) => Status::NotInstalled,
            Err(Error::NotLoggedIn) => Status::NotLoggedIn,
            Err(e) => Status::Failed(e.to_string()),
        };
        if r.is_err() { self.conn = None }
        r
    }

    fn start_turn(&mut self, text: &str) -> Result<String, Error> {
        if !self.conn.as_ref().is_some_and(Conn::alive) || self.status != Status::Ready { self.prewarm()?; }
        let (Some(conn), Some(thread)) = (&self.conn, self.thread()) else { return Err(Error::Exited) };
        let r = conn.request("turn/start", json!({ "threadId": thread, "input": [{ "type": "text", "text": text, "text_elements": [] }] }), self.opts.timeout)?;
        let turn = r["turn"]["id"].as_str().ok_or_else(|| Error::Protocol("turn/start without a turn id".into()))?.to_string();
        self.shared.lock().unwrap().turn = Some(turn.clone());
        Ok(turn)
    }

    fn interrupt(&mut self) -> Result<(), Error> {
        let (thread, turn) = { let s = self.shared.lock().unwrap(); (s.thread.clone(), s.turn.clone()) };
        let (Some(conn), Some(thread), Some(turn)) = (&self.conn, thread, turn) else { return Ok(()) };
        conn.request("turn/interrupt", json!({ "threadId": thread, "turnId": turn }), self.opts.timeout).map(|_| ())
    }
}

/// `thread/start`: read-only sandbox in the empty working folder, `quiet mcp`
/// as the conversation's MCP server (`config` takes the same dotted keys as
/// `codex -c`, turned into TOML), its tools allowed without asking.
fn thread_params(o: &Options) -> Value {
    let mut config = serde_json::Map::new();
    if let Some(m) = &o.mcp {
        config.insert("mcp_servers.quiet".into(), json!({ "command": m.command, "args": m.args, "default_tools_approval_mode": "approve" }));
    }
    let mut p = json!({ "cwd": o.cwd, "sandbox": "read-only", "approvalPolicy": "on-request", "config": config });
    if let Some(m) = &o.model { p["model"] = json!(m); }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mcp;
    use std::path::PathBuf;

    #[test]
    fn thread_start_params() {
        let mut o = Options::new(PathBuf::from("/tmp/qa"));
        o.mcp = Some(Mcp { command: PathBuf::from("/Applications/Quiet.md.app/Contents/MacOS/quiet"), args: vec!["mcp".into()] });
        let p = thread_params(&o);
        assert_eq!(p["sandbox"], "read-only");
        assert_eq!(p["cwd"], "/tmp/qa");
        assert_eq!(p["config"]["mcp_servers.quiet"]["args"], json!(["mcp"]));
        assert!(p.get("model").is_none());
    }

    #[test]
    fn min_version() { assert!(version_in("x/0.166.9").unwrap() < MIN_VERSION); }
}
