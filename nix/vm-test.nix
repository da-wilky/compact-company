{ pkgs, module }:
let
  # a key for this test only; real identities stay off the server
  identity = "AGE-SECRET-KEY-15QJD0KN088HW2AEN50HDA40630U50VC23C8JKHVLA3508DM5HPRSLZZEC9";
  recipient = "age1wyqdmlphf38v35ls7res52y9m7tsp475dck3e8zuzhu2eenyldvqvxgjl6";
  backup = {
    inherit recipient;
    # due as soon as the service starts
    time = "00:00";
    prefix = "vm";
    s3 = {
      bucket = "backups";
      region = "us-east-1";
      endpoint = "http://127.0.0.1:9000";
      access_key_id = "test-access-key";
      secret_access_key_file = "/etc/s3-secret";
    };
  };
in
pkgs.testers.runNixOSTest {
  name = "compact-company";
  nodes.machine =
    { config, lib, ... }:
    {
      imports = [ module ];
      environment.systemPackages = [
        pkgs.curl
        pkgs.rustfs-cli
        pkgs.sqlite
        config.services.compact-company.package
      ];
      environment.etc."compact-company-oidc".text = "secret";
      environment.etc."s3-secret".text = "test-secret-key";
      environment.etc."rustfs-keys".text = ''
        RUSTFS_ACCESS_KEY=test-access-key
        RUSTFS_SECRET_KEY=test-secret-key
      '';
      environment.etc."backup.key".text = identity;
      environment.etc."restore.toml".source = (pkgs.formats.toml { }).generate "restore.toml" {
        data_dir = "/tmp/restored";
        inherit backup;
      };
      services.rustfs = {
        enable = true;
        environmentFile = "/etc/rustfs-keys";
      };
      services.compact-company = {
        enable = true;
        settings = {
          business.name = "VM Test";
          # discovery runs at sign-in, so an unreachable provider still lets the service start
          oidc = {
            issuer = "http://127.0.0.1:1/realm";
            client_id = "compact-company";
            client_secret_file = "/etc/compact-company-oidc";
            name = "VM IdP";
          };
          inherit backup;
        };
      };
      # started once the bucket exists, so its first backup doesn't wait an hour for a retry
      systemd.services.compact-company.wantedBy = lib.mkForce [ ];
    };
  testScript = ''
    machine.wait_for_unit("rustfs.service")
    machine.wait_for_open_port(9000)
    machine.succeed("rc alias set local http://127.0.0.1:9000 test-access-key test-secret-key")
    machine.succeed("rc bucket create local/backups")
    machine.succeed("systemctl start compact-company")

    machine.wait_for_unit("compact-company.service")
    machine.wait_for_open_port(9440)
    machine.succeed("curl -sf http://127.0.0.1:9440/api/health | grep -q ok")
    machine.succeed("curl -sf http://127.0.0.1:9440/ | grep -q /_app/")
    machine.succeed("curl -sf http://127.0.0.1:9440/api/session | grep -q 'VM IdP'")
    machine.succeed("journalctl -u compact-company | grep -q 'setup#'")
    machine.succeed("test -s /var/lib/compact-company/compact-company.db")
    machine.fail("sudo -u nobody cat /var/lib/compact-company/compact-company.db")

    list = "(compact-company restore --config /etc/restore.toml --identity /etc/backup.key || true)"
    machine.wait_until_succeeds(f"{list} | grep -q Z$", timeout=180)
    snapshot = machine.succeed(
        list
    ).strip().splitlines()[-1]
    report = machine.succeed(
        f"compact-company restore --config /etc/restore.toml --identity /etc/backup.key --snapshot {snapshot}"
    )
    assert "match their checksums" in report, report
    query = "SELECT count(*), group_concat(data) FROM process_version"
    original = machine.succeed(f"sqlite3 /var/lib/compact-company/compact-company.db '{query}'")
    restored = machine.succeed(f"sqlite3 /tmp/restored/compact-company.db '{query}'")
    assert original == restored, (original, restored)
    machine.fail(
        f"compact-company restore --config /etc/restore.toml --identity /etc/backup.key --snapshot {snapshot}"
    )
  '';
}
