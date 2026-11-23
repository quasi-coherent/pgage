{
  description = "pgage";
  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    fenix.url = "github:nix-community/fenix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
    treefmt-nix.url = "github:numtide/treefmt-nix";
  };
  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.treefmt-nix.flakeModule
      ];

      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      perSystem =
        {
          inputs',
          lib,
          pkgs,
          self',
          system,
          ...
        }:
        let
          root = ./.;

          stable = inputs'.fenix.packages.stable;
          nightly = inputs'.fenix.packages.default;
          crane = (inputs.crane.mkLib pkgs).overrideToolchain stable.toolchain;

          cargoArtifacts = crane.buildDepsOnly {
            src = crane.cleanCargoSource root;
            strictDeps = true;
          };

          src = lib.fileset.toSource {
            inherit root;
            fileset = lib.fileset.unions [
              (crane.fileset.commonCargoSources root)
              (crane.fileset.commonCargoSources ./pgage-derive)
              (lib.fileset.maybeMissing ./tests/sql)
            ];
          };

          args = {
            inherit src;
            strictDeps = true;
          };
        in
        {
          devShells.default =
            let
              fmtt = pkgs.writeShellApplication {
                name = "fmtt";
                text = ''${lib.getExe self'.formatter} "$@"'';
              };
            in
            pkgs.mkShell {
              RUST_SRC_PATH = "${stable.toolchain}/lib/rustlib/src/rust/library";
              shellHook = ''
                export DATABASE_URL="postgresql://postgres:postgres@localhost:5432"
              '';
              packages = [
                fmtt
                pkgs.cargo-expand
                pkgs.cargo-shear
                pkgs.cargo-sort-derives
                pkgs.just
                pkgs.nix-direnv
                pkgs.postgresql_18
                self'.packages.pgagedb
                stable.toolchain
              ];
            };

          checks = {
            lint = crane.cargoClippy (
              args
              // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--workspace --all-features --keep-going -- -Dwarnings";
              }
            );

            test = crane.cargoNextest (
              args
              // {
                inherit cargoArtifacts;
                partitions = 1;
                partitionType = "count";
                cargoNextestPartitionsExtraArgs = "--no-tests=pass";
              }
            );

            docs = crane.cargoDoc (
              args
              // {
                inherit cargoArtifacts;
              }
            );
          };

          packages =
            lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
              pgage = pkgs.callPackage ./image.nix { postgresql = pkgs.postgresql_18; };
            }
            // {
              pgagedb =
                let
                  linuxSystem = builtins.replaceStrings [ "darwin" ] [ "linux" ] system;
                in
                pkgs.writeShellApplication {
                  name = "pgagedb";
                  runtimeInputs = with pkgs; [
                    docker-client
                    git
                    gnutar
                    jq
                  ];
                  text = ''
                    prj="$(git rev-parse --show-toplevel)"
                    flake="git+file:$prj"
                    name="pgage"

                    case "''${1:-}" in
                      up|u)
                        nix build "$flake#packages.${linuxSystem}.$name" --refresh --out-link "result-$name"
                        docker image load -i "$prj/result-$name"
                        img="$(tar -Oxf "./result-$name" manifest.json | jq -r ".[0].RepoTags[0]")"
                        docker tag "$img" "$name"
                        docker run -d \
                          --name "$name" \
                          -p "''${PGAGE_PORT:-5432}:5432" \
                          -e POSTGRES_PASSWORD="''${PGAGE_PASSWORD:-postgres}" \
                          "$name" >/dev/null
                        ;;
                      down|d)
                        docker rm -f -v "$name" >/dev/null
                        ;;
                      *)
                        echo "usage: pgagedb up|u|down|d" >&2
                        exit 1
                        ;;
                    esac
                  '';
                };
            };

          treefmt = {
            projectRootFile = ".envrc";
            programs = {
              nixfmt.enable = true;
              rustfmt = {
                enable = true;
                package = nightly.toolchain;
              };
              taplo.enable = true;
              typos.enable = true;
            };
            settings = {
              global.excludes = [ ".direnv/*" ];
              formatter = {
                "sort-derive" = {
                  command = "${pkgs.bash}/bin/bash";
                  options = [
                    "-euc"
                    ''
                      for f in "$@"; do
                        ${lib.getBin pkgs.cargo-sort-derives}/bin/cargo-sort-derives sort-derives --color never --path $f
                      done
                    ''
                    "--" # bash swallows the second argument when using -c
                  ];
                  includes = [ "*.rs" ];
                };
              };
            };
          };
        };
    };
}
