---
name: wait-for-lsp-proxy tasks
description: Forensic task breakdown for the wait-for-lsp proxy implementation
type: project
---

# TASKS: wait-for-lsp-proxy

## Declared Fixtures

| Path | Description |
|------|-------------|
| `tests/fixtures/rust-analyzer-session.bin` | 9831 bytes of captured LSP wire traffic (7 framed messages) |
| `tests/fixtures/lsp-workspace/Cargo.toml` | Minimal Cargo workspace for rust-analyzer diagnostics |
| `tests/fixtures/lsp-workspace/src/main.rs` | Rust source with intentional type errors (triggers 4+ severity-1 diagnostics) |

## Test Fixture

To regenerate the fixture: `uv run python tests/fixtures/capture_lsp_traffic.py`

Requires `rust-analyzer` on PATH. The fixture contains 7 messages:
- 1 initialize response, 4 publishDiagnostics (0→2→4→7 diags), 1 refresh request, 1 shutdown response

## Exploration Notes

- Severity ordering within a publishDiagnostics message can vary; criteria check counts per severity, not positions.
- Message 5 has 4× severity-1 + 3× severity-4, not 3+4 (adjusted during fixture exploration).
- All 9831 bytes parse as valid Content-Length framed JSON — no malformed messages in real traffic, but the parser must still handle them.
- The fixture is path-dependent (absolute `uri` fields); tests use structural matching, not byte-equality.

---

## Group A: Project Scaffold

### TASK-1: Initialize cargo project with dependencies
**Kind:** direct
**Goal:** Runnable Rust project skeleton with all dependencies declared

**Guidance:**
- Run `cargo init` to create the crate
- Add dependencies: `serde_json` (for `serde_json::Value`), `clap` (derive feature), `ctrlc`
- Set `edition = "2021"`
- Create `src/lib.rs` — the library root for parser, filter, config
- Create `src/main.rs` — CLI entry point referencing the lib

**Success Criteria:**
- `cargo check` passes
- `cargo test` runs 0 tests successfully
- `cargo clippy -- -D warnings` passes (or suppress warnings appropriately for empty lib)

**Files to create:**
- `Cargo.toml`
- `src/lib.rs`
- `src/main.rs`

---

## Group B: Config Module

### TASK-2: Implement config.rs — environment variable parsing
**Kind:** direct
**Goal:** Read STAY_FRESH_DROP_DIAGNOSTICS, STAY_FRESH_MIN_SEVERITY, STAY_FRESH_LOG from the environment

**Guidance:**
- Define a `Config` struct with fields: `drop_diagnostics: bool` (default true), `min_severity: u8` (default 1), `log_enabled: bool` (default false)
- Implement `Config::from_env()` that reads env vars via `std::env::var`
- `STAY_FRESH_DROP_DIAGNOSTICS`: "false" → false, anything else including absent → true
- `STAY_FRESH_MIN_SEVERITY`: parse as u8, default 1 on missing/invalid
- `STAY_FRESH_LOG`: "true" → true, anything else → false
- Include `Config::log_enabled()` convenience method

**Verification:**
- Test each env var separately with explicit values
- Test defaults when env vars are absent
- Test "false" → false for drop_diagnostics, "true" → true for log

**Files:**
- `src/config.rs` (export from lib.rs)

---

## Group C: Parser Module

### TASK-3: Implement parser.rs — LSP wire-protocol MessageParser
**Kind:** lib-tdd
**Goal:** Parse Content-Length framed LSP messages from a byte stream

**Success Criteria:**
- `MessageParser::new()` creates empty parser
- `parser.feed(b"Content-Length: 2790\r\n\r\n" + 2790_bytes)` returns 1 parsed message
- `parser.feed()` on the full 9831-byte fixture returns exactly 7 `serde_json::Value` items
- Each parsed value is valid JSON (serde_json::from_slice succeeds)
- Partial header feed → 0 results → subsequent feed with remainder → message returned
- Multiple messages concatenated in single feed → all N messages returned
- Content-Length parsing is case-insensitive (`content-length:`, `CONTENT-LENGTH:`, `Content-Length:`)
- Whitespace-tolerant: `Content-Length: 2790` and `Content-Length:2790` both work
- Malformed header (no Content-Length found) → skipped, buffer advanced past next `\r\n\r\n`
- Partial JSON body → 0 results → subsequent feed with remainder → message returned
- Invalid JSON body (after complete Content-Length) → skipped, no panic

**Test file:** `tests/parser_tests.rs`

**Acceptance:**
```bash
cargo test -- test_parser_replay_fixture  # uses tests/fixtures/rust-analyzer-session.bin
cargo test -- test_parser_  # all parser tests
```

**Exploration notes:**
- Real LSP traffic has clean framing — the parser robustness features (malformed headers, partial bodies) are tested with synthetic data, not the fixture.

**Files:**
- `src/parser.rs` (export from lib.rs)

---

## Group D: Filter Module

### TASK-4: Implement filter.rs — diagnostic filtering
**Kind:** lib-tdd
**Goal:** Drop or filter publishDiagnostics notifications based on config

**Success Criteria (DROP_DIAGNOSTICS=true, default):**
- Message 1 (initialize response) → passes through unchanged (same JSON value)
- Message 6 (workspace/diagnostic/refresh) → passes through unchanged
- Message 7 (shutdown response) → passes through unchanged
- Message 2 (publishDiagnostics, 0 diags) → dropped (returns None)
- Message 3 (publishDiagnostics, 2 diags) → dropped
- Messages 4, 5 (publishDiagnostics, 4/7 diags) → dropped
- Any non-notification message (has `id`, no `method`) → passes through unchanged

**Success Criteria (DROP_DIAGNOSTICS=false, MIN_SEVERITY=1):**
- Message 3: passes through with 2 diagnostics (both severity 1)
- Message 5: passes through with 4 diagnostics (severity 1 only); 3 severity-4 diagnostics removed
- Message 2 (0 diagnostics): passes through (empty diagnostics array unchanged)
- Diagnostic without severity field gets treated as severity 1 (LSP default)
- MIN_SEVERITY=2 keeps severity 1+2, drops 3+4
- MIN_SEVERITY=4 keeps all severities

**Test file:** `tests/filter_tests.rs`

**Acceptance:**
```bash
cargo test -- test_filter_  # all filter tests
```

**Guidance for parsed_message manipulation:**
- Work with `&mut serde_json::Value`
- Access `msg["method"]` for method check
- Access `msg["params"]["diagnostics"]` for the diagnostics array
- Use `Value::Array` for diagnostics, filter in place with `retain`
- For drop mode, return `None` (caller skips the message)
- For severity mode, mutate `msg["params"]["diagnostics"]` in place and return `Some(msg)`

**Exploration notes:**
- Message 5 has 4 severity-1 + 3 severity-4 diagnostics (adjusted during fixture validation).

**Files:**
- `src/filter.rs` (export from lib.rs)

---

## Group E: Proxy Module

### TASK-5: Implement proxy.rs — child process I/O orchestration
**Kind:** direct
**Goal:** Spawn real LSP server, forward stdin/stderr, filter stdout, propagate signals and exit codes

**Guidance:**

Define a `Proxy` struct or module-level function:

```
fn run_proxy(config: &Config, lsp_command: &str, lsp_args: &[String]) -> i32
```

Implementation:
1. Spawn child process via `std::process::Command` with piped stdin/stdout/stderr
2. Spawn 3 reader/writer threads:
   - **stdin_thread**: reads from `std::io::stdin()`, writes to `child.stdin`. Exits on EOF or broken pipe.
   - **stdout_thread**: reads from `child.stdout`, feeds `MessageParser`, applies `filter()`, writes filtered output to `std::io::stdout()`. Exits on EOF (pipe closed).
   - **stderr_thread**: reads from `child.stderr`, writes raw bytes to `std::io::stderr()`. Exits on EOF.
3. Use `ctrlc::set_handler` to kill child on Ctrl+C / SIGTERM
4. Main thread calls `child.wait()` — blocks until child exits
5. After wait, join all threads (they should exit naturally as pipes close)
6. Return child's exit code

**Important considerations:**
- Use `std::thread::spawn` for each I/O thread
- The signal handler must be set in the main thread before spawning (ctrlc limitation)
- All I/O uses raw byte buffers `[u8; 4096]` or `[u8; 8192]` — no line-based reading
- Child stdin handle must be closed after stdin_thread exits, or use `take()` on child.stdin to avoid deadlock
- Debug logging with eprintln! gated on `config.log_enabled()`

**Error handling:**
- If child fails to spawn → print error to stderr, return exit code 1
- If an I/O thread panics → still try to kill child and return error
- Use `child.kill()` in signal handler (ctrlc handler closure captures `&child`)

**Files:**
- `src/proxy.rs` (export from lib.rs)

---

## Group F: Main Entry Point

### TASK-6: Implement main.rs — CLI entry and signal handling
**Kind:** direct
**Goal:** Parse CLI args, load config, set up signal handler, run proxy, propagate exit code

**Guidance:**
- Use clap derive for argument parsing:

```rust
#[derive(Parser)]
struct Args {
    /// LSP server command to proxy
    lsp_command: String,
    /// Arguments to pass to the LSP server
    lsp_args: Vec<String>,
}
```

- `main()` flow: parse args → load config → set ctrlc handler → call `proxy::run_proxy()` → exit with returned code
- If no arguments provided, print usage and exit with code 1
- Debug-log the config at startup if `log_enabled()` is true

**Files:**
- `src/main.rs`

---

## Group G: Plugin JSON

### TASK-7: Create rust-analyzer plugin.json
**Kind:** direct
**Goal:** Plugin JSON for installing the proxy as the rust-analyzer LSP command

**Guidance:**
- Create `plugins/wait-for-lsp-rust/plugin.json`
- Format matches fortls-lsp: `name`, `version`, `lspServers` with `command: "wait-for-lsp"` + `args: ["rust-analyzer"]`
- Extension mapping: `.rs` → `rust`

**Plugin JSON:**
```json
{
  "name": "wait-for-lsp-rust",
  "description": "Rust language server (rust-analyzer) via wait-for-lsp proxy — filters stale diagnostics",
  "version": "0.1.0",
  "lspServers": {
    "rust-analyzer": {
      "command": "wait-for-lsp",
      "args": ["rust-analyzer"],
      "extensionToLanguage": {
        ".rs": "rust"
      }
    }
  }
}
```

**Files:**
- `plugins/wait-for-lsp-rust/plugin.json`

---

## Group H: Integration Tests

### TASK-8: Integration tests with mock LSP server
**Kind:** direct
**Goal:** End-to-end validation of the proxy against a simulated LSP server

**Guidance:**

Write `tests/integration_test.rs` with a helper `MockLspServer` that:
1. Reads pre-framed messages from stdin (or a pre-defined sequence)
2. Echoes back messages with known Content-Length framing
3. Exits with a configurable exit code

Integration test scenarios:

**Scenario 1: Diagnostics filtered**
- Mock server emits: [initialize_response, publishDiagnostics(3 diags), shutdown_response]
- Proxy receives filtered stream: [initialize_response, shutdown_response] (diags dropped)

**Scenario 2: Non-diagnostics pass through**
- Mock server emits: [initialize_response, hover_response, shutdown_response]
- Proxy output matches input exactly

**Scenario 3: Exit code propagation**
- Mock server exits with 42
- Proxy exits with 42

**Scenario 4: Signal forwarding**
- Spawn proxy in background
- Send SIGTERM to proxy
- Verify both proxy and mock child exit

**Files:**
- `tests/integration_test.rs`

---

## Dependency Map

```
TASK-1 (scaffold) ─┬─→ TASK-2 (config) ──→ TASK-3 (parser) ──→ TASK-4 (filter)
                    │                                                    ↓
                    └────────────────────────────────────────→ TASK-5 (proxy)
                                                                     ↓
                                                              TASK-6 (main)
                                                                     ↓
                                                              TASK-7 (plugin)
                                                                     ↓
                                                              TASK-8 (integration)
```
