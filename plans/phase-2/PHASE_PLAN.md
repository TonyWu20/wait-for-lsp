# Phase 2: Distribution Infrastructure, Multi-Language Plugin, and Quality Gates

**Date:** 2026-05-13
**Status:** Draft

## Goals

1. **CI pipeline (GitHub Actions)** — Build, test, and clippy on every push and PR for macOS (arm64, x86_64) and Linux (x86_64). Without this, every code change is a blind merge — nothing catches regressions until the user happens to run tests locally.

2. **Pre-built binary releases** — GitHub Actions release workflow that builds binaries for macOS arm64, macOS x86_64, and Linux x86_64 on tag, then uploads to GitHub Releases. Today the only install path is `cargo install --path .` which requires a full Rust toolchain; pre-built binaries remove that barrier entirely.

3. **Unified plugin with LSP registration** — Consolidate the existing rust-analyzer-only plugin into a single `wait-for-lsp` plugin that registers all supported `lspServers`: rust-analyzer (Rust), pyright + `ty` (Python), fortls (Fortran). Register in the user's existing marketplace (`my-claude-marketplace`) so users install once and get coverage for all three languages.

4. **Nix flake** — `flake.nix` with overlay that fetches the pre-built release binary, plus `lib.wait-for-lsp` helper so Nix users can install declaratively. Nix/NixOS users (common in development tooling) get a single `nix profile install github:TonyWu20/wait-for-lsp` workflow.

5. **Cross-LSP integration tests** — Capture real LSP traffic from pyright and fortls, create fixture-anchored integration tests that verify the proxy handles real-world diagnostic patterns for each LSP. The current tests use a `cat` mock — they verify the proxy binary works, not that it works *with actual LSP servers*.

## Scope Boundaries

**In scope:**
- GitHub Actions CI YAML (build, test, clippy — both `ubuntu-latest` and `macos-latest`)
- GitHub Actions release workflow (tag-triggered, multi-arch, upload to Releases)
- Unified `plugins/wait-for-lsp/plugin.json` with multiple `lspServers` entries
- Marketplace registration in `my-claude-marketplace/.claude-plugin/marketplace.json`
- `flake.nix` that fetches pre-built binary from GitHub Releases
- Integration test fixtures captured from real pyright and fortls sessions
- New integration tests referencing captured fixtures

**Out of scope:**
- TypeScript/JavaScript LSP support (deferred to Phase 3)
- `wait-for-lsp setup` CLI subcommand (rendered unnecessary by marketplace distribution)
- Homebrew formula (user has no macOS distribution need beyond the pre-built binary)
- Windows cross-platform support
- Changes to the proxy binary's Rust source code (it is already language-agnostic)

## Design Notes

### CI design
- Use `taiki-e/install-action@v2` for cargo tools (nextest if desired, otherwise cargo test)
- `cargo clippy -- -D warnings` as a hard gate — warnings fail the build
- Run on `push` (main) and `pull_request` (all branches)
- Matrix: `os: [ubuntu-latest, macos-latest]`
- No nightly Rust needed — stable channel is sufficient

### Release workflow design
- Trigger on `push: tags: ["v*"]`
- Build matrix over `{ os: [ubuntu-latest, macos-latest], target: [x86_64-apple-darwin, aarch64-apple-darwin, x86_64-unknown-linux-gnu] }` — but macOS runner can only build macOS targets, Linux runner builds Linux targets. So the actual matrix pairs OS with compatible targets.
- On macOS, cross-compile for x86_64 (from arm64 runner) using `rustup target add x86_64-apple-darwin`
- Use `softprops/action-gh-release@v2` to upload artifacts
- Format: `wait-for-lsp-{version}-{target}.tar.gz` containing the binary + LICENSE + README

### Plugin consolidation approach
- Create `plugins/wait-for-lsp/plugin.json` — new unified plugin
- Keep `plugins/wait-for-lsp-rust/plugin.json` as-is for backward compatibility
- Unified plugin name: `wait-for-lsp`; version `0.2.0`
- Marketplace entry points to this repo, users install via marketplace

### Nix flake design
- `flake.nix` uses `fetchurl` + `install -m755` pattern (no build, just unpack)
- `systems: ["x86_64-linux", "aarch64-linux", "x86_64-darwin", "aarch64-darwin"]`
- `nixpkgs.overlays.default` provides `wait-for-lsp` package
- Hash management: either `sha256 = lib.fakeSha256;` with instructions, or pre-computed per-release
- No `rustPlatform.buildRustPackage` — user explicitly chose fetch-pre-built over build-from-source

### Cross-LSP test fixture capture
- Capture approach: run the LSP server with a known input script, capture stdin+stdout, extract the diagnostic payloads
- Fixture format: standalone `.json` files in `tests/fixtures/` matching the existing pattern
- At minimum: pyright diagnostics (with a Python file that has intentional type errors), fortls diagnostics (with a Fortran file that has intentional errors)
- The `ty` LSP may be newer — fixture for it can be added later once its diagnostics format is understood

## Deferred Items Absorbed

- **Plugin JSON files for Python and Fortran** (from original plan): Absorbed into the unified plugin approach. Instead of three separate plugin.json files, one unified file covers all languages.
- **Cross-LSP integration testing** (from original plan's scope carve-outs): Added as Goal 5.

## Domain Terms

**Unified Plugin**:
A single `plugin.json` whose `lspServers` map contains entries for multiple language servers across multiple languages — as opposed to per-language plugins. Users install once and get coverage for all supported languages.
*Resolved:* During goal discussion, the user revealed they maintain a personal Claude Code marketplace (`my-claude-marketplace`). This changed the distribution model: a unified plugin registered in the marketplace replaces both the `setup` subcommand and per-language plugin files.

**Pre-built binary**:
A compiled `wait-for-lsp` executable distributed via GitHub Releases, built by CI on a tag push. Distinguished from "source-built" (cargo install --path .) and "nixpkgs-built" (rustPlatform.buildRustPackage).
*Resolved:* The user chose fetch-pre-built over build-from-source for the Nix flake, simplifying the flake to a download + install pattern.

## Open Questions

1. **`ty` LSP CLI interface** — What is the exact command to invoke `ty` as an LSP server? The flake's plugin.json entry needs `command: "wait-for-lsp"` + `args: ["ty", ...]`. Need to verify whether `ty` ships an LSP mode and its invocation syntax.

2. **Marketplace plugin discovery path** — When the marketplace entry points to this GitHub repo, what path does Claude Code use to find the plugin.json? Does it look at root-level `plugin.json`, `.claude-plugin/`, or something else? This may require creating a `.claude-plugin` directory in this repo. (The user's other marketplace repos already have `.claude-plugin/` directories.)

3. **CI cache strategy** — Should we cache `~/.cargo` and `target/` between CI runs? The default `Swatinem/rust-cache@v2` approach speeds up builds significantly but occasionally causes stale artifact issues. Worth including with the standard caveats.
