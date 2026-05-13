# Diagnostic Self-Test: Stale Filter Verification

## Diagnostics Enumerated

| # | Diagnostic | Purpose | Verification |
|---|-----------|---------|-------------|
| 1 | `test_stale_diagnostic_dropped_when_older` | Version < tracked → dropped | PASS |
| 2 | `test_fresh_diagnostic_passes_when_matching` | Version == tracked → passes | PASS |
| 3 | `test_fresh_diagnostic_passes_when_newer` | Version > tracked → passes | PASS |
| 4 | `test_stale_filter_no_version_in_message_passes` | No version in msg → passes | PASS |
| 5 | `test_stale_filter_no_tracked_version_passes` | No tracked version → passes | PASS |
| 6 | `test_stale_filter_disabled_passes_old_diag` | Filter off → passes | PASS |
| 7 | `test_stale_filter_different_file_passes` | Different URI → passes | PASS |
| 8 | `test_stale_filter_severity_also_applied` | Severity filter still applies | PASS |
| 9 | Live proxy log evidence (this session) | Version tracking visible in `/tmp/wait-for-lsp-err.log` | PASS |

## Gap: No diagnostic tests the debounce scenario

There is no test that simulates the following real-world sequence:
1. Client sends `didChange(u, v_2)` — tracked version becomes 2
2. Server pushes `publishDiagnostics(u, v_1)` — stale, correctly dropped
3. Server pushes `publishDiagnostics(u, v_2)` — fresh, correctly forwarded
4. **Client caches the last forwarded state (v_1) during the gap between steps 2 and 3**

The existing tests verify steps 1-3. Step 4 is the gap.

## Suggested Tight Test

A test that verifies debounce behavior:
- Set debounce window to 100ms
- Send `didChange(u, v_2)`
- Send `publishDiagnostics(u, v_1)` within debounce window → dropped
- Send `publishDiagnostics(u, v_2)` within debounce window → queued, not forwarded
- Wait 150ms → only the latest (v_2) is forwarded, v_1 never seen by client
