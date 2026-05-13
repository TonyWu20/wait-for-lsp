## 2026-05-13: stale-system-reminder-gap
**Root cause**: publishDiagnostics messages were forwarded immediately, letting intermediate diagnostic states reach the client where they were cached as stale.
**Fix**: `src/proxy.rs:173-215` — per-URI dedup queue flushes only the latest diagnostic per read cycle.
**Pattern**: message-dedup
