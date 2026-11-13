# Validates the app's e-invoices (ZUGFeRD's EN 16931 XML and XRechnung, for a regular and a small
# business; invoice, cancellation, partial and final invoice) with the KoSIT validator.
{ pkgs, server }:
let
  validator = pkgs.fetchurl {
    url = "https://github.com/itplr-kosit/validator/releases/download/v1.6.3/validator-1.6.3-standalone.jar";
    hash = "sha256-eZ5kvvypfUCA4DYIyAuF3VpezF9K5PNdERbsKFW5p8k=";
  };
  configuration = pkgs.fetchurl {
    url = "https://github.com/itplr-kosit/validator-configuration-xrechnung/releases/download/v2026-08-31/xrechnung-3.0.2-validator-configuration-2026-08-31.zip";
    hash = "sha256-JTDNEHxBRRHF0EYuwQ+IaRA5Wr/Kgg24LoPXC/ASIag=";
  };
in
pkgs.runCommand "kosit"
  {
    nativeBuildInputs = [
      pkgs.jdk21_headless
      pkgs.unzip
    ];
  }
  ''
    mkdir config xml
    unzip -q ${configuration} -d config
    for kind in invoice cancellation partial final; do
      for business in "" --small-business; do
        for flavour in "" --xrechnung; do
          ${pkgs.lib.getExe server} einvoice --kind $kind $business $flavour > xml/$kind$business$flavour.xml
        done
      done
    done
    java -jar ${validator} -s config/scenarios.xml -r config -o report xml/*.xml | tee result
    grep -q "Rejected:  0" result
    mv report $out
  ''
