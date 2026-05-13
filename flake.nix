{
  description = "LSP proxy that filters stale diagnostics between Claude Code and any LSP server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { nixpkgs, ... }:
    let
      # Map Nix system names to GitHub Release tarball target names
      systemToTarget = {
        x86_64-linux = "x86_64-unknown-linux-gnu";
        aarch64-linux = "aarch64-unknown-linux-gnu";
        x86_64-darwin = "x86_64-apple-darwin";
        aarch64-darwin = "aarch64-apple-darwin";
      };

      eachSystem = nixpkgs.lib.genAttrs (builtins.attrNames systemToTarget);

      # Build the URL for a given version and system.
      # Example: https://github.com/TonyWu20/wait-for-lsp/releases/download/v0.2.0/wait-for-lsp-v0.2.0-x86_64-unknown-linux-gnu.tar.gz
      releaseUrl = version: system:
        "https://github.com/TonyWu20/wait-for-lsp/releases/download/${version}/wait-for-lsp-${version}-${systemToTarget.${system}}.tar.gz";

      mkPackage =
        {
          stdenv,
          fetchurl,
          system,
        }:
        let
          # bump this on each release
          version = "0.2.0";
          tag = "v${version}";

          # ─── SHA256 hashes per system ───
          # Update these when creating a new release:
          #   1. Download the tarball from GitHub Releases
          #   2. Run: nix hash file wait-for-lsp-<version>-<target>.tar.gz
          #   3. Replace the fakeSha256 below with the real hash
          hashes = {
            x86_64-linux = nixpkgs.lib.fakeSha256;
            aarch64-linux = nixpkgs.lib.fakeSha256;
            x86_64-darwin = nixpkgs.lib.fakeSha256;
            aarch64-darwin = nixpkgs.lib.fakeSha256;
          };
        in
        stdenv.mkDerivation {
          pname = "wait-for-lsp";
          inherit version;

          src = fetchurl {
            url = releaseUrl tag system;
            sha256 = hashes.${system};
          };

          sourceRoot = ".";

          installPhase = ''
            mkdir -p $out/bin
            install -m755 wait-for-lsp $out/bin/wait-for-lsp
          '';

          meta = {
            description = "LSP proxy that filters stale diagnostics between Claude Code and any LSP server";
            homepage = "https://github.com/TonyWu20/wait-for-lsp";
            license = nixpkgs.lib.licenses.mit;
            platforms = builtins.attrNames systemToTarget;
            mainProgram = "wait-for-lsp";
          };
        };
    in
    {
      packages = eachSystem (system: {
        default = nixpkgs.legacyPackages.${system}.callPackage mkPackage {
          inherit system;
        };
      });

      overlays = {
        default = final: prev: {
          wait-for-lsp = final.callPackage mkPackage {
            system = final.system;
          };
        };
      };
    };
}
