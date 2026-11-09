{ pkgs, module }:
pkgs.testers.runNixOSTest {
  name = "compact-company";
  nodes.machine = {
    imports = [ module ];
    environment.systemPackages = [ pkgs.curl ];
    environment.etc."compact-company-oidc".text = "secret";
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
      };
    };
  };
  testScript = ''
    machine.wait_for_unit("compact-company.service")
    machine.wait_for_open_port(9440)
    machine.succeed("curl -sf http://127.0.0.1:9440/api/health | grep -q ok")
    machine.succeed("curl -sf http://127.0.0.1:9440/ | grep -q /_app/")
    machine.succeed("curl -sf http://127.0.0.1:9440/api/session | grep -q 'VM IdP'")
    machine.succeed("journalctl -u compact-company | grep -q 'setup#'")
    machine.succeed("test -s /var/lib/compact-company/compact-company.db")
    machine.fail("sudo -u nobody cat /var/lib/compact-company/compact-company.db")
  '';
}
