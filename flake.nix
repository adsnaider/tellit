{
  description = "Code review diff viewer and commenting tool for Jujutsu (jj)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        tellit-web = pkgs.buildNpmPackage rec {
          pname = "tellit-web";
          version = "0.1.0";
          src = ./frontend;
          packageJSON = ./frontend/package.json;
          npmDepsHash = "sha256-eL/UxLcu2cqfFuTaGQkzBG1nEALCEBopIwXn7bl1Nyw=";
          npmFlags = [ "--legacy-peer-deps" ];
        };

        tellit-bin = pkgs.rustPlatform.buildRustPackage {
          pname = "tellit-cli";
          version = "0.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
        };
      in rec {
        packages.tellit = pkgs.symlinkJoin {
          name = "tellit-0.1.0";
          paths = [ tellit-bin ];
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postBuild = ''
            mkdir -p $out/share/tellit/skills/tellit
            mkdir -p $out/share/tellit/web
            cp -r ${tellit-web}/lib/node_modules/difit/* $out/share/tellit/web/
            install -m 644 ${./skills/tellit/SKILL.md} $out/share/tellit/skills/tellit/SKILL.md
            install -m 644 ${./skills/tellit.md} $out/share/tellit/skills/tellit.md

            wrapProgram $out/bin/tellit \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.nodejs ]} \
              --set-default TELLIT_WEB_DIR $out/share/tellit/web
          '';
        };

        packages.default = packages.tellit;

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
          self.packages.${pkgs.system}.tellit
        ];
        skills = lib.mkIf (config ? skills) {
          tellit = "${self.packages.${pkgs.system}.tellit}/share/tellit/skills/tellit";
        };
      };
    };
}
