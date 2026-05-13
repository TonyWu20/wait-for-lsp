# ADR-001: Per-URI dedup queue for publishDiagnostics

**Date**: 2026-05-13

## Context

The proxy forwards `publishDiagnostics` from the LSP server to Claude Code. Without any buffering, every diagnostic notification reaches the client immediately. This causes two stale-diagnostic problems:

1. **Incremental refinement**: rust-analyzer pushes multiple `publishDiagnostics` at the same version as analysis progresses (e.g., "3 errors found" → "2 errors found" → "1 error found"). Without dedup, each batch reaches the client acting on the first "3 errors" is a waste cycle.

2. **Interleaved cross-file updates**: When the agent edits two files in rapid succession, diagnostics for file A arrive before file B's re-analysis finishes. The client sees a partial cross-file state — file A has errors (current), file B shows clean (but rust-analyzer hasn't finished analyzing the change yet).

Version tracking (ADR-n/a, implemented same session) handles the cross-version case (didChange v_N+1 renders publishDiagnostics v_N stale) but does NOT handle the same-version incremental case, nor the interleaved cross-file case, because both arrive at the same version.

## Decision

Add a per-URI `HashMap<String, Value>` buffer (`Dedup Queue`) in the proxy's stdout filter thread:

- When `filter_message` returns a `publishDiagnostics` message, extract its URI and insert it into the queue (overwriting any previous entry for that URI).
- When `filter_message` returns a non-diagnostic message (hover, go-to-definition, etc.), forward it immediately — no buffering.
- When `filter_message` returns `None` (message dropped by drop/severity/stale filter), log the drop — no queue entry.
- At the end of each read cycle (after processing all messages from the current `child_stdout.read()` chunk), flush the queue: forward the latest `publishDiagnostics` per URI to `stdout`.

## Consequences

**Positive:**
- Incremental refinement at the same version is deduplicated — only the final state reaches Claude.
- Cross-file edits don't expose partial state — both files' diagnostics flush together.
- Zero additional latency in steady state (no timer, no debounce window — flush is tied to the natural read cycle).
- Non-diagnostic LSP operations (hover, completions, go-to-definition) are unaffected — they bypass the queue.

**Negative:**
- Slightly longer time-to-diagnostic during active analysis (one read cycle delay). In practice this is <1ms since the pipe read completes when rust-analyzer finishes writing its response.
- Memory: queue holds at most one `Value` per open URI. Negligible for realistic workspace sizes.

**Neutral:**
- The queue is per-thread (`HashMap` in the stdout thread, no `Arc<Mutex<>>` needed) — no cross-thread synchronization.
- The `filter_message` function itself is unchanged — the queue is added in the caller's loop in `thread_filter_stdout`.

## Alternatives Considered

- **Timer-based debounce**: Forward diagnostics after a 100-200ms quiet period. Rejected because it adds predictable latency even in steady state, and a timer thread adds complexity. The read-cycle-based approach achieves the same dedup without timers.
- **Version-matching only**: Forward only when `publishDiagnostics.version == trackedVersion`. Already in place. Doesn't handle incremental refinement at the same version (rust-analyzer pushes multiple v2 batches).
- **No dedup**: The original design. Causes the stale-diagnostic problems described above.
