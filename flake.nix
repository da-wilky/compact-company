{
  description = "compact-company: customers, time tracking and invoices for small businesses";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      lint = ''
        [ -f go.mod ] || { echo "run from the repository root" >&2; exit 1; }
        golangci-lint fmt --diff ./...
        golangci-lint run ./...
        sh scripts/markup_assertions.sh
        sh scripts/table_wrap.sh
        sh scripts/css_rules.sh
      '';
    in
    {
      packages = forAll (pkgs: {
        default = pkgs.buildGoModule {
          pname = "compact-company";
          version = "0.1.0";
          src = ./.;
          vendorHash = "sha256-khaGQDBUQUDtCnPp7Y4A0ARFjXzpRnnGzRbjQY8q/GU=";
          nativeBuildInputs = [ pkgs.makeWrapper ];
          nativeCheckInputs = [ pkgs.typst ];
          postInstall = ''
            wrapProgram $out/bin/compact-company --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.typst ]}
          '';
          meta = {
            license = pkgs.lib.licenses.agpl3Only;
            mainProgram = "compact-company";
          };
        };
      });

      apps = forAll (
        pkgs:
        pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          ui-check = {
            type = "app";
            program = pkgs.lib.getExe (
              pkgs.writeShellApplication {
                name = "ui-check";
                runtimeInputs = [
                  pkgs.go
                  pkgs.curl
                  pkgs.git
                  pkgs.typst
                  (pkgs.python3.withPackages (p: [ p.playwright ]))
                ];
                text = ''
                  [ -f go.mod ] || { echo "run from the repository root" >&2; exit 1; }
                  # A host LD_LIBRARY_PATH pointing at newer libraries breaks Chromium's glibc.
                  unset LD_LIBRARY_PATH
                  # A free port by default, so parallel runs don't collide.
                  port=''${UI_CHECK_PORT:-$(python3 -I -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')}
                  work=$(mktemp -d)
                  mkdir "$work/shots"
                  printf 'listen = "127.0.0.1:%s"\ndata_dir = "%s/data"\n\n[auth]\nmode = "none"\n' "$port" "$work" > "$work/config.toml"
                  CGO_ENABLED=0 go build -o "$work/compact-company" ./cmd/compact-company
                  "$work/compact-company" serve --config "$work/config.toml" > "$work/server.log" 2>&1 &
                  server=$!
                  trap 'kill $server' EXIT
                  for _ in $(seq 50); do curl -s -o /dev/null "http://127.0.0.1:$port/" && break; sleep 0.2; done
                  python3 -I scripts/ui_check.py "http://127.0.0.1:$port" "$work/data/app.db" "$work/shots" ${pkgs.lib.getExe pkgs.chromium} "$@"
                '';
              }
            );
          };
        }
        // {
          check = {
            type = "app";
            program = pkgs.lib.getExe (
              pkgs.writeShellApplication {
                name = "check";
                runtimeInputs = [
                  pkgs.go
                  pkgs.golangci-lint
                  pkgs.typst
                ];
                text = lint + "go test ./...\n";
              }
            );
          };
          # The pre-commit hook: everything in check but the tests.
          lint = {
            type = "app";
            program = pkgs.lib.getExe (
              pkgs.writeShellApplication {
                name = "lint";
                runtimeInputs = [
                  pkgs.go
                  pkgs.golangci-lint
                ];
                text = lint;
              }
            );
          };
          default = {
            type = "app";
            program = toString (
              pkgs.writeShellScript "compact-company-dev" ''
                if [ $# -eq 0 ]; then
                  set -- serve --config ${./config.dev.toml}
                fi
                exec ${pkgs.lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.default} "$@"
              ''
            );
          };
        }
      );

      checks = forAll (pkgs: {
        tests = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        lint = self.packages.${pkgs.stdenv.hostPlatform.system}.default.overrideAttrs (old: {
          pname = "compact-company-lint";
          nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.golangci-lint ];
          buildPhase = ''
            runHook preBuild
            HOME=$TMPDIR GOLANGCI_LINT_CACHE=$TMPDIR/golangci-lint golangci-lint run ./...
            HOME=$TMPDIR GOLANGCI_LINT_CACHE=$TMPDIR/golangci-lint golangci-lint fmt --diff ./...
            runHook postBuild
          '';
          doCheck = false;
          installPhase = "touch $out";
        });
        markup-assertions = pkgs.runCommand "compact-company-markup-assertions" { } ''
          cd ${./.}
          sh scripts/markup_assertions.sh
          touch $out
        '';
        table-wrap = pkgs.runCommand "compact-company-table-wrap" { } ''
          cd ${./.}
          sh scripts/table_wrap.sh
          touch $out
        '';
        css-rules = pkgs.runCommand "compact-company-css-rules" { } ''
          cd ${./.}
          sh scripts/css_rules.sh
          touch $out
        '';
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.delve
            pkgs.go
            pkgs.gopls
            pkgs.golangci-lint
            pkgs.sqlite
            pkgs.typst
          ];
          # Debug builds compile cgo at -O0, which Nix's _FORTIFY_SOURCE hardening turns into an error; nothing here needs cgo.
          CGO_ENABLED = "0";
          shellHook = "git config core.hooksPath .githooks";
        };
      });
    };
}
