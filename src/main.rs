mod config;
mod db;
mod frontend;
mod routes;
mod storage;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use config::Config;
use std::{io::Read, sync::Arc};

#[derive(Parser)]
#[command(name = "openpaste", version, about = "Paste service for text, terminal output and binaries")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the HTTP server
    Serve {
        /// Address to bind (overrides BIND)
        #[arg(long)]
        bind: Option<String>,
        /// Serve the API only, without the web UI
        #[arg(long)]
        headless: bool,
    },
    /// Upload a file (or stdin) and print the resulting URL
    Up {
        /// File to upload; omit to read stdin
        file: Option<String>,
        /// Override the file name sent to the server
        #[arg(long)]
        name: Option<String>,
        /// Server base URL (env: OPENPASTE_SERVER)
        #[arg(long, env = "OPENPASTE_SERVER", default_value = "http://localhost:8080")]
        server: String,
    },
    /// Print the raw content of a paste to stdout
    Get {
        /// Paste id or full URL
        id: String,
        #[arg(long, env = "OPENPASTE_SERVER", default_value = "http://localhost:8080")]
        server: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "openpaste=info,tower_http=warn".into()))
        .init();

    match Cli::parse().cmd.unwrap_or(Cmd::Serve { bind: None, headless: false }) {
        Cmd::Serve { bind, headless } => serve(bind, headless).await,
        Cmd::Up { file, name, server } => up(file, name, server).await,
        Cmd::Get { id, server } => get(id, server).await,
    }
}

async fn serve(bind: Option<String>, headless: bool) -> Result<()> {
    let mut cfg = Config::from_env()?;
    if let Some(b) = bind {
        cfg.bind = b;
    }
    cfg.headless |= headless;

    let state = routes::AppState {
        db: db::Db::connect(&cfg.database_url).await?,
        blobs: storage::Blobs::new(&cfg.storage)?,
        cfg: Arc::new(cfg.clone()),
    };

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!("listening on http://{} (base_url {})", cfg.bind, cfg.base_url);
    axum::serve(listener, routes::router(state))
        .with_graceful_shutdown(async { tokio::signal::ctrl_c().await.ok(); })
        .await?;
    Ok(())
}

async fn up(file: Option<String>, name: Option<String>, server: String) -> Result<()> {
    let (bytes, name) = match &file {
        Some(f) => (
            std::fs::read(f)?,
            name.or_else(|| Some(f.rsplit('/').next().unwrap_or(f).to_string())),
        ),
        None => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf)?;
            (buf, name)
        }
    };
    if bytes.is_empty() {
        bail!("nothing to upload");
    }

    let client = reqwest::Client::new();
    let mut req = client.post(server.trim_end_matches('/')).body(bytes);
    if let Some(n) = name {
        req = req.header("x-filename", n);
    }
    let res = req.send().await?;
    let status = res.status();
    let text = res.text().await?;
    if !status.is_success() {
        bail!("upload failed ({status}): {}", text.trim());
    }
    print!("{text}");
    Ok(())
}

async fn get(id: String, server: String) -> Result<()> {
    let url = if id.starts_with("http") {
        format!("{}/raw", id.trim_end_matches('/').trim_end_matches("/raw"))
    } else {
        format!("{}/paste/{id}/raw", server.trim_end_matches('/'))
    };
    let res = reqwest::get(&url).await?;
    if !res.status().is_success() {
        bail!("{}: {}", res.status(), url);
    }
    use std::io::Write;
    std::io::stdout().write_all(&res.bytes().await?)?;
    Ok(())
}
