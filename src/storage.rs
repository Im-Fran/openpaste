use crate::config::Storage as StorageCfg;
use anyhow::Result;
use bytes::Bytes;
use object_store::{local::LocalFileSystem, aws::AmazonS3Builder, ObjectStore, PutPayload};
use std::sync::Arc;

#[derive(Clone)]
pub struct Blobs {
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl Blobs {
    pub fn new(cfg: &StorageCfg) -> Result<Self> {
        Ok(match cfg {
            StorageCfg::Local { path } => {
                std::fs::create_dir_all(path)?;
                Blobs { store: Arc::new(LocalFileSystem::new_with_prefix(path)?), prefix: String::new() }
            }
            StorageCfg::S3 { bucket, prefix } => Blobs {
                // Credentials/region come from the standard AWS_* env vars.
                store: Arc::new(AmazonS3Builder::from_env().with_bucket_name(bucket).build()?),
                prefix: format!("{}/", prefix.trim_matches('/')),
            },
        })
    }

    fn path(&self, key: &str) -> object_store::path::Path {
        object_store::path::Path::from(format!("{}{key}", self.prefix))
    }

    pub async fn put(&self, key: &str, data: Bytes) -> Result<()> {
        self.store.put(&self.path(key), PutPayload::from_bytes(data)).await?;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Result<Bytes> {
        Ok(self.store.get(&self.path(key)).await?.bytes().await?)
    }
}
