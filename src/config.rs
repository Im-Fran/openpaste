use anyhow::{bail, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub base_url: String,
    pub database_url: String,
    pub storage: Storage,
    pub max_size: usize,
    pub headless: bool,
    pub tls: Option<Tls>,
    pub tls_reload: Option<std::time::Duration>,
    pub http_redirect: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Tls {
    pub cert: String,
    pub key: String,
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

        // Ambos o ninguno: con solo uno de los dos el servidor arrancaría en
        // texto plano sin que nadie se entere.
        let tls = match (std::env::var("TLS_CERT").ok(), std::env::var("TLS_KEY").ok()) {
            (Some(cert), Some(key)) => Some(Tls { cert, key }),
            (None, None) => None,
            _ => bail!("TLS_CERT and TLS_KEY must be set together"),
        };

        // Certbot renueva sin avisar: releer el PEM cada tanto evita el reinicio.
        let tls_reload = match env("TLS_RELOAD_SECS", "3600").parse::<u64>()? {
            0 => None,
            secs => Some(std::time::Duration::from_secs(secs)),
        };

        Ok(Config {
            bind: env("BIND", "0.0.0.0:8080"),
            base_url: env("BASE_URL", "http://localhost:8080").trim_end_matches('/').to_string(),
            database_url: env("DATABASE_URL", "sqlite://./data/openpaste.db?mode=rwc"),
            storage,
            max_size: env("MAX_UPLOAD_BYTES", "104857600").parse()?,
            headless: env("HEADLESS", "false") == "true",
            tls,
            tls_reload,
            http_redirect: std::env::var("HTTP_REDIRECT_BIND").ok(),
        })
    }
}
