# Investigation: Stale diagnostics visible in system-reminders despite version tracking

## Symptom

After editing Rust files through the wait-for-lsp proxy, stale diagnostics appear in Claude Code's system reminder `<new-diagnostics>` blocks. The proxy's version tracking correctly filters `publishDiagnostics` at the LSP protocol level (older version → dropped), but the client still shows cached diagnostics from the last forwarded state.

## Claims from Prior Investigation (this session)

### Claim 1: "Version tracking works — publishDiagnostics with version < tracked version are dropped"
- **Classification**: DERIVED (verified via proxy log `[wait-for-lsp] tracked <uri> version <n>` and `cargo check --tests` confirming 0 errors)
- **Evidence**: Session log shows `[wait-for-lsp] tracked file:///.../filter.rs version 4` followed by `cargo check --tests` exit 0. The `test_stale_diagnostic_dropped_when_older` unit test passes.
- **Admissible?**: Yes, for the claim that the proxy correctly drops old-version publishDiagnostics at the protocol level.

### Claim 2: "Claude Code sends version numbers in didChange"
- **Classification**: EXTERNAL (verified via proxy log capture)
- **Evidence**: `[wait-for-lsp] tracked file:///.../main.rs version 2` appeared after an Edit tool invocation. Each edit increments the version.
- **Admissible?**: Yes. The LSP client sends incrementing version numbers.

### Claim 3: "System-reminder shows stale diagnostics after fix"
- **Classification**: EXTERNAL (observed directly in this session)
- **Evidence**: After fixing `strict_mode` field in `filter.rs:278`, system-reminder continued showing `struct config::Config has no field named 'strict_mode'` at line 278. `cargo check --tests` confirmed 0 errors.
- **Admissible?**: Yes. This is the primary observed symptom.

### Claim 4: "The official stay-fresh-lsp-proxy (TypeScript) does not implement version tracking"
- **Classification**: EXTERNAL (verified by reading source code at https://github.com/iloom-ai/stay-fresh-lsp-proxy)
- **Evidence**: `proxy.ts` has only `dropAll` and `minSeverity` — no stdin-side parsing, no version map, no didChange tracking.
- **Admissible?**: Yes. Relevant for establishing that version tracking is a novel improvement.

### Claim 5: "The remaining gap is a timing/race issue"
- **Classification**: HYPOTHESIZED (inferred from observed behavior)
- **Evidence**: The proxy drops stale publishDiagnostics, but the client's system-reminder mechanism appears to cache the last received diagnostic state. When the proxy drops the intermediate state, the client never receives a replacement until rust-analyzer finishes processing the latest didChange and pushes fresh diagnostics.
- **Admissible?**: Not as a criterion, but as a hypothesis to test.

## Open Questions

1. Does the `<new-diagnostics>` system-reminder come from LSP publishDiagnostics, or from a separate file-watch-based diagnostic pathway?
2. If from LSP: does the proxy forward an *empty* publishDiagnostics (0 diagnostics) after the final re-analysis, clearing the stale state?
3. If from file-watch: can the proxy influence this pathway at all?
