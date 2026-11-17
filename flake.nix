{
  description = "Code review diff viewer and commenting tool for Jujutsu (jj)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
      in rec {
        packages.tellit = pkgs.rustPlatform.buildRustPackage {
          pname = "tellit";
          version = "0.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          nativeBuildInputs = [
            pkgs.trunk
            pkgs.wasm-bindgen-cli
            pkgs.lld
          ];
          nativeCheckInputs = [
            pkgs.git
            pkgs.jujutsu
          ];
          preBuild = ''
            (
              cd crates/tellit-ui
              HOME="$TMPDIR" TRUNK_TOOLS_WASM_BINDGEN="${pkgs.wasm-bindgen-cli.version}" \
                trunk build --release --offline
            )
          '';
          preCheck = ''
            export HOME="$TMPDIR"
          '';
          cargoBuildFlags = [
            "-p"
            "tellit-cli"
          ];
          postInstall = ''
            install -Dm644 ${./skills/tellit/SKILL.md} $out/share/tellit/skills/tellit/SKILL.md
          '';
        };

        packages.default = packages.tellit;

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            trunk
            wasm-bindgen-cli
            lld
            dprint
          ];
        };
      }
    )
    // {
      homeModules.default = {
        config,
        lib,
        pkgs,
        ...
      }: {
        home.packages = [
          self.packages.${pkgs.system}.tellit
        ];
        skills = lib.mkIf (config ? skills) {
          tellit = "${self.packages.${pkgs.system}.tellit}/share/tellit/skills/tellit";
        };
      };
    };
}
