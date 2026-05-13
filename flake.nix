{
  description = "LSP proxy that filters stale diagnostics between Claude Code and any LSP server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    { nixpkgs, crane, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      eachSystem = nixpkgs.lib.genAttrs systems;

      meta = {
        description = "LSP proxy that filters stale diagnostics between Claude Code and any LSP server";
        homepage = "https://github.com/TonyWu20/wait-for-lsp";
        license = nixpkgs.lib.licenses.mit;
        platforms = systems;
        mainProgram = "wait-for-lsp";
      };
    in
    {
      packages = eachSystem (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;
        in
        {
          default = craneLib.buildPackage {
            src = craneLib.cleanCargoSource ./.;
            doCheck = false;
            inherit meta;
          };
        }
      );

      overlays.default = final: prev: {
        wait-for-lsp =
          let
            craneLib = crane.mkLib final;
          in
          craneLib.buildPackage {
            src = craneLib.cleanCargoSource ./.;
            doCheck = false;
            inherit meta;
          };
      };
    };
}
