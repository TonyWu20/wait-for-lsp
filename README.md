# wait-for-lsp

LSP proxy that filters stale `textDocument/publishDiagnostics` between Claude Code and any LSP server, preventing fix loops caused by outdated diagnostics.

## How it works

```
Claude Code  ←→  wait-for-lsp  ←→  rust-analyzer (or any LSP server)
```

The proxy sits between Claude Code and the LSP server, forwarding all messages transparently except `publishDiagnostics`. Diagnostics are filtered through three layers:

1. **Diagnostic Drop** (`STAY_FRESH_DROP_DIAGNOSTICS=true`) — drops all diagnostics (default for safety)
2. **Severity Filter** (`STAY_FRESH_MIN_SEVERITY=1`) — keeps only diagnostics at or below the threshold
3. **Version Tracking** (`STAY_FRESH_STALE_FILTER=true`) — tracks `didOpen`/`didChange` versions per URI; drops `publishDiagnostics` older than the tracked version
4. **Dedup Queue** — buffers per-URI; only the latest diagnostic per URI per read cycle reaches the client

## Stable-filter mode (recommended)

Set these env vars to pass all diagnostics but drop stale ones:

```
STAY_FRESH_DROP_DIAGNOSTICS=false
STAY_FRESH_MIN_SEVERITY=4
STAY_FRESH_STALE_FILTER=true   # default
```

This means: if you edit a file and rust-analyzer starts re-analyzing, intermediate/incremental diagnostics from the previous state are dropped. You only see the final, correct diagnostic state after re-analysis completes.

## Installation

```bash
cargo install --path .
claude plugin install plugins/wait-for-lsp-rust --scope project
claude plugin disable rust-analyzer-lsp@claude-plugins-official --scope user
```

## Env vars

| Variable | Default | Description |
|----------|---------|-------------|
| `STAY_FRESH_DROP_DIAGNOSTICS` | `true` | Drop all diagnostics (kill switch) |
| `STAY_FRESH_MIN_SEVERITY` | `1` | Max severity to keep (1=Error..4=Hint) |
| `STAY_FRESH_STALE_FILTER` | `true` | Version-based stale detection |
| `STAY_FRESH_LOG` | `false` | Debug logging to stderr |
