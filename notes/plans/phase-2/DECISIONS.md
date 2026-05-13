# Phase 2: Decisions & Success Criteria

**Date:** 2026-05-13
**Grill:** drive-outcomes step 3

---

## CI Pipeline

| Decision | Value |
|----------|-------|
| Trigger | `push` (main) + `pull_request` (all branches) |
| Clippy severity | Warn only (not a hard gate) |
| Caching | `Swatinem/rust-cache@v2` — auto-invalidated on `Cargo.lock` changes |
| Platforms | `ubuntu-latest`, `macos-latest` |

**Success criteria:**
- CI workflow runs and passes on push to main and on PRs
- `cargo test --workspace` passes on both platforms
- `cargo clippy` runs and warns (doesn't fail) on both platforms
- `Swatinem/rust-cache` restores before build, saves after build
- Cache is invalidated when `Cargo.lock` changes

---

## Binary Releases

| Decision | Value |
|----------|-------|
| Tag format | `v*` (e.g., `v0.2.0`) |
| Archive contents | Binary only (single `wait-for-lsp` executable per archive) |
| Targets | `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu` |
| Upload action | `softprops/action-gh-release@v2` |
| Archive format | `wait-for-lsp-{version}-{target}.tar.gz` |

**Success criteria:**
- Tag push `v*` triggers release workflow
- Workflow builds 3 targets, produces 3 `.tar.gz` archives
- Each archive is a release asset on the GitHub Release page
- Extracted binary runs and `wait-for-lsp --help` exits 0

---

## Unified Plugin

| Decision | Value |
|----------|-------|
| File path | `.claude-plugin/plugin.json` (repo root) |
| Old plugin | Deleted (`plugins/wait-for-lsp-rust/` removed) |
| LSP servers | `rust-analyzer`, `pyright`, `ty` (server), `fortls` |
| Plugin name | `wait-for-lsp` |
| Version | `0.2.0` |

**Success criteria:**
- `.claude-plugin/plugin.json` exists at repo root
- Contains `lspServers` entries: rust-analyzer, pyright, ty, fortls
- Each entry has correct `command: "wait-for-lsp"`, `args: [<lsp-name>, ...]`, and `extensionToLanguage`
- Old `plugins/wait-for-lsp-rust/` directory deleted
- Marketplace registration added to `my-claude-marketplace/.claude-plugin/marketplace.json`

---

## Nix Flake

| Decision | Value |
|----------|-------|
| Package scope | `wait-for-lsp` package + `overlays.default` only |
| Deferred | NixOS module, home-manager module, devshell |
| Build strategy | Fetch pre-built binary from GitHub Releases |
| Hash management | Pre-computed in release workflow (update `flake.nix` after tag) |
| Platforms | `x86_64-linux`, `aarch64-linux`, `x86_64-darwin`, `aarch64-darwin` |

**Success criteria:**
- `flake.nix` at repo root installable via `nix profile install github:TonyWu20/wait-for-lsp`
- `nix build .#wait-for-lsp` produces a working binary (downloads from releases)
- `overlays.default` usable from other flakes
- Flake lockfile present

---

## Cross-LSP Integration Tests

| Decision | Value |
|----------|-------|
| Python fixture | File with intentional type errors (missing imports, unknown modules) |
| Fortran fixture | Unused variable, undeclared variable, undefined subroutine |
| Capture format | Raw recorded LSP traffic (full JSON-RPC, Content-Length framing) |
| Capture method | Script that runs LSP server, sends known message sequence, records stdout |

**Success criteria:**
- Fixture file for pyright: recorded LSP session with a `.py` file containing missing-import errors
- Fixture file for fortls: recorded LSP session with a `.f90` file containing 3 intentional errors
- Integration tests read fixtures, replay through proxy, assert:
  - Non-diagnostic messages (init, hover, etc.) pass through unchanged
  - `publishDiagnostics` notifications are filtered according to proxy rules
  - Under `STAY_FRESH_STALE_FILTER=true`, stale diagnostics (wrong version) are dropped
- Tests use `speculative_v` (the fixture-based approach from existing tests)

---

## Marketplace Registration

| Decision | Value |
|----------|-------|
| Ref | `main` |
| Entry location | `my-claude-marketplace/.claude-plugin/marketplace.json` |

**Success criteria:**
- `wait-for-lsp` entry added to marketplace.json
- Source points to `TonyWu20/wait-for-lsp`, ref `main`
- Users can install via their marketplace
