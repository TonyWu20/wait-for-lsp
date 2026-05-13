# wait-for-lsp: Rust Rewrite of stay-fresh-lsp-proxy

## Context

Claude Code's LSP plugins push `textDocument/publishDiagnostics` after every edit, but the diagnostics arrive from the *previous* file state — not the current one. Claude sees stale errors, thinks the code is broken, and tries to "fix" problems that don't exist. This causes unnecessary reverts, fix loops, and wasted turns.

The [`iloom-ai/stay-fresh-lsp-proxy`](https://github.com/iloom-ai/stay-fresh-lsp-proxy) (TypeScript) solves this by acting as an LSP proxy that filters stale diagnostics. This project is a **Rust rewrite** with the same core behavior, plus Fortran support.

## Architecture Overview

```
Claude Code → .lsp.json specifies: wait-for-lsp <real-lsp-server> [args]
                       │
                       ▼
              wait-for-lsp (proxy binary)
                 ├── spawns real LSP server as child process
                 ├── stdin:  raw bytes forwarded verbatim (no parsing)
                 ├── stdout: parsed, textDocument/publishDiagnostics filtered, re-encoded
                 ├── stderr: raw bytes forwarded verbatim
                 └── signals: SIGTERM/SIGINT forwarded to child; child exit propagated
```

## Source Files

```
src/
  main.rs       -- CLI entry (clap), setup signal handler, spawn proxy threads, propagate exit code
  config.rs     -- Env var reading (STAY_FRESH_DROP_DIAGNOSTICS, STAY_FRESH_MIN_SEVERITY, STAY_FRESH_LOG)
  parser.rs     -- LSP wire-protocol MessageParser (Content-Length framing, state machine)
  filter.rs     -- Diagnostic filtering logic (drop-all or severity-based)
  proxy.rs      -- Child spawn, 3 I/O threads, signal handler, thread join + exit propagation

plugins/
  wait-for-lsp-typescript/plugin.json
  wait-for-lsp-python/plugin.json
  wait-for-lsp-rust/plugin.json
  wait-for-lsp-fortran/plugin.json
```

## Key Dependencies (Cargo.toml)

- `serde_json`: parse/filter/re-serialize LSP JSON messages (use `serde_json::Value`, not strongly-typed structs)
- `clap` (derive): CLI argument parsing
- `ctrlc`: signal handling (SIGTERM/SIGINT forwarding to child process)

No async runtime. Debug logging via `eprintln!` + `STAY_FRESH_LOG` env var check.

## Core Algorithm: MessageParser

LSP wire protocol uses HTTP-like framing: `Content-Length: <N>\r\n\r\n<JSON body>`.

- State machine: `buffer: Vec<u8>`, `content_length: Option<usize>`
- `feed(&mut self, chunk: &[u8]) -> Vec<serde_json::Value>`
- Scan for `\r\n\r\n` header terminator, parse `Content-Length` (case-insensitive, whitespace-tolerant)
- Accumulate body bytes, extract and JSON-parse
- Handle: partial headers/bodies, malformed headers (skip), invalid JSON (skip), multiple messages in one chunk

## Filter Logic

Check `msg["method"] == "textDocument/publishDiagnostics"`:
- If `STAY_FRESH_DROP_DIAGNOSTICS != "false"` (default): **drop entire notification**
- Else: filter `params.diagnostics` array, keep only entries where `severity.unwrap_or(1) <= STAY_FRESH_MIN_SEVERITY` (default 1)

All other messages (requests, responses, other notifications) pass through unchanged.

## Thread Architecture

No async runtime — 4 OS threads, coordinated via OS pipe semantics. When the child process dies (naturally or killed by signal), all pipes close, threads exit their I/O loops, and main joins them.

```
main thread
  ├── child.wait()  ← blocks on child exit

  ├── stdin_thread
  │     stdin.read(buf) → child_stdin.write_all(buf)
  │     exits when: stdin EOF OR child_stdin pipe closes (child died)

  ├── stdout_thread
  │     child_stdout.read(buf) → parser.feed(buf) → filter → stdout.write_all(buf)
  │     exits when: child_stdout.read() returns 0 (pipe closed)

  ├── stderr_thread
  │     child_stderr.read(buf) → stderr.write_all(buf)
  │     exits when: child_stderr.read() returns 0 (pipe closed)

  └── signal_thread
        ctrlc::set_handler → kills child
        exits when: child.kill() returns
```

**Coordination flow (normal exit):**
1. Child LSP exits → `child.wait()` returns
2. Child stdin/stdout/stderr pipes close
3. All three I/O threads hit EOF → exit their loops
4. Main joins all threads, returns child's exit code

**Coordination flow (signal):**
1. SIGTERM/SIGINT caught by `ctrlc` handler
2. `child.kill()` sends signal to child
3. Child dies → pipes close → same drain cascade as above

No channels, no `Arc<AtomicBool>`, no `select!`. The OS already provides the synchronization.

## Plugin Format

Each plugin is a single `plugin.json` with `lspServers` field (matches the fortls-lsp format already in use):

```json
{
  "name": "wait-for-lsp-fortran",
  "version": "0.1.0",
  "lspServers": {
    "fortls": {
      "command": "wait-for-lsp",
      "args": ["fortls"],
      "extensionToLanguage": { ".f90": "fortran", ... }
    }
  }
}
```

The proxy binary is invoked as: `wait-for-lsp <real-lsp-server> [server-args...]`

## CLI Interface

```
wait-for-lsp <lsp-command> [lsp-args...]    # Proxy mode (default)
wait-for-lsp setup --typescript --python --rust --fortran   # Install plugins
```

## Configuration (env vars, in ~/.claude/settings.json)

| Variable | Default | Description |
|----------|---------|-------------|
| `STAY_FRESH_DROP_DIAGNOSTICS` | `true` | Drop all diagnostics |
| `STAY_FRESH_MIN_SEVERITY` | `1` | Max severity to keep (1=Error,2=Warn,3=Info,4=Hint) |
| `STAY_FRESH_LOG` | `false` | Enable debug logging |

## Distribution

- `cargo install --git https://github.com/TonyWu20/wait-for-lsp` (or from crates.io later)
- GitHub Releases with pre-built binaries for macOS (arm64/x86_64) and Linux (x86_64)
- Plugin JSON files installed by cloning the repo and running `claude plugin install <path>`

## Implementation Sequence

1. `cargo init` + `Cargo.toml`
2. `config.rs` — simplest module
3. `parser.rs` — core algorithm, heavy unit tests
4. `filter.rs` — filtering logic, unit tests
5. `proxy.rs` — async I/O orchestration
6. `main.rs` — CLI + signal handling
7. Plugin JSON files (4 languages)
8. Integration tests (mock LSP server)
9. README + CI (GitHub Actions)

## Verification

1. **Unit tests**: `cargo test` — parser edge cases (13+ tests), filter logic (7+ tests), config parsing
2. **Integration test**: spawn mock LSP server (shell script) that emits framed diagnostics → verify diagnostics filtered, non-diagnostics pass through, child exit code propagated, signal kills cascade correctly
3. **Manual smoke test**: `echo '...init message...' | wait-for-lsp rust-analyzer` — proxy forwards response
4. **Claude Code end-to-end**: install plugin, edit file, verify no stale diagnostics reach Claude
