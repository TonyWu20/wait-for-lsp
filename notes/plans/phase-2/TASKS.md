# TASKS.md — Phase 2: Distribution Infrastructure, Multi-Language Plugin, and Quality Gates

**Date:** 2026-05-13
**Phase slug:** phase-2
**ODD reference:** `/Users/tony/programming/rust-development-pipeline/skills/drive-outcomes/references/odd-pattern.md`

---

## Declared Fixtures

| Path | Description | Source |
|------|-------------|--------|
| `tests/fixtures/pyright-session.bin` | Raw captured LSP traffic from pyright-langserver against `main.py` with intentional type errors (missing import, type mismatch, wrong arg type). Contains 13 messages: 11 log/responses + 1 publishDiagnostics (3 diags, all sev 1) + 1 shutdown. | Captured via `capture_pyright_traffic.py` |
| `tests/fixtures/fortls-session.bin` | Raw captured LSP traffic from fortls against `main.f90` with intentional errors. Contains 3 messages: initialize response + publishDiagnostics (0 diags, empty array) + shutdown. fortls is a completion/navigation server — it does not emit semantic diagnostics. | Captured via `capture_fortls_traffic.py` |
| `tests/fixtures/lsp-workspace/src/main.py` | Python file with intentional type errors (type assignability, undefined module, arg type mismatch) | Created for fixture capture |
| `tests/fixtures/lsp-workspace/src/main.f90` | Fortran file with undeclared variable and undefined subroutine | Created for fixture capture |
| `tests/fixtures/rust-analyzer-session.bin` | Existing rust-analyzer fixture (7 messages, 4 publishDiagnostics with varying diag counts) | Pre-existing |

---

## Group A: CI & Release Infrastructure

### TASK-A1: GitHub Actions CI pipeline
**Kind:** direct
**Goal:** Automated build, test, and lint on every push to main and on PRs.

**Success Criteria:**
- `.github/workflows/ci.yml` exists at repo root
- Triggers on `push` (main) and `pull_request` (all branches)
- Matrix: `os: [ubuntu-latest, macos-latest]`
- Steps: `cargo test --workspace`, `cargo clippy` (warn only, not `-D warnings`)
- Uses `Swatinem/rust-cache@v2` for dependency caching
- Both OS matrix entries pass

**Test code:** N/A — verified by GitHub Actions run status

**Exploration notes:** Clippy is warn-only (user preference). Cache auto-invalidated on Cargo.lock changes.

---

### TASK-A2: GitHub Actions release workflow
**Kind:** direct
**Goal:** On tag push `v*`, build binaries for all targets and upload to GitHub Releases.

**Success Criteria:**
- `.github/workflows/release.yml` exists
- Trigger: `push: tags: ["v*"]`
- Builds `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`
- On macOS runner: build for both `aarch64-apple-darwin` (native) and cross-compile `x86_64-apple-darwin`
- On Linux runner: build for `x86_64-unknown-linux-gnu`
- Archives: `wait-for-lsp-{version}-{target}.tar.gz` containing only the binary
- Uploads to the auto-created GitHub Release via `softprops/action-gh-release@v2`

**Test code:** N/A — verified by creating a `v0.2.0-alpha.1` tag (dry run)

**Dependencies:** TASK-A1 (CI must exist first, or they can be built together)

---

## Group B: Plugin & Marketplace

### TASK-B1: Unified plugin.json
**Kind:** direct
**Goal:** Create `.claude-plugin/plugin.json` at repo root with all supported LSP servers.

**Success Criteria:**
- `.claude-plugin/plugin.json` exists at repo root
- Contains `lspServers` with these entries:
  - `rust-analyzer`: `args: ["rust-analyzer"]`, extension `.rs` → `rust`
  - `pyright`: `args: ["pyright-langserver", "--stdio"]`, extension `.py` → `python`
  - `ty`: `args: ["ty", "server"]`, extension `.py` → `python`
  - `fortls`: `args: ["fortls"]`, extension `.f90` → `fortran` (also `.f`, `.f95`, `.f03`, `.f08`)
- Plugin name: `wait-for-lsp`, version: `0.2.0`
- Old `plugins/wait-for-lsp-rust/` directory deleted

**Test code:** `validate_plugin_json.py` or manual `cargo check` that JSON is valid

**Exploration notes:**
- `ty server` uses stdio by default (confirmed from zed config example)
- `pyright-langserver --stdio` confirmed working — capture succeeded
- fortls sends `Content-Type` header alongside `Content-Length` — parser already handles multi-headers

---

### TASK-B2: Marketplace registration
**Kind:** direct
**Goal:** Register `wait-for-lsp` plugin in `my-claude-marketplace` so users can install it.

**Success Criteria:**
- Entry added to `my-claude-marketplace/.claude-plugin/marketplace.json`
- Fields: `name: "wait-for-lsp"`, `source: { source: "github", repo: "TonyWu20/wait-for-lsp", ref: "main" }`
- Description and tags appropriate to the plugin
- Entry placed in second position (after rust-development-pipeline) following existing convention

**Test code:** N/A — verify marketplace.json is valid JSON

**Dependencies:** TASK-B1 (plugin.json must exist in the target repo)

---

## Group C: Nix Flake

### TASK-C1: flake.nix with overlay
**Kind:** direct
**Goal:** Provide Nix flake that fetches pre-built `wait-for-lsp` binary from GitHub Releases.

**Success Criteria:**
- `flake.nix` at repo root
- Supports systems: `x86_64-linux`, `aarch64-linux`, `x86_64-darwin`, `aarch64-darwin`
- Package `wait-for-lsp` downloads the pre-built binary from GitHub Releases using `fetchurl`
- `overlays.default` provides `wait-for-lsp` package for use by other flakes
- `nix profile install github:TonyWu20/wait-for-lsp` produces working binary
- `nix build .#wait-for-lsp` succeeds (requires a release to exist)
- `flake.lock` checked in

**Hash management:** SHA256 pre-computed by release workflow and committed to `flake.nix`. For initial development, use `lib.fakeSha256` and update after first real release.

**Test code:** `nix build .#wait-for-lsp` (after a release exists)

---

## Group D: Cross-LSP Integration Tests

### TASK-D1: Pyright fixture-anchored integration tests
**Kind:** lib-tdd
**Goal:** Test the proxy's message parser and diagnostic filter against real pyright LSP traffic.

**Success Criteria:**
**Parser test** (`tests/cross_lsp_test.rs`):
- `test_pyright_fixture_parses_all_messages()` — feed `pyright-session.bin` to `MessageParser`, verify 13 messages parsed
- `test_pyright_fixture_content_lengths()` — verify buffer is empty after parsing all 13 messages (all bytes consumed)
- `test_pyright_fixture_message_types()` — verify message types: message 12 is `textDocument/publishDiagnostics` with 3 diagnostics

**Filter test:**
- `test_pyright_drop_all()` — with `drop_diagnostics=true`, only non-diag messages survive (messages 1-11, 13 → 12 expected)
- `test_pyright_severity_filter()` — with `drop_diagnostics=false, min_severity=3`, the 3 diags in message 12 (all sev 1) should be dropped (severity 1 > 3 means they're errors, actually 1 is the LOWEST numeric severity — `sev <= config.min_severity` means KEEP. So `min_severity=1` keeps all 3, `min_severity=4` keeps all 3, `min_severity=0` drops all.)
  - Correction: pyright diagnostics are all severity 1. `sev <= min_severity` means keep if severity is at or below threshold. So `min_severity >= 1` keeps them, `min_severity = 0` drops all.
- `test_pyright_version_stale_filter()` — with `stale_filter_enabled=true`, verify version tracking works against pyright's `"version": 1` in publishDiagnostics

**Test file:** `tests/cross_lsp_test.rs` (new)

**Anchoring:** All tests read `tests/fixtures/pyright-session.bin` — real captured LSP traffic, not synthetic data.

**Exploration findings:**
- Pyright sends `window/logMessage` notifications before the initialize response (messages 1-2 are log messages, message 3 is initialize response)
- Pyright includes `"version": 1` in publishDiagnostics params — proxy's version tracking applies
- All 3 diagnostics are severity 1 (Error) — severity filtering won't distinguish them
- Fixture has 13 messages total (excluding exit notification which has no response)

---

### TASK-D2: Fortls fixture-anchored integration tests
**Kind:** lib-tdd
**Goal:** Test the proxy's message parser and diagnostic filter against real fortls LSP traffic.

**Success Criteria:**
**Parser test:**
- `test_fortls_fixture_parses_all_messages()` — feed `fortls-session.bin` to `MessageParser`, verify 3 messages parsed
- `test_fortls_fixture_content_lengths()` — verify buffer empty after parsing
- `test_fortls_fixture_header_content_type()` — verify that the `Content-Type` header in fortls output is correctly handled by the parser (already confirmed parser handles multi-headers, but verify explicitly)
- `test_fortls_fixture_message_types()` — message 2 is `publishDiagnostics` with empty diagnostics array

**Filter test:**
- `test_fortls_drop_all()` — with `drop_diagnostics=true`, message 2 (publishDiagnostics) dropped, messages 1 and 3 survive
- `test_fortls_empty_diag_passes()` — with `drop_diagnostics=false`, the empty publishDiagnostics passes through unchanged

**Test file:** `tests/cross_lsp_test.rs` (same file as TASK-D1)

**Anchoring:** All tests read `tests/fixtures/fortls-session.bin` — real captured LSP traffic from fortls 3.2.2.

**Exploration findings:**
- **Important: fortls does NOT produce semantic diagnostics.** It sends `publishDiagnostics` with an empty `diagnostics: []` array. This is by design — fortls is a completion/navigation server. The test validates correct handling of empty-diagnostic messages from a real LSP server.
- fortls includes `Content-Type` header (`application/vscode-jsonrpc; charset=utf-8`) alongside `Content-Length`. The existing `MessageParser::parse_content_length()` correctly handles multi-header messages (splits by `\r\n`, iterates all lines, finds the `content-length:` line).
- 3 messages total: initialize → capabilities, publishDiagnostics (empty), shutdown → null

---

## Group Dependency Graph

```
TASK-A1 (CI) ──→ TASK-A2 (Releases)
                      │
                      ├──→ TASK-C1 (Nix flake — needs release binary URL)
                      │
TASK-B1 (Plugin) ──→ TASK-B2 (Marketplace)
                      │
TASK-D1 (pyright tests)  (no deps — fixture already captured)
TASK-D2 (fortls tests)   (no deps — fixture already captured)
```

TASK-A1 and TASK-A2 can be built together (same directory, related YAML). TASK-B1 and TASK-D1/D2 are independent of CI/releases. TASK-C1 depends on at least one real release existing.

---

## Implementation Order

1. **TASK-D1 + TASK-D2** — Cross-LSP tests first (no dependencies, fixtures already captured, tests are pure Rust)
2. **TASK-B1 + TASK-B2** — Plugin + marketplace (simple JSON files, can do alongside CI)
3. **TASK-A1 + TASK-A2** — CI + release workflow (tested by pushing to GitHub)
4. **TASK-C1** — Nix flake (depends on a real release existing for verification)

---

## Resume: phase-2
**Tasks done**: TASK-D1, TASK-D2, TASK-B1, TASK-B2, TASK-A1, TASK-A2
**Next task**: TASK-C1 (Nix flake)
**Status**: in-progress
