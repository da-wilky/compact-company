use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use url::Url;

use crate::settings::Business;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    /// The address browsers use; passkeys are bound to its host.
    #[serde(default = "default_public_url")]
    pub public_url: Url,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default)]
    pub storage: Storage,
    #[serde(default)]
    pub business: Business,
    pub oidc: Option<Oidc>,
    /// Reverse proxies whose X-Forwarded-For names the real client, e.g. `["127.0.0.1"]`.
    #[serde(default)]
    pub trusted_proxies: Vec<std::net::IpAddr>,
    /// Where approved Typst packages are downloaded from.
    #[serde(default = "default_typst_packages")]
    pub typst_packages: Url,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Oidc {
    pub issuer: Url,
    pub client_id: String,
    client_secret_file: PathBuf,
    #[serde(skip)]
    pub client_secret: String,
    /// Label of the sign-in button.
    #[serde(default = "default_oidc_name")]
    pub name: String,
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
    #[error("public_url must be an origin such as https://cc.example.com, not {0}")]
    PublicUrl(Url),
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
        let url = &config.public_url;
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(Error::PublicUrl(url.clone()));
        }
        if let Some(s3) = &mut config.storage.s3 {
            s3.secret_access_key = read(&s3.secret_access_key_file)?.trim_end().to_owned();
        }
        if let Some(oidc) = &mut config.oidc {
            oidc.client_secret = read(&oidc.client_secret_file)?.trim_end().to_owned();
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

fn default_public_url() -> Url {
    "http://localhost:9440".parse().expect("valid URL")
}

fn default_data_dir() -> PathBuf {
    "data".into()
}

fn default_oidc_name() -> String {
    "Single sign-on".into()
}

fn default_typst_packages() -> Url {
    "https://packages.typst.org/".parse().expect("valid URL")
}
