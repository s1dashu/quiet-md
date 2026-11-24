//! The Codex backend against a fake app-server (support/fake_app_server.rs),
//! and the [`Backend`] interface driven the same way over a second, minimal
//! backend.

use quiet_agent::backends::codex::Codex;
use quiet_agent::{Backend, Error, Event, Mcp, Options, Status, TurnEnd, Warmup};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quiet-agent-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn options(name: &str, mode: &str) -> Options {
    let mut o = Options::new(tmp(name));
    o.executable = Some(PathBuf::from(env!("CARGO_BIN_EXE_quiet-agent-fake-app-server")));
    o.mcp = Some(Mcp { command: PathBuf::from("/nonexistent/quiet"), args: vec!["mcp".into()] });
    o.env = vec![("FAKE_CODEX_MODE".into(), mode.into()), ("FAKE_CODEX_LOG".into(), o.cwd.join("log").into())];
    o.timeout = Duration::from_secs(10);
    o
}

/// Events until the turn ends.
fn turn(a: &dyn Backend, id: &str) -> Vec<Event> {
    let mut out = vec![];
    loop {
        let e = a.events().recv_timeout(Duration::from_secs(10)).expect("the turn to end");
        let done = matches!(&e, Event::Completed { turn, .. } if turn == id);
        out.push(e);
        if done { return out }
    }
}

fn text(events: &[Event]) -> String { events.iter().filter_map(|e| match e { Event::Text { delta, .. } => Some(delta.as_str()), _ => None }).collect() }

/// What the panel will do with any backend: warm up, send, read the reply
/// to its end, send again, stop a turn midway.
fn conversation(a: &mut dyn Backend) {
    a.prewarm().unwrap();
    assert_eq!(a.status(), &Status::Ready);
    let id = a.start_turn("hi").unwrap();
    let ev = turn(a, &id);
    assert_eq!(text(&ev), "Hello");
    assert_eq!(ev.last(), Some(&Event::Completed { turn: id.clone(), end: TurnEnd::Completed, error: None }));
    let id2 = a.start_turn("again").unwrap();
    assert_ne!(id, id2);
    assert_eq!(text(&turn(a, &id2)), "Hello");
    let id3 = a.start_turn("wait").unwrap();
    assert_eq!(a.events().recv_timeout(Duration::from_secs(10)).unwrap(), Event::Text { turn: id3.clone(), delta: "Thinking".into() });
    a.interrupt().unwrap();
    assert_eq!(turn(a, &id3).last(), Some(&Event::Completed { turn: id3, end: TurnEnd::Interrupted, error: None }));
    // Nothing running: nothing to stop.
    a.interrupt().unwrap();
}

#[test]
fn both_backends_hold_a_conversation() {
    conversation(&mut Codex::new(options("conversation", "")));
    conversation(&mut Echo::new());
}

/// initialize → thread/start → a streamed turn: text, thinking, the tool
/// call; approvals declined; unknown notifications and other threads ignored.
#[test]
fn codex_turn() {
    let o = options("turn", "");
    let log = o.cwd.join("log");
    let mut a = Codex::new(o);
    assert_eq!(a.status(), &Status::Starting);
    a.prewarm().unwrap();
    assert_eq!((a.status(), a.version(), a.thread().as_deref()), (&Status::Ready, Some("0.171.0"), Some("th-1")));
    // Warm: nothing sent yet but the handshake and the thread.
    let sent = || std::fs::read_to_string(&log).unwrap();
    assert_eq!(sent(), "initialize\ninitialized\naccount/read\nthread/start\n");
    a.prewarm().unwrap();
    assert_eq!(sent().lines().count(), 4, "already warm");
    let id = a.start_turn("hi").unwrap();
    let ev = turn(&a, &id);
    assert_eq!(text(&ev), "Hello");
    let t = || id.clone();
    assert!(ev.contains(&Event::Thinking { turn: t(), delta: "Look it up.".into() }));
    assert!(ev.contains(&Event::ToolStarted { turn: t(), id: "c1".into(), name: "quiet.quiet".into() }));
    assert!(ev.contains(&Event::ToolFinished { turn: t(), id: "c1".into(), name: "quiet.quiet".into(), ok: true }));
    for what in ["rm -rf ~", "file change"] {
        assert!(ev.contains(&Event::ApprovalRequested { turn: Some(t()), what: what.into() }));
        assert!(ev.contains(&Event::Declined { turn: Some(t()), what: what.into() }));
    }
    assert_eq!(sent().matches("initialize\n").count(), 1);
}

/// The process dies mid-turn: the turn fails, and the next message starts
/// a new process and conversation.
#[test]
fn codex_restarts_after_a_crash() {
    let o = options("crash", "");
    let log = o.cwd.join("log");
    let mut a = Codex::new(o);
    a.prewarm().unwrap();
    assert!(matches!(a.start_turn("crash"), Err(Error::Exited)));
    assert!(a.events().iter().any(|e| e == Event::Exited));
    let id = a.start_turn("hi").unwrap();
    assert_eq!(text(&turn(&a, &id)), "Hello");
    assert_eq!(a.status(), &Status::Ready);
    assert_eq!(std::fs::read_to_string(&log).unwrap().matches("initialize\n").count(), 2);
}

#[test]
fn codex_not_installed() {
    let mut o = options("missing", "");
    o.executable = Some(o.cwd.join("no-codex-here"));
    let mut a = Codex::new(o);
    assert_eq!(a.status(), &Status::NotInstalled);
    assert_eq!(a.prewarm().err(), Some(Error::NotInstalled));
    assert_eq!(a.start_turn("hi").err(), Some(Error::NotInstalled));
    assert_eq!(a.status(), &Status::NotInstalled);
}

#[test]
fn codex_too_old() {
    let mut a = Codex::new(options("old", "old"));
    assert_eq!(a.prewarm().err(), Some(Error::TooOld("0.100.0".into(), "0.167.0".into())));
    assert!(matches!(a.status(), Status::Failed(m) if m.contains("too old")));
}

#[test]
fn codex_not_logged_in() {
    let mut a = Codex::new(options("login", "logged-out"));
    assert_eq!(a.prewarm().err(), Some(Error::NotLoggedIn));
    assert_eq!(a.status(), &Status::NotLoggedIn);
}

/// A second backend, as small as one gets: in-process, it answers "Hello"
/// in two pieces, and `wait` waits for an interrupt.
struct Echo { status: Status, tx: Sender<Event>, rx: Receiver<Event>, turns: u32, running: Option<String> }

impl Echo {
    fn new() -> Echo { let (tx, rx) = channel(); Echo { status: Status::Starting, tx, rx, turns: 0, running: None } }
}

impl Backend for Echo {
    fn name(&self) -> &str { "Echo" }
    fn status(&self) -> &Status { &self.status }
    fn version(&self) -> Option<&str> { Some("1.0.0") }
    fn events(&self) -> &Receiver<Event> { &self.rx }
    fn prewarm(&mut self) -> Result<Warmup, Error> { self.status = Status::Ready; Ok(Warmup::default()) }
    fn start_turn(&mut self, text: &str) -> Result<String, Error> {
        self.prewarm()?;
        self.turns += 1;
        let turn = format!("echo-{}", self.turns);
        if text == "wait" {
            self.tx.send(Event::Text { turn: turn.clone(), delta: "Thinking".into() }).unwrap();
            self.running = Some(turn.clone());
            return Ok(turn);
        }
        for d in ["Hel", "lo"] { self.tx.send(Event::Text { turn: turn.clone(), delta: d.into() }).unwrap(); }
        self.tx.send(Event::Completed { turn: turn.clone(), end: TurnEnd::Completed, error: None }).unwrap();
        Ok(turn)
    }
    fn interrupt(&mut self) -> Result<(), Error> {
        if let Some(turn) = self.running.take() { self.tx.send(Event::Completed { turn, end: TurnEnd::Interrupted, error: None }).unwrap(); }
        Ok(())
    }
}
