//! Reserved: the general backend, for agents that speak the Agent Client
//! Protocol natively (Gemini CLI and the long tail). Not written yet.
//!
//! TODO: once ACP v2 is stable (docs/design/ai-assistant.md › 1): start the
//! agent's own executable (found with `find::program`), JSON-RPC 2.0 over
//! stdio (`initialize`, `session/new` with `quiet mcp` among its
//! `mcpServers`, `session/prompt` streaming `session/update`,
//! `session/cancel`), declining `session/request_permission`. Hand-written
//! like the Codex client, without the `agent-client-protocol` crate unless
//! its size is acceptable then.
