# Anchor Criteria: stale-system-reminder-gap

## Fixture Files

- `tests/fixtures/rust-analyzer-session.bin` — Captured LSP wire traffic (7 server→client messages). Contains 4 `publishDiagnostics` notifications at versions matching the session's didOpen. Does NOT contain the full round-trip (client→server→client) for a rapid-edit scenario.
- `tests/fixtures/lsp-workspace/` — Minimal Cargo workspace with intentional type errors for rust-analyzer to analyze.

## LSP Specification Anchors

### Content-Length Framing (LSP base protocol)
- Source: LSP Spec § 3.1 (Base Protocol), Content-Length header framing
- All messages use `Content-Length: <N>\r\n\r\n<JSON>` format
- No implicit message boundaries — Content-Length is authoritative

### textDocument/publishDiagnostics Notification (LSP Spec § 3.16.0)
- Source: LSP Spec § 3.16.0
- `params.version?: integer` — Optional version number. When present, indicates the
  document version for which these diagnostics were computed.
- Source: rust-analyzer source, `publish_diagnostics` implementation:
  `params.version` is set to the document version at analysis time

### textDocument/didChange Notification (LSP Spec § 3.7.4)
- Source: LSP Spec § 3.7.4
- `params.textDocument.version: integer` — REQUIRED. The version number of the
  document after the change.

### Version Tracking Invariant
- Source: LSP Spec § 3.7.4 + § 3.16.0 composition
- After a `didChange(v_N+1)` for URI u, any `publishDiagnostics(u, v_M≤N)` is stale
  by definition: it was computed before the change represented by v_N+1 was known.
- The client should not act on stale diagnostics. The proxy's role is to enforce
  this invariant when the server violates it (by pushing stale diagnostics due to
  async re-analysis).

## Success Criteria

### Criterion 1: Version-based stale filter (EXISTING — already met)
- When the proxy has tracked version N for URI u, and a `publishDiagnostics(u, v_M)` arrives with M < N, the notification is dropped.
- **Verification**: `test_stale_diagnostic_dropped_when_older` (passes)
- **Anchor**: LSP Spec § 3.7.4 + § 3.16.0 version invariant

### Criterion 2: System-reminder must not show stale diagnostics (NEW — the gap)
- After a `didChange(u, v_N+1)` has been processed by the proxy, the client must
  never display diagnostics computed against version ≤ v_N for URI u.
- **Anchor**: Design intent of the proxy.
- **Current status**: FAILS — system-reminder in this session showed
  `struct config::Config has no field named 'strict_mode'` at line 278 after the
  field had been removed and `cargo check --tests` confirmed 0 errors.

### Criterion 3: Fresh diagnostics must eventually arrive (NEW — no regressions)
- For every `didChange(u, v_N)`, the client must eventually receive a
  `publishDiagnostics(u, v_N)` (or higher) reflecting the new file state.
- **Anchor**: LSP Spec § 3.16.0 — servers are expected to publish diagnostics after
  processing changes.
- **Current status**: PASSES — rust-analyzer does push fresh diagnostics after
  re-analysis. The issue is the TIMING window during which the client shows stale
  cached state.

### Criterion 4: Client receives empty diagnostics when file becomes clean (NEW — no regressions)
- If a `didChange` fixes all errors, the corresponding `publishDiagnostics` with
  0 diagnostics must be forwarded to the client.
- **Anchor**: LSP Spec § 3.16.0 — the server sends 0-diagnostic notifications to
  clear prior state.
- **Current status**: PASSES — the proxy forwards empty diagnostics (version matches).
