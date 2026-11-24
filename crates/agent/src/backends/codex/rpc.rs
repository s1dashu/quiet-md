//! One `codex app-server` process: JSON-RPC over its stdin/stdout, one
//! message per line, no `jsonrpc` field (Codex's own framing). A thread
//! reads its output: answers go to whoever asked, notifications become
//! [`Event`]s, and its requests for approval are all declined.

use crate::{Error, Event, TurnEnd};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Value, Error>>>>>;

/// What the reader shares with the [`super::Codex`]: its thread, and the turn
/// running in it (cleared when the turn ends).
#[derive(Default)]
pub(crate) struct Shared { pub thread: Option<String>, pub turn: Option<String> }

pub(crate) struct Conn {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    next: AtomicU64,
    pending: Pending,
    alive: Arc<AtomicBool>,
}

impl Conn {
    pub fn spawn(mut cmd: Command, events: Sender<Event>, shared: Arc<Mutex<Shared>>) -> Result<Conn, Error> {
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| Error::Spawn(e.to_string()))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or_else(|| Error::Spawn("no stdin".into()))?));
        let stdout = child.stdout.take().ok_or_else(|| Error::Spawn("no stdout".into()))?;
        let (pending, alive): (Pending, _) = (Default::default(), Arc::new(AtomicBool::new(true)));
        let r = Reader { stdin: stdin.clone(), pending: pending.clone(), events, shared };
        let a = alive.clone();
        std::thread::Builder::new().name("codex-app-server".into()).spawn(move || {
            let mut lines = BufReader::new(stdout);
            let mut line = String::new();
            while matches!(lines.read_line(&mut line), Ok(n) if n > 0) {
                if let Ok(msg) = serde_json::from_str::<Value>(line.trim()) { r.handle(msg); }
                line.clear();
            }
            a.store(false, Ordering::SeqCst);
            for (_, tx) in r.pending.lock().unwrap().drain() { let _ = tx.send(Err(Error::Exited)); }
            if let Some(turn) = r.shared.lock().unwrap().turn.take() { let _ = r.events.send(Event::Completed { turn, end: TurnEnd::Failed, error: Some("codex app-server exited".into()) }); }
            let _ = r.events.send(Event::Exited);
        }).map_err(|e| Error::Spawn(e.to_string()))?;
        Ok(Conn { child, stdin, next: AtomicU64::new(1), pending, alive })
    }

    pub fn alive(&self) -> bool { self.alive.load(Ordering::SeqCst) }

    /// Sends `method` and waits for its answer.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, Error> {
        let rx = self.send(method, params)?;
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(Error::Timeout(method.into())),
            Err(_) => Err(Error::Exited),
        }
    }

    /// Sends `method`; the answer arrives on the receiver.
    pub fn send(&self, method: &str, params: Value) -> Result<Receiver<Result<Value, Error>>, Error> {
        if !self.alive() { return Err(Error::Exited) }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        if let Err(e) = write(&self.stdin, &json!({ "id": id, "method": method, "params": params })) { self.pending.lock().unwrap().remove(&id); return Err(e) }
        Ok(rx)
    }

    pub fn notify(&self, method: &str) -> Result<(), Error> { write(&self.stdin, &json!({ "method": method })) }
}

impl Drop for Conn {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}

fn write(stdin: &Mutex<ChildStdin>, msg: &Value) -> Result<(), Error> {
    let mut s = stdin.lock().unwrap();
    writeln!(s, "{msg}").and_then(|_| s.flush()).map_err(|_| Error::Exited)
}

struct Reader { stdin: Arc<Mutex<ChildStdin>>, pending: Pending, events: Sender<Event>, shared: Arc<Mutex<Shared>> }

impl Reader {
    fn handle(&self, msg: Value) {
        let method = msg.get("method").and_then(Value::as_str);
        match (method, msg.get("id")) {
            (None, Some(id)) => {
                let Some(tx) = id.as_u64().and_then(|id| self.pending.lock().unwrap().remove(&id)) else { return };
                let r = match msg.get("error") {
                    Some(e) => Err(Error::Rpc { code: e["code"].as_i64().unwrap_or(0), message: e["message"].as_str().unwrap_or_default().into() }),
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(r);
            }
            (Some(m), Some(id)) => self.request(m, id.clone(), msg.get("params").unwrap_or(&Value::Null)),
            (Some(m), None) => self.notification(m, msg.get("params").unwrap_or(&Value::Null)),
            (None, None) => {}
        }
    }

    /// The server's requests. Commands, file changes and anything else that
    /// asks for a person are declined: the assistant changes documents only
    /// through `quiet` (docs/design/ai-assistant.md › 4).
    fn request(&self, method: &str, id: Value, p: &Value) {
        let what = |k: &str| p.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
        let (reply, declined) = match method {
            "item/commandExecution/requestApproval" => (json!({ "decision": "decline" }), Some(what("command").unwrap_or("command").to_string())),
            "item/fileChange/requestApproval" => (json!({ "decision": "decline" }), Some("file change".into())),
            "execCommandApproval" | "applyPatchApproval" => (json!({ "decision": { "denied": { "rejection": "Quiet.md changes documents only through quiet" } } }), Some(if method == "execCommandApproval" { "command" } else { "file change" }.into())),
            "mcpServer/elicitation/request" => (json!({ "action": "decline", "content": null }), Some(format!("{} request", what("serverName").unwrap_or("tool")))),
            "item/permissions/requestApproval" => (Value::Null, Some("permissions".into())),
            _ => (Value::Null, None),
        };
        let msg = if reply.is_null() { json!({ "id": id, "error": { "code": -32601, "message": "not supported by Quiet.md" } }) } else { json!({ "id": id, "result": reply }) };
        let _ = write(&self.stdin, &msg);
        if let Some(what) = declined {
            let _ = self.events.send(Event::ApprovalRequested { turn: what_turn(p), what: what.clone() });
            let _ = self.events.send(Event::Declined { turn: what_turn(p), what });
        }
    }

    /// Notifications this client uses; the rest are ignored.
    fn notification(&self, method: &str, p: &Value) {
        let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        // Another thread's, should the server ever send one.
        if p.get("threadId").and_then(Value::as_str).is_some_and(|t| self.shared.lock().unwrap().thread.as_deref().is_some_and(|ours| ours != t)) { return }
        let ev = match method {
            "item/agentMessage/delta" => Event::Text { turn: s("turnId"), delta: s("delta") },
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => Event::Thinking { turn: s("turnId"), delta: s("delta") },
            "item/started" | "item/completed" => {
                let item = &p["item"];
                let Some(name) = tool_name(item) else { return };
                let id = item["id"].as_str().unwrap_or_default().to_string();
                if method == "item/started" { Event::ToolStarted { turn: s("turnId"), id, name } }
                else { Event::ToolFinished { turn: s("turnId"), id, name, ok: item["status"].as_str() == Some("completed") } }
            }
            "turn/completed" => {
                let turn = &p["turn"];
                let id = turn["id"].as_str().unwrap_or_default().to_string();
                { let mut sh = self.shared.lock().unwrap(); if sh.turn.as_deref() == Some(id.as_str()) { sh.turn = None } }
                let end = match turn["status"].as_str() { Some("interrupted") => TurnEnd::Interrupted, Some("failed") => TurnEnd::Failed, _ => TurnEnd::Completed };
                Event::Completed { turn: id, end, error: turn["error"]["message"].as_str().map(str::to_string) }
            }
            "error" => Event::Error { turn: Some(s("turnId")).filter(|t| !t.is_empty()), message: p["error"]["message"].as_str().unwrap_or("error").into(), will_retry: p["willRetry"].as_bool().unwrap_or(false) },
            _ => return,
        };
        let _ = self.events.send(ev);
    }
}

fn what_turn(p: &Value) -> Option<String> { p.get("turnId").and_then(Value::as_str).map(str::to_string) }

/// A tool item's name for the panel: `server.tool` for MCP, else its kind.
fn tool_name(item: &Value) -> Option<String> {
    match item["type"].as_str()? {
        "mcpToolCall" => Some(format!("{}.{}", item["server"].as_str().unwrap_or_default(), item["tool"].as_str().unwrap_or_default())),
        k @ ("commandExecution" | "fileChange" | "webSearch" | "dynamicToolCall") => Some(k.into()),
        _ => None,
    }
}
