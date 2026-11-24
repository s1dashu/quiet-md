//! A stand-in for `codex app-server` in the tests: answers the messages the
//! client uses, in the shapes `codex app-server generate-ts` (0.171.0)
//! describes, and plays a scripted turn for what the user "says":
//! - anything: thinking, an MCP tool call, an unknown notification, a
//!   command and a file change asking for approval (the client must
//!   decline both), then the reply "Hello" and completion;
//! - `wait`: one word, then nothing until `turn/interrupt`;
//! - `crash`: exits at once.
//!
//! `FAKE_CODEX_MODE`: `old` (Codex 0.100.0), `logged-out`. `FAKE_CODEX_LOG`:
//! a file each received method is appended to.

use serde_json::{Value, json};
use std::io::{BufRead, Write};

fn main() {
    if std::env::args().nth(1).as_deref() != Some("app-server") { std::process::exit(2) }
    let mode = std::env::var("FAKE_CODEX_MODE").unwrap_or_default();
    let log = std::env::var_os("FAKE_CODEX_LOG");
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut turns = 0;
    while let Some(Ok(line)) = lines.next() {
        let msg: Value = serde_json::from_str(&line).expect("one JSON message per line");
        assert!(msg.get("jsonrpc").is_none(), "Codex's framing has no jsonrpc field");
        let (id, method, p) = (msg["id"].clone(), msg["method"].as_str().unwrap_or_default().to_string(), &msg["params"]);
        if let Some(l) = &log { let mut f = std::fs::OpenOptions::new().create(true).append(true).open(l).unwrap(); writeln!(f, "{method}").unwrap(); }
        match method.as_str() {
            "initialize" => {
                let v = if mode == "old" { "0.100.0" } else { "0.171.0" };
                reply(&id, json!({ "userAgent": format!("quiet_md/{v} (Mac OS 26.1.0; arm64) unknown"), "codexHome": "/tmp/codex", "platformFamily": "unix", "platformOs": "macos", "someFutureField": 1 }));
            }
            "initialized" => send(json!({ "method": "account/updated", "params": { "authMode": null, "reason": "workspaceRoutingReady" } })),
            "account/read" => reply(&id, if mode == "logged-out" { json!({ "account": null, "userId": null, "requiresOpenaiAuth": true }) }
                else { json!({ "account": { "type": "chatgpt", "email": "a@b.c", "planType": "plus", "workspaceName": null }, "userId": "u", "requiresOpenaiAuth": true }) }),
            "thread/start" => {
                // What the client must ask for.
                let mcp = &p["config"]["mcp_servers.quiet"];
                if p["sandbox"] != "read-only" || !p["cwd"].is_string() || mcp["args"][0] != "mcp" || mcp["default_tools_approval_mode"] != "approve" {
                    send(json!({ "id": id, "error": { "code": -32602, "message": format!("unexpected thread/start {p}") } }));
                    continue;
                }
                reply(&id, json!({ "thread": { "id": "th-1", "preview": "", "ephemeral": false }, "model": "terra", "modelProvider": "openai", "serviceTier": null, "cwd": p["cwd"], "approvalPolicy": "on-request", "sandbox": { "type": "readOnly" }, "reasoningEffort": null }));
                send(json!({ "method": "thread/started", "params": { "thread": { "id": "th-1" } } }));
                send(json!({ "method": "mcpServer/startupStatus/updated", "params": { "name": "quiet", "status": "ready" } }));
            }
            "turn/start" => {
                turns += 1;
                let turn = format!("tu-{turns}");
                let text = p["input"][0]["text"].as_str().unwrap_or_default().to_string();
                if text == "crash" { std::process::exit(1) }
                reply(&id, json!({ "turn": { "id": turn, "items": [], "status": "inProgress", "error": null } }));
                send(note("turn/started", json!({ "threadId": "th-1", "turn": { "id": turn, "status": "inProgress" } })));
                if text == "wait" {
                    send(note("item/agentMessage/delta", json!({ "threadId": "th-1", "turnId": turn, "itemId": "m1", "delta": "Thinking" })));
                    continue;
                }
                send(note("item/reasoning/summaryTextDelta", json!({ "threadId": "th-1", "turnId": turn, "itemId": "r1", "delta": "Look it up.", "summaryIndex": 0 })));
                send(note("item/started", json!({ "threadId": "th-1", "turnId": turn, "startedAtMs": 1, "item": { "type": "mcpToolCall", "id": "c1", "server": "quiet", "tool": "quiet", "status": "inProgress", "arguments": { "args": ["ls"] } } })));
                send(note("made/up", json!({ "anything": [1, 2, 3] })));
                send(note("item/completed", json!({ "threadId": "th-1", "turnId": turn, "completedAtMs": 2, "item": { "type": "mcpToolCall", "id": "c1", "server": "quiet", "tool": "quiet", "status": "completed" } })));
                // Approval requests: the client must decline both before this goes on.
                for (rid, m, params) in [
                    ("srv-1", "item/commandExecution/requestApproval", json!({ "threadId": "th-1", "turnId": turn, "itemId": "x1", "command": "rm -rf ~" })),
                    ("srv-2", "item/fileChange/requestApproval", json!({ "threadId": "th-1", "turnId": turn, "itemId": "x2" })),
                ] {
                    send(json!({ "id": rid, "method": m, "params": params }));
                    let answer: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
                    assert_eq!(answer["id"], rid);
                    let ok = answer["result"]["decision"] == "decline";
                    send(note("item/agentMessage/delta", json!({ "threadId": "th-1", "turnId": turn, "itemId": "m1", "delta": if ok { "" } else { "NOT-DECLINED " } })));
                }
                for d in ["Hel", "lo"] { send(note("item/agentMessage/delta", json!({ "threadId": "th-1", "turnId": turn, "itemId": "m1", "delta": d }))); }
                send(note("item/agentMessage/delta", json!({ "threadId": "th-other", "turnId": "x", "itemId": "m9", "delta": "someone else's" })));
                send(note("turn/completed", json!({ "threadId": "th-1", "turn": { "id": turn, "status": "completed", "error": null, "items": [] } })));
            }
            "turn/interrupt" => {
                reply(&id, json!({}));
                send(note("turn/completed", json!({ "threadId": "th-1", "turn": { "id": p["turnId"], "status": "interrupted", "error": null, "items": [] } })));
            }
            _ if !id.is_null() => send(json!({ "id": id, "error": { "code": -32601, "message": "unknown method" } })),
            _ => {}
        }
    }
}

fn note(method: &str, params: Value) -> Value { json!({ "method": method, "params": params }) }
fn reply(id: &Value, result: Value) { send(json!({ "id": id, "result": result })) }
fn send(msg: Value) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{msg}").unwrap();
    out.flush().unwrap();
}
