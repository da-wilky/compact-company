{ pkgs, module }:
pkgs.testers.runNixOSTest {
  name = "compact-company";
  nodes.machine = {
    imports = [ module ];
    services.compact-company = {
      enable = true;
      settings = {
        listen = "127.0.0.1:9440";
        auth.mode = "none";
        business = {
          name = "Test GmbH";
          address = [ "Hauptstr. 1" ];
          tax_number = "12/345/67890";
        };
      };
    };
  };
  # The preview renders a PDF with typst, so it proves the hardened service can run it.
  testScript = ''
    url = "http://127.0.0.1:9440"
    machine.wait_for_open_port(9440)
    machine.succeed(f"curl -sf -d name=Aufwind -d address=Hauptstr.%202 -d currency=EUR -d invoice_language=de -d timezone=Europe/Berlin -d hourly_rate=85 {url}/customers")
    machine.succeed(f"curl -sf -d name=Site -d billable=on {url}/customers/1/projects")
    machine.succeed(f"curl -sf -d customer=1 -d grouping=project -d rounding_minutes=0 -d payment_days=14 -d project=1 {url}/invoices")
    machine.succeed(f"curl -sf -d description=Workshop -d quantity=1 -d unit=h -d unit_price=85 {url}/invoices/1/lines")
    machine.succeed(f"curl -sf -o preview.pdf {url}/invoices/1/preview")
    machine.succeed("head -c 4 preview.pdf | grep -q %PDF")
    machine.succeed("test -f /var/lib/compact-company/app.db")
  '';
}
