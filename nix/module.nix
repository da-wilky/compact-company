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
  isSecret = path: lib.hasSuffix "_file" (lib.last path);
  credential = path: lib.concatStringsSep "." path;
  # The service user cannot read secrets owned by root, so each `*_file` is passed as a credential.
  settings = lib.mapAttrsRecursive (
    path: value:
    if isSecret path then "/run/credentials/compact-company.service/${credential path}" else value
  ) cfg.settings;
  credentials = lib.flatten (
    lib.collect lib.isList (
      lib.mapAttrsRecursive (
        path: value: if isSecret path then [ "${credential path}:${value}" ] else [ ]
      ) cfg.settings
    )
  );
in
{
  options.services.compact-company = {
    enable = lib.mkEnableOption "compact-company, a business suite for one German business";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "compact-company.packages.\${system}.default";
    };

    settings = lib.mkOption {
      description = ''
        The config file, see the README. Every key ending in `_file` names a file the service
        receives as a systemd credential, so secrets may stay readable by root only.
      '';
      default = { };
      type = lib.types.submodule {
        freeformType = toml.type;
        options = {
          listen = lib.mkOption {
            type = lib.types.str;
            default = "127.0.0.1:9440";
          };
          data_dir = lib.mkOption {
            type = lib.types.str;
            default = "/var/lib/compact-company";
            readOnly = true;
          };
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.compact-company = {
      description = "compact-company";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} serve --config ${toml.generate "compact-company.toml" settings}";
        Restart = "on-failure";
        DynamicUser = true;
        StateDirectory = "compact-company";
        StateDirectoryMode = "0700";
        LoadCredential = credentials;
        UMask = "0077";
        CapabilityBoundingSet = "";
        NoNewPrivileges = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        PrivateDevices = true;
        PrivateTmp = true;
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
