# Resolution: stale-system-reminder-gap

**Symptom**: After editing Rust files through the wait-for-lsp proxy, stale diagnostics appear in Claude Code's `<new-diagnostics>` system reminders even though the proxy's version tracking correctly filters stale `publishDiagnostics` at the LSP protocol level.

**Root cause**: The proxy forwarded every `publishDiagnostics` immediately. When rust-analyzer pushed diagnostics incrementally (e.g., v4 with errors, v5 with different errors), each intermediate state was forwarded to the client. The client cached the last received state. Even though the proxy dropped stale diagnostics (version < tracked), the intermediate forwarded states misled the client by showing outdated error information.

**Fix location**: `src/proxy.rs:173-215` — `thread_filter_stdout`

**Fix description**: Added a per-URI dedup queue in the stdout filter thread. Instead of forwarding `publishDiagnostics` messages immediately, they are queued per URI. At the end of each read cycle, only the latest queued diagnostic per URI is forwarded. Non-diagnostic messages (hover results, go-to-definition responses, etc.) continue to be forwarded immediately.

**How it works**:
1. When `filter_message` returns a `publishDiagnostics` message, extract the URI
2. Insert into `pending: HashMap<String, Value>` — overwrites any previous entry for that URI
3. At the end of the read cycle (after `parser.feed()` returns all messages), flush the queue: forward only the latest per URI
4. Non-publishDiagnostics messages bypass the queue and are forwarded immediately

This means if rust-analyzer pushes v4, v5, v6 in a single burst, only v6 reaches the client. If they arrive in separate read cycles, each burst's latest diagnostic is forwarded, but intermediate states between bursts are never seen by the client.

**Anchor criteria used**:
- Criterion 2 (system-reminder must not show stale diagnostics) — FIXED
- Criterion 3 (fresh diagnostics must eventually arrive) — PRESERVED
- Criterion 4 (empty diagnostics forwarded when file becomes clean) — PRESERVED

**Prior notes reclassified**:
No prior notes existed beyond this session's conversation. Claims from this session are documented in INVESTIGATION.md.

**Date**: 2026-05-13
