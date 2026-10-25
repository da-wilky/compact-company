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
    in
    {
      packages = forAll (pkgs: {
        default = pkgs.buildGoModule {
          pname = "compact-company";
          version = "0.1.0";
          src = ./.;
          vendorHash = "sha256-khaGQDBUQUDtCnPp7Y4A0ARFjXzpRnnGzRbjQY8q/GU=";
          meta = {
            license = pkgs.lib.licenses.agpl3Only;
            mainProgram = "compact-company";
          };
        };
      });

      apps = forAll (pkgs: {
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
      });

      checks = forAll (pkgs: {
        tests = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        lint = self.packages.${pkgs.stdenv.hostPlatform.system}.default.overrideAttrs (old: {
          pname = "compact-company-lint";
          nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.golangci-lint ];
          buildPhase = ''
            runHook preBuild
            HOME=$TMPDIR GOLANGCI_LINT_CACHE=$TMPDIR/golangci-lint golangci-lint run ./...
            runHook postBuild
          '';
          doCheck = false;
          installPhase = "touch $out";
        });
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.delve
            pkgs.go
            pkgs.gopls
            pkgs.golangci-lint
            pkgs.sqlite
          ];
        };
      });
    };
}
