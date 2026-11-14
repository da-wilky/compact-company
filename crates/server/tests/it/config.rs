use std::fs;

use compact_company::config::{Config, Error};

fn load(toml: &str) -> Result<Config, Error> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, toml).unwrap();
    Config::load(&path)
}

#[test]
fn empty_config_uses_defaults() {
    let config = load("").unwrap();
    assert_eq!(config.listen.to_string(), "127.0.0.1:9440");
    assert_eq!(config.data_dir.to_str(), Some("data"));
    assert!(config.storage.s3.is_none());
}

#[test]
fn unknown_keys_are_rejected() {
    assert!(matches!(
        load("lisen = \"0.0.0.0:80\""),
        Err(Error::Parse(..))
    ));
    assert!(matches!(
        load("[business]\nnmae = \"x\""),
        Err(Error::Parse(..))
    ));
}

#[test]
fn secrets_are_read_only_from_files() {
    let s3 = "[storage.s3]\nbucket = \"b\"\nregion = \"r\"\naccess_key_id = \"id\"\n";
    assert!(matches!(
        load(&format!("{s3}secret_access_key = \"inline\"")),
        Err(Error::Parse(..))
    ));

    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("secret");
    fs::write(&secret, "from-file\n").unwrap();
    let config = load(&format!("{s3}secret_access_key_file = {secret:?}")).unwrap();
    assert_eq!(config.storage.s3.unwrap().secret_access_key, "from-file");
}

#[test]
fn public_url_is_an_origin() {
    assert!(load("public_url = \"https://cc.example.com\"").is_ok());
    assert!(matches!(
        load("public_url = \"https://example.com/cc/\""),
        Err(Error::PublicUrl(_))
    ));
}
