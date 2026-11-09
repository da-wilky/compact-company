use std::fs;

#[test]
fn committed_openapi_description_is_current() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/openapi.json");
    let generated = compact_company::openapi().to_pretty_json().unwrap() + "\n";
    assert!(
        fs::read_to_string(path).unwrap() == generated,
        "web/openapi.json is outdated; run `cargo run -- openapi > web/openapi.json`"
    );
}
