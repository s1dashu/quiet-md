//! Measures the assistant's latency against a real Codex:
//!
//! ```sh
//! cargo run -p quiet_agent --example chat -- --quiet <quiet executable> --prewarm-wait 15
//! ```
//!
//! Prints how long each prewarm step took, then reads messages from stdin and
//! prints each reply with the time from sending to its first word and to
//! completion. Ctrl-C stops a turn (twice quits). Options: `--codex PATH`,
//! `--model NAME`, `--library DIR` (passed to `quiet mcp`), `--prewarm-wait S`
//! (idle seconds between warming up and reading the first message).

use quiet_agent::backends::codex::Codex;
use quiet_agent::{Backend, Event, Mcp, Options, TurnEnd};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let cwd = std::env::temp_dir().join(format!("quiet-agent-chat-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).expect("working folder");
    let mut o = Options::new(cwd.clone());
    let (mut quiet, mut library, mut wait) = (None, None, 0u64);
    while let Some(a) = args.next() {
        let mut v = || args.next().unwrap_or_else(|| { eprintln!("{a} needs a value"); std::process::exit(2) });
        match a.as_str() {
            "--quiet" => quiet = Some(PathBuf::from(v())),
            "--library" => library = Some(v()),
            "--codex" => o.executable = Some(PathBuf::from(v())),
            "--model" => o.model = Some(v()),
            "--prewarm-wait" => wait = v().parse().unwrap_or(0),
            _ => { eprintln!("unknown option {a}"); std::process::exit(2) }
        }
    }
    if let Some(q) = quiet {
        let mut args = vec!["mcp".to_string()];
        if let Some(l) = library { args.extend(["--library".into(), l]); }
        o.mcp = Some(Mcp { command: q, args });
    }
    let mut agent = Codex::new(o);
    let t = Instant::now();
    match agent.prewarm() {
        Ok(w) => println!("prewarm {:.2}s: spawn {:.2}s, initialize {:.2}s (codex {}), account {:.2}s, thread/start {:.2}s",
            secs(t.elapsed()), secs(w.spawn), secs(w.initialize), agent.version().unwrap_or("?"), secs(w.account), secs(w.thread)),
        Err(e) => { eprintln!("{e} ({:?})", agent.status()); std::process::exit(1) }
    }
    if wait > 0 { println!("idle {wait}s…"); std::thread::sleep(Duration::from_secs(wait)); }
    print!("> ");
    let _ = std::io::stdout().flush();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { print!("> "); let _ = std::io::stdout().flush(); continue }
        let t = Instant::now();
        let turn = match agent.start_turn(&line) { Ok(t) => t, Err(e) => { eprintln!("{e}"); continue } };
        let mut first = None;
        loop {
            let Ok(e) = agent.events().recv() else { return };
            match e {
                Event::Text { turn: id, delta } if id == turn => {
                    first.get_or_insert_with(|| t.elapsed());
                    print!("{delta}");
                    let _ = std::io::stdout().flush();
                }
                Event::Thinking { .. } => {}
                Event::ToolStarted { name, .. } => println!("\n[{name}…]"),
                Event::ToolFinished { name, ok, .. } => println!("[{name} {}]", if ok { "done" } else { "failed" }),
                Event::ApprovalRequested { .. } => {}
                Event::Declined { what, .. } => println!("\n[declined: {what}]"),
                Event::Error { message, .. } => println!("\n[error: {message}]"),
                Event::Completed { turn: id, end, error } if id == turn => {
                    println!("\n— first word {}, done {:.2}s{}{}", first.map_or("-".into(), |f| format!("{:.2}s", secs(f))), secs(t.elapsed()),
                        if end == TurnEnd::Completed { String::new() } else { format!(", {end:?}") }, error.map(|e| format!(": {e}")).unwrap_or_default());
                    break;
                }
                Event::Exited => { println!("\n[codex app-server exited]"); break }
                _ => {}
            }
        }
        print!("> ");
        let _ = std::io::stdout().flush();
    }
    let _ = std::fs::remove_dir_all(&cwd);
}

fn secs(d: Duration) -> f64 { d.as_secs_f64() }
