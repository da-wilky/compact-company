use std::{fmt, path::Path, sync::Arc};

use axum::body::Bytes;
use object_store::{
    ObjectStore, ObjectStoreExt, PutMode, aws::AmazonS3Builder, local::LocalFileSystem,
    path::Path as StorePath,
};
use sha2::{Digest, Sha256};

use crate::config::S3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    fn path(&self) -> StorePath {
        let hex = self.to_string();
        StorePath::from(format!("sha256/{}/{}", &hex[..2], &hex[2..]))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

/// Content-addressed: a file is stored under its SHA-256 and never overwritten (ADR 0016).
pub struct Files {
    store: Arc<dyn ObjectStore>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] object_store::Error),
    #[error("cannot create {0}: {1}")]
    CreateDir(String, std::io::Error),
    #[error("stored file {0} does not match its hash")]
    Corrupt(ContentHash),
}

impl Files {
    pub fn local(dir: &Path) -> Result<Self, Error> {
        std::fs::create_dir_all(dir).map_err(|e| Error::CreateDir(dir.display().to_string(), e))?;
        Ok(Self {
            store: Arc::new(LocalFileSystem::new_with_prefix(dir)?),
        })
    }

    pub fn s3(config: &S3) -> Result<Self, Error> {
        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(&config.bucket)
            .with_region(&config.region)
            .with_access_key_id(&config.access_key_id)
            .with_secret_access_key(&config.secret_access_key);
        if let Some(endpoint) = &config.endpoint {
            builder = builder
                .with_endpoint(endpoint)
                .with_allow_http(endpoint.starts_with("http://"));
        }
        Ok(Self {
            store: Arc::new(builder.build()?),
        })
    }

    pub async fn put(&self, bytes: Bytes) -> Result<ContentHash, Error> {
        let hash = ContentHash::of(&bytes);
        match self
            .store
            .put_opts(&hash.path(), bytes.into(), PutMode::Create.into())
            .await
        {
            Ok(_) | Err(object_store::Error::AlreadyExists { .. }) => Ok(hash),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get(&self, hash: ContentHash) -> Result<Bytes, Error> {
        let bytes = self.store.get(&hash.path()).await?.bytes().await?;
        if ContentHash::of(&bytes) != hash {
            return Err(Error::Corrupt(hash));
        }
        Ok(bytes)
    }
}
