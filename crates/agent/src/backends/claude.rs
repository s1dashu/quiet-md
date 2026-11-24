//! Reserved: Claude Code through the user's own `claude` (docs/design/
//! ai-assistant.md › 7.5: its terms for this use are to be checked first).
//! Not written yet.
//!
//! TODO: `claude -p --input-format stream-json --output-format stream-json`
//! kept running, one JSON message per line each way. That format is not a
//! published contract, so pin the versions it was checked against (refuse
//! others with `Error::TooOld` or a not-supported error) and test against
//! recorded output. `quiet mcp` through `--mcp-config`; no tools but quiet's
//! (`--allowedTools`), so nothing else needs declining.
