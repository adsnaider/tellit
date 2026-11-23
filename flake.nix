{
  description = "Code review diff viewer and commenting tool for Jujutsu (jj)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    crane,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
        inherit (pkgs) lib;
        craneLib = crane.mkLib pkgs;

        version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

        # Must match the `wasm-bindgen` pin in `Cargo.toml` / `Cargo.lock`.
        wasmBindgenCli = pkgs.wasm-bindgen-cli_0_2_129;

        # List `crates/tellit-ui` subpaths explicitly so local `dist/` and `.stage/`
        # directories never enter path-based evaluations.
        uiSrc = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.lock
            ./Cargo.toml
            ./crates/tellit-types/Cargo.toml
            ./crates/tellit-types/src
            ./crates/tellit-ui/Cargo.toml
            ./crates/tellit-ui/index.html
            ./crates/tellit-ui/styles.css
            ./crates/tellit-ui/public
            ./crates/tellit-ui/src
          ];
        };

        cliSrc = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.lock
            ./Cargo.toml
            ./build.rs
            ./src
            ./crates/tellit-types/Cargo.toml
            ./crates/tellit-types/src
            ./crates/tellit-ui/Cargo.toml
          ];
        };

        testSrc = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.lock
            ./Cargo.toml
            ./build.rs
            ./src
            ./tests
            ./crates/tellit-types/Cargo.toml
            ./crates/tellit-types/src
            ./crates/tellit-ui/Cargo.toml
            ./crates/tellit-ui/src
          ];
        };

        # Share one vendored tree and dummy workspace across all Crane derivations.
        # `mkDummySrc` inspects `testSrc` at eval time for Cargo auto-target discovery
        # without depending on `.rs` file contents, so source edits keep dep caches warm.
        cargoVendorDir = craneLib.vendorCargoDeps {
          cargoLock = ./Cargo.lock;
        };

        dummySrc = craneLib.mkDummySrc {
          src = testSrc;
          cargoLock = ./Cargo.lock;
        };

        commonCargoArgs = {
          inherit version cargoVendorDir;
          cargoLock = ./Cargo.lock;
          strictDeps = true;
        };

        wasmCommonArgs =
          commonCargoArgs
          // {
            pname = "tellit-ui";
            CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
            cargoExtraArgs = "--locked -p tellit-ui";
            nativeBuildInputs = [
              pkgs.lld
            ];
          };

        tellit-ui-deps = craneLib.buildDepsOnly (
          wasmCommonArgs
          // {
            inherit dummySrc;
            cargoCheckCommand = "true";
            doCheck = false;
          }
        );

        tellit-ui = craneLib.buildTrunkPackage (
          wasmCommonArgs
          // {
            src = uiSrc;
            cargoArtifacts = tellit-ui-deps;
            wasm-bindgen-cli = wasmBindgenCli;
            trunkIndexPath = "index.html";
            # Use env vars instead of `--offline`/`--locked` CLI flags because trunk 0.21+
            # takes optional bool values on those flags and misparses a trailing index path.
            TRUNK_OFFLINE = "true";
            TRUNK_BUILD_LOCKED = "true";
            TRUNK_TOOLS_WASM_BINDGEN = wasmBindgenCli.version;
            # Cargo requires every workspace member to have at least one target file on disk.
            postPatch = ''
              mkdir -p src
              touch src/main.rs
            '';
            preBuild = ''
              export HOME="$TMPDIR"
              export CARGO_TARGET_DIR="$(realpath -m "''${CARGO_TARGET_DIR:-target}")"
              cd crates/tellit-ui
            '';
            postBuild = ''
              mv ./dist ../..
              cd ../..
            '';
          }
        );

        tellit-cli-deps = craneLib.buildDepsOnly (
          commonCargoArgs
          // {
            pname = "tellit-cli";
            inherit dummySrc;
            cargoExtraArgs = "--locked -p tellit-cli";
            cargoCheckCommand = "true";
            doCheck = false;
          }
        );

        tellit-cli = craneLib.buildPackage (
          commonCargoArgs
          // {
            pname = "tellit-cli";
            src = cliSrc;
            cargoArtifacts = tellit-cli-deps;
            cargoExtraArgs = "--locked -p tellit-cli";
            doCheck = false;
            # Cargo requires every workspace member to have at least one target file on disk.
            postPatch = ''
              mkdir -p crates/tellit-ui/src
              touch crates/tellit-ui/src/main.rs
            '';
          }
        );

        # Wrap the CLI binary with `TELLIT_WEB_DIR` pointing at `tellit-ui` so UI and
        # skill edits never rebuild `tellit-cli`.
        tellit =
          pkgs.runCommandLocal "tellit-${version}" {
            pname = "tellit";
            inherit version;
            nativeBuildInputs = [
              pkgs.makeBinaryWrapper
            ];
            meta.mainProgram = "tellit";
          } ''
            makeBinaryWrapper ${tellit-cli}/bin/tellit $out/bin/tellit \
              --inherit-argv0 \
              --set-default TELLIT_WEB_DIR ${tellit-ui}
            install -Dm644 ${./skills/tellit/SKILL.md} $out/share/tellit/skills/tellit/SKILL.md
          '';

        # Separate workspace + dev-dependency cache so `nix build` skips dev-dependencies
        # while `nix flake check` reuses cached test artifacts.
        tellit-test-deps = craneLib.buildDepsOnly (
          commonCargoArgs
          // {
            pname = "tellit-tests";
            inherit dummySrc;
            cargoExtraArgs = "--locked --workspace";
            cargoCheckCommand = "true";
          }
        );
      in {
        packages = {
          inherit tellit tellit-ui;
          default = tellit;
        };

        checks = {
          inherit tellit;
          tellit-tests = craneLib.cargoTest (
            commonCargoArgs
            // {
              pname = "tellit-tests";
              src = testSrc;
              cargoArtifacts = tellit-test-deps;
              cargoExtraArgs = "--locked --workspace";
              nativeBuildInputs = [
                pkgs.git
                pkgs.jujutsu
              ];
              preConfigure = ''
                export HOME="$TMPDIR"
              '';
            }
          );
        };

        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.rust-analyzer
            pkgs.clippy
            pkgs.trunk
            wasmBindgenCli
            pkgs.lld
            pkgs.dprint
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
      }: let
        tellitPkg = self.packages.${pkgs.stdenv.hostPlatform.system}.tellit;
      in {
        home.packages = [
          tellitPkg
        ];
        skills = lib.mkIf (config ? skills) {
          tellit = "${tellitPkg}/share/tellit/skills/tellit";
        };
      };
    };
}
