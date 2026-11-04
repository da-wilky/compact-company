self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.compact-company;
  toml = pkgs.formats.toml { };
in
{
  options.services.compact-company = {
    enable = lib.mkEnableOption "compact-company";
    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "compact-company.packages.\${pkgs.stdenv.hostPlatform.system}.default";
    };
    settings = lib.mkOption {
      type = toml.type;
      default = { };
      description = "The `config.toml` contents; see the README for the keys. `data_dir` is the service's state directory.";
      example = lib.literalExpression ''
        {
          listen = "127.0.0.1:9440";
          public_url = "https://compact.example";
          auth = {
            mode = "oidc";
            issuer = "https://id.example/realms/main";
            client_id = "compact-company";
          };
        }
      '';
    };
    clientSecretFile = lib.mkOption {
      type = lib.types.nullOr (
        lib.types.pathWith {
          inStore = false;
          absolute = true;
        }
      );
      default = null;
      description = "OIDC client secret, read as root and handed to the service as a systemd credential; sets `auth.client_secret_file`.";
    };
  };

  config = lib.mkIf cfg.enable {
    services.compact-company.settings = {
      data_dir = "/var/lib/compact-company";
      auth.client_secret_file = lib.mkIf (
        cfg.clientSecretFile != null
      ) "/run/credentials/compact-company.service/client-secret";
    };

    systemd.services.compact-company = {
      description = "compact-company";
      wantedBy = [ "multi-user.target" ];
      # OIDC discovery runs at startup.
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} serve --config ${toml.generate "compact-company.toml" cfg.settings}";
        Restart = "on-failure";
        DynamicUser = true;
        StateDirectory = "compact-company";
        StateDirectoryMode = "0700";
        UMask = "0077";
        LoadCredential = lib.optional (
          cfg.clientSecretFile != null
        ) "client-secret:${cfg.clientSecretFile}";
        CapabilityBoundingSet = "";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        PrivateUsers = true;
        ProcSubset = "pid";
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        ProtectSystem = "strict";
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
          "~@resources"
        ];
      };
    };
  };
}
