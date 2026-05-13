# Divergence Surface: stale-system-reminder-gap

## Proxy Pipeline Anatomy

```
Client (Claude Code)          Proxy                         Server (rust-analyzer)
      │                         │                                  │
      │  ── didChange(u, v_N) ──►── forward ──────────────────────►│
      │                         │  track v_N                       │  ╔══ RACE WINDOW ═══╗
      │                         │                                  │  ║ (re-analysis)    ║
      │  ── didChange(u, v_N+1)─►── forward ──────────────────────►│  ║                   ║
      │                         │  track v_N+1                     │  ╚══════════════════╝
      │                         │  ◄── publishDiagnostics(u, v_N) ──│  (stale — version N)
      │                         │  DROP (v_N < v_N+1)              │
      │                         │  ◄── publishDiagnostics(u, v_N+1)──│  (fresh)
      │  ◄── publishDiagnostics─── forward ────────────────────────│
      │       (u, v_N+1)        │                                  │
```

## Divergence Items

### D1: System-reminder path — is it the same LSP stream?
- **Hypothesis**: The `<new-diagnostics>` system-reminder mechanism may NOT come from
  the LSP publishDiagnostics stream. It might come from a file-watch-based diagnostic
  aggregation that bypasses the proxy entirely.
- **Evidence needed**: Does the system-reminder appear when the proxy drops a
  publishDiagnostics but NOT when it forwards one?
- **Test**: Enable `STAY_FRESH_DROP_DIAGNOSTICS=true` (drops ALL diagnostics). If
  system-reminders still show diagnostics, they come from a non-LSP path.
- **Status**: TO BE TESTED in Step 7

### D2: Proxy drops stale diag, client never receives replacement
- **Hypothesis**: When the proxy drops `publishDiagnostics(u, v_N)` (stale), the client
  has no way to know that the server has advanced past v_N. It holds the last
  forwarded diagnostics at version v_N-1 (or earlier). Until `publishDiagnostics(u, v_N+1)`
  arrives, the client shows stale cached state.
- **Evidence**: This session — after fixing `filter.rs:278`, the system-reminder still
  showed the error for several seconds before clearing.
- **Mitigation**: Debounce — after `didChange(u, v_N+1)`, suppress ALL
  publishDiagnostics for u until either (a) a matching or newer version arrives, or
  (b) a timeout expires and a fallback state is forwarded.
- **Status**: TO BE TESTED in Step 7

### D3: Version numbers do not monotonically increase
- **Hypothesis**: Some LSP clients reuse version numbers or send version=null.
  If the proxy tracks version=null, the comparison `msg_version < tracked` would
  never trigger (null < any → false).
- **Evidence**: Counter-evidence in this session — proxy logged
  `tracked ... version 2`, `tracked ... version 3`, etc. Monotonically increasing.
- **Status**: RULED OUT by Criterion 1 (this session's log evidence)

### D4: Race between stdin thread and stdout thread
- **Hypothesis**: The stdin thread updates the version map, but the stdout thread
  may read stale version map data (before the lock is released) when checking a
  publishDiagnostics.
- **Architecture**: `Arc<Mutex<HashMap<String, i64>>>` — both threads acquire the
  lock. The Mutex guarantees mutual exclusion. No race condition on the version map.
- **Status**: RULED OUT by architecture (Mutex provides happens-before)

### D5: publishDiagnostics without version field
- **Hypothesis**: The proxy skips staleness check when `params.version` is missing
  (passes through). If rust-analyzer omits the version, all diagnostics pass
  without staleness checking.
- **Evidence**: Proved in this session — proxy consistently logs "tracked ... version N"
  and the unit test `test_stale_filter_no_version_in_message_passes` confirms this
  fallback behavior.
- **Status**: PARTIAL — rust-analyzer does include version, but the fallback
  (no version → pass through) means any LSP server that omits version will bypass
  stale filtering.

### D6: File-system watch bypasses LSP
- **Hypothesis**: Claude Code may use a file-system watcher (inotify/FSEvents) to
  detect file changes and request diagnostics directly, bypassing the LSP proxy.
  This would mean the diagnostics in system-reminders come from a direct
  client→server request, not through the proxy's filtered stream.
- **Evidence needed**: Does disabling the proxy (using official rust-analyzer plugin)
  change system-reminder behavior? If the same behavior occurs, the path is
  outside the proxy.
- **Status**: TO BE TESTED in Step 7 — this would make the entire proxy
  ineffective for the system-reminder gap, though LSP tool operations still benefit.

### D7: Debounce window too narrow
- **Hypothesis**: Even with version tracking, the proxy forwards stale diagnostics
  if rust-analyzer pushes them BEFORE the proxy processes the didChange. The
  stdin/stdout threads are independent — the stdout thread might read a
  publishDiagnostics for version N before the stdin thread has processed
  the didChange for version N+1.
- **Architecture**: This is theoretically possible but unlikely in practice because
  the stdin thread reads stdin in the same OS pipe write that the client sends.
  The Rust stdio buffers are flushed per write, so the proxy should see the
  didChange before rust-analyzer processes it (and thus before it responds).
- **Status**: TO BE TESTED in Step 7 — the debounce approach would eliminate
  even this theoretical race.

## Summary

| Item | Status | Next Step |
|------|--------|-----------|
| D1 (system-reminder path) | TO BE TESTED | Enable drop-all mode, see if reminders appear |
| D2 (no replacement forwarded) | TO BE TESTED | Implement debounce |
| D3 (non-monotonic versions) | RULED OUT | Log evidence |
| D4 (thread race) | RULED OUT | Architecture |
| D5 (missing version field) | PARTIAL | Known fallback, document |
| D6 (file-watch bypass) | TO BE TESTED | Compare with/without proxy |
| D7 (stdin/stdout race) | TO BE TESTED | Debounce eliminates this too |
