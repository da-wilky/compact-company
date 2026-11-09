use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::settings::Business;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default)]
    pub storage: Storage,
    #[serde(default)]
    pub business: Business,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub s3: Option<S3>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S3 {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key_id: String,
    secret_access_key_file: PathBuf,
    #[serde(skip)]
    pub secret_access_key: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read {0}: {1}")]
    Read(PathBuf, std::io::Error),
    #[error("invalid config {0}: {1}")]
    Parse(PathBuf, toml::de::Error),
}

impl Default for Config {
    fn default() -> Self {
        toml::from_str("").expect("every config key has a default")
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text = read(path)?;
        let mut config: Self =
            toml::from_str(&text).map_err(|e| Error::Parse(path.to_owned(), e))?;
        if let Some(s3) = &mut config.storage.s3 {
            s3.secret_access_key = read(&s3.secret_access_key_file)?.trim_end().to_owned();
        }
        Ok(config)
    }
}

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|e| Error::Read(path.to_owned(), e))
}

fn default_listen() -> SocketAddr {
    ([127, 0, 0, 1], 9440).into()
}

fn default_data_dir() -> PathBuf {
    "data".into()
}
