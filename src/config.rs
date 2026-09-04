use anyhow::{bail, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub base_url: String,
    pub database_url: String,
    pub storage: Storage,
    pub max_size: usize,
    pub headless: bool,
}

#[derive(Clone, Debug)]
pub enum Storage {
    Local { path: String },
    S3 { bucket: String, prefix: String },
}

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let storage = match env("STORAGE_DRIVER", "local").as_str() {
            "local" => Storage::Local { path: env("STORAGE_PATH", "./data/blobs") },
            "s3" => Storage::S3 {
                bucket: std::env::var("S3_BUCKET")
                    .map_err(|_| anyhow::anyhow!("S3_BUCKET is required when STORAGE_DRIVER=s3"))?,
                prefix: env("S3_PREFIX", "openpaste"),
            },
            other => bail!("unknown STORAGE_DRIVER '{other}' (expected 'local' or 's3')"),
        };

        Ok(Config {
            bind: env("BIND", "0.0.0.0:8080"),
            base_url: env("BASE_URL", "http://localhost:8080").trim_end_matches('/').to_string(),
            database_url: env("DATABASE_URL", "sqlite://./data/openpaste.db?mode=rwc"),
            storage,
            max_size: env("MAX_UPLOAD_BYTES", "104857600").parse()?,
            headless: env("HEADLESS", "false") == "true",
        })
    }
}
