---
name: wait-for-lsp-proxy design decisions
description: Design decisions, fixture declarations, and success criteria for the Rust rewrite of the LSP proxy
type: project
---

# DECISIONS: wait-for-lsp-proxy

## Goals

- **Primary**: Implement the proxy binary that sits between Claude Code and rust-analyzer, filtering stale `textDocument/publishDiagnostics` notifications.
- **Scope**: Proxy core + Rust/rust-analyzer plugin only. No `setup` subcommand. Other languages deferred.
- **Distribution**: Binary compiled via `cargo build`. Plugin JSON installed manually via `claude plugin install`.

## Fixture Files

### Fixture 1: LSP Workspace

`tests/fixtures/lsp-workspace/` — A minimal Cargo workspace with intentional type errors:

- `Cargo.toml`: Minimal package definition (edition 2021)
- `src/main.rs`: Contains two type errors (`let x: i32 = "not a number"` and `greet(42)`)

**Purpose**: Provides a real Rust file that rust-analyzer can analyze and produce diagnostics for.

### Fixture 2: Captured LSP Session (binary)

`tests/fixtures/rust-analyzer-session.bin` — 9831 bytes of Content-Length framed LSP wire traffic, captured from a live rust-analyzer session against the workspace fixture.

Contains 7 framed messages:
| # | Method | Content-Length | Key Data |
|---|--------|---------------|----------|
| 1 | `initialize` response | 2790 | Server capabilities |
| 2 | `textDocument/publishDiagnostics` | 193 | 0 diagnostics (initial clear) |
| 3 | `textDocument/publishDiagnostics` | 706 | 2 diagnostics, both severity 1 |
| 4 | `textDocument/publishDiagnostics` | 1925 | 4 diagnostics (3 sev 1, 1 sev 4 with relatedInformation) |
| 5 | `textDocument/publishDiagnostics` | 3953 | 7 diagnostics (4 sev 1, 3 sev 4) |
| 6 | `workspace/diagnostic/refresh` | 64 | Non-diagnostic request (must pass through) |
| 7 | `shutdown` response | 38 | Result null |

**Purpose**: Authentic LSP wire data for testing the parser and filter against real-world traffic patterns (multiple messages, mixed types, incremental diagnostic refinement).

## Success Criteria

### Parser

1. `MessageParser::feed()` on the full 9831-byte fixture returns exactly 7 parsed messages
2. Each parsed message's JSON body round-trips through `serde_json::from_slice` (all 7 are valid JSON)
3. Content-Length values parsed correctly: 2790, 193, 706, 1925, 3953, 64, 38
4. Partial header feed (e.g., `Content-Len`) → no messages returned → subsequent feed with remainder → message returned
5. Multiple messages in a single feed call return all messages
6. Malformed Content-Length (non-numeric, missing) → skipped without panic
7. Partial JSON body → no message returned → subsequent feed with remainder → message returned

### Filter (DROP_DIAGNOSTICS=true, the default)

8. Messages 1, 6, 7 pass through filter() unchanged (same JSON value)
9. Messages 2-5 (publishDiagnostics) are dropped (filter returns None)
10. Any non-publishDiagnostics message passes through unchanged

### Filter (DROP_DIAGNOSTICS=false, MIN_SEVERITY=1)

11. Message 3 passes through with both severity-1 diagnostics preserved
12. Message 5 passes through with 4 diagnostics (only severity 1), severity-4 diagnostics removed
13. Message 2 (0 diagnostics) passes through unchanged

### Proxy Integration (mock server)

14. Spawn a mock LSP server that emits framed diagnostics → receive filtered stream
15. Non-diagnostics pass through the proxy unchanged
16. Child process exit code is propagated (mock exits 42 → proxy exits 42)
17. Signal to proxy → signal forwarded to child, both exit

### Config

18. `STAY_FRESH_DROP_DIAGNOSTICS` default is `true`
19. `STAY_FRESH_MIN_SEVERITY` default is `1`
20. `STAY_FRESH_LOG=false` produces no debug output; `=true` produces eprintln output

## Architecture Decisions

1. **No async runtime** — 4 OS threads coordinated via OS pipe semantics (per plan)
2. **serde_json::Value for JSON** — no strongly-typed LSP structs (per plan)
3. **clap derive for CLI** — per plan
4. **ctrlc for signal handling** — maps SIGTERM/SIGINT to child process kill
5. **eprintln! for debug logging** — gated by STAY_FRESH_LOG env var (per plan)
6. **No `setup` subcommand** — deferred to future phase; proxy-only binary
7. **Plugin format** — matches fortls-lsp format (`lspServers` key, `command` + `args` + `extensionToLanguage`)
8. **No channels/Arc/AtomicBool** — OS pipe close on child death is the synchronization mechanism

## Domain Terms Validated

All terms in CONTEXT.md confirmed correct. No updates needed.

## Fixture Anchoring Notes

- The captured fixture is path-dependent: `uri` fields contain absolute paths to the user's machine. Tests that compare JSON structure should use structural matching (check `method` field, `diagnostics` length) rather than byte-equality.
- Severity values verified against LSP specification: 1=Error, 2=Warning, 3=Info, 4=Hint.
- The `publishDiagnostics` messages arrive in a specific ordering (0 diags → 2 → 4 → 7) as rust-analyzer progressively refines its analysis. The filter does not depend on ordering.
