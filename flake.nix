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
      in rec {
        packages.commenter = pkgs.rustPlatform.buildRustPackage {
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

        packages.difit = pkgs.buildNpmPackage rec {
          pname = "difit";
          version = "5.0.12";
          src = ./frontend;
          packageJSON = ./frontend/package.json;
          npmDepsHash = "sha256-eL/UxLcu2cqfFuTaGQkzBG1nEALCEBopIwXn7bl1Nyw=";
          npmFlags = [ "--legacy-peer-deps" ];
          postInstall = ''
            mkdir -p $out/share/difit
            cp -r skills $out/share/difit/
          '';
        };

        packages.default = packages.commenter;

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            nodejs
          ];
        };
      }
    ) // {
      homeModules.default = { config, lib, pkgs, ... }: {
        home.packages = [
          self.packages.${pkgs.system}.commenter
          self.packages.${pkgs.system}.difit
        ];
        skills = lib.mkIf (config ? skills) {
          commenter = "${self.packages.${pkgs.system}.commenter}/share/commenter/skills/commenter";
          difit = "${self.packages.${pkgs.system}.difit}/share/difit/skills/difit";
          difit-review = "${self.packages.${pkgs.system}.difit}/share/difit/skills/difit-review";
        };
      };
    };
}
