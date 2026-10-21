{
  description = "Code review commenting tool for Jujutsu (jj)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "commenter";
          version = "0.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          postInstall = ''
            install -d $out/share/commenter/skills/commenter
            install -m 644 skills/commenter/SKILL.md $out/share/commenter/skills/commenter/SKILL.md
            install -m 644 skills/commenter.md $out/share/commenter/skills/commenter.md
          '';
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
          ];
        };
      }
    ) // {
      homeModules.default = { config, lib, pkgs, ... }: {
        home.packages = [ self.packages.${pkgs.system}.default ];
        skills = lib.mkIf (config ? skills) {
          commenter = "${self.packages.${pkgs.system}.default}/share/commenter/skills/commenter";
        };
      };
    };
}
