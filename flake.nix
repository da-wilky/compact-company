{
  description = "compact-company: a business suite for one German business";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      build =
        pkgs:
        let
          craneLib = crane.mkLib pkgs;
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              (craneLib.fileset.commonCargoSources ./.)
              ./crates/server/migrations
              ./crates/server/typst
              ./crates/server/tests/fixtures
              ./web/openapi.json
              ./web/i18n
              ./GLOSSARY.md
            ];
          };
          pname = "compact-company";
          version = "0.1.0";
          web = pkgs.buildNpmPackage {
            pname = "compact-company-web";
            version = "0.1.0";
            src = ./web;
            npmDepsHash = "sha256-AsAlfrIffiIRWO9k++Aw2OI2kXVEx1YL9pQlUv+7q58=";
            installPhase = "cp -r build $out";
          };
          cargoArtifacts = craneLib.buildDepsOnly {
            inherit src pname version;
            strictDeps = true;
          };
          caBundle = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
          withClient = {
            inherit
              src
              cargoArtifacts
              pname
              version
              ;
            strictDeps = true;
            # rust-embed reads the client from web/build at compile time
            preBuild = "mkdir -p web && cp -r ${web} web/build";
          };
        in
        {
          inherit
            craneLib
            src
            web
            caBundle
            ;
          server = craneLib.buildPackage (
            withClient
            // {
              # the S3 client refuses to start without CA certificates, even for plain HTTP
              SSL_CERT_FILE = caBundle;
              meta.mainProgram = "compact-company";
            }
          );
          clippy = craneLib.cargoClippy (
            withClient // { cargoClippyExtraArgs = "--all-targets -- --deny warnings"; }
          );
          svelte-check = web.overrideAttrs {
            pname = "compact-company-web-check";
            npmBuildScript = "check";
            installPhase = "touch $out";
          };
        };
    in
    {
      packages = forAll (
        pkgs:
        let
          b = build pkgs;
        in
        {
          default = b.server;
          web = b.web;
          image = pkgs.dockerTools.buildLayeredImage {
            name = "compact-company";
            tag = "latest";
            config = {
              Entrypoint = [ (pkgs.lib.getExe b.server) ];
              Cmd = [
                "serve"
                "--config"
                "${pkgs.writeText "config.toml" ''
                  listen = "0.0.0.0:9440"
                  data_dir = "/data"
                ''}"
              ];
              Env = [ "SSL_CERT_FILE=${b.caBundle}" ];
              ExposedPorts."9440/tcp" = { };
              Volumes."/data" = { };
            };
          };
        }
      );

      apps = forAll (pkgs: {
        ui-check = {
          type = "app";
          program = pkgs.lib.getExe (
            pkgs.writeShellApplication {
              name = "ui-check";
              runtimeInputs = [ pkgs.nodejs ];
              text = ''
                # a host LD_LIBRARY_PATH pointing at other libraries breaks Chromium
                unset LD_LIBRARY_PATH
                exec node ${./scripts/ui-check.mjs} \
                  ${pkgs.lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.default} \
                  ${pkgs.lib.getExe pkgs.chromium} "$@"
              '';
            }
          );
        };
        default = {
          type = "app";
          program = toString (
            pkgs.writeShellScript "compact-company-serve" ''
              exec ${pkgs.lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.default} serve "$@"
            ''
          );
        };
      });

      checks = forAll (
        pkgs:
        let
          b = build pkgs;
        in
        {
          inherit (b) server clippy svelte-check;
          fmt = b.craneLib.cargoFmt { inherit (b) src; };
          kosit = import ./nix/kosit.nix {
            inherit pkgs;
            inherit (b) server;
          };
        }
        # the VM test needs KVM, which GitHub's arm64 runners lack
        // pkgs.lib.optionalAttrs (pkgs.stdenv.hostPlatform.system == "x86_64-linux") {
          vm = import ./nix/vm-test.nix {
            inherit pkgs;
            module = self.nixosModules.default;
          };
        }
      );

      nixosModules.default = import ./nix/module.nix self;

      devShells = forAll (pkgs: {
        default = (build pkgs).craneLib.devShell {
          packages = [
            pkgs.nodejs
            pkgs.rust-analyzer
            pkgs.clippy
            pkgs.rustfmt
          ];
        };
      });

      formatter = forAll (pkgs: pkgs.nixfmt);
    };
}
