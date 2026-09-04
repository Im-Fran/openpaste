mod config;
mod db;
mod frontend;
mod routes;
mod storage;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use axum_server::tls_rustls::RustlsConfig;
use config::Config;
use qrcode::{render::unicode::Dense1x2, QrCode};
use std::{
    io::{IsTerminal, Read},
    sync::Arc,
};

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
        /// PEM certificate chain; enables HTTPS (overrides TLS_CERT)
        #[arg(long, requires = "tls_key")]
        tls_cert: Option<String>,
        /// PEM private key (overrides TLS_KEY)
        #[arg(long, requires = "tls_cert")]
        tls_key: Option<String>,
        /// Address for a plain-HTTP listener that 308s to BASE_URL (overrides HTTP_REDIRECT_BIND)
        #[arg(long)]
        http_redirect: Option<String>,
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

    let default =
        Cmd::Serve { bind: None, headless: false, tls_cert: None, tls_key: None, http_redirect: None };
    match Cli::parse().cmd.unwrap_or(default) {
        Cmd::Serve { bind, headless, tls_cert, tls_key, http_redirect } => {
            serve(bind, headless, tls_cert.zip(tls_key), http_redirect).await
        }
        Cmd::Up { file, name, server } => up(file, name, server).await,
        Cmd::Get { id, server } => get(id, server).await,
    }
}

async fn serve(
    bind: Option<String>,
    headless: bool,
    tls: Option<(String, String)>,
    http_redirect: Option<String>,
) -> Result<()> {
    let mut cfg = Config::from_env()?;
    if let Some(b) = bind {
        cfg.bind = b;
    }
    cfg.headless |= headless;
    if let Some((cert, key)) = tls {
        cfg.tls = Some(config::Tls { cert, key });
    }
    if http_redirect.is_some() {
        cfg.http_redirect = http_redirect;
    }
    if cfg.http_redirect.is_some() {
        if cfg.tls.is_none() {
            bail!("the HTTP redirect needs TLS_CERT/TLS_KEY — there is nothing to redirect to otherwise");
        }
        // Redirigir a un BASE_URL http:// sería un bucle, no una redirección.
        if !cfg.base_url.starts_with("https://") {
            bail!("the HTTP redirect needs an https:// BASE_URL, got '{}'", cfg.base_url);
        }
    }

    let state = routes::AppState {
        db: db::Db::connect(&cfg.database_url).await?,
        blobs: storage::Blobs::new(&cfg.storage)?,
        cfg: Arc::new(cfg.clone()),
    };

    let app = routes::router(state);
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    let scheme = if cfg.tls.is_some() { "https" } else { "http" };
    tracing::info!("listening on {scheme}://{} (base_url {})", cfg.bind, cfg.base_url);
    if cfg.tls.is_some() && cfg.base_url.starts_with("http://") {
        tracing::warn!("TLS is on but BASE_URL is http://; the links handed to clients will be wrong");
    }

    let Some(tls) = &cfg.tls else {
        axum::serve(listener, app)
            .with_graceful_shutdown(async { tokio::signal::ctrl_c().await.ok(); })
            .await?;
        return Ok(());
    };

    // rustls 0.23 exige elegir proveedor; ring ya viene con reqwest.
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("failed to install the rustls crypto provider"))?;
    let tls_cfg = RustlsConfig::from_pem_file(&tls.cert, &tls.key)
        .await
        .map_err(|e| anyhow::anyhow!("cannot load TLS_CERT '{}' / TLS_KEY '{}': {e}", tls.cert, tls.key))?;

    if let Some(every) = cfg.tls_reload {
        let (tls_cfg, cert, key) = (tls_cfg.clone(), tls.cert.clone(), tls.key.clone());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                match tls_cfg.reload_from_pem_file(&cert, &key).await {
                    Ok(()) => tracing::debug!("TLS certificate reloaded from {cert}"),
                    // Un cert a medio renovar no debe tumbar el server: seguimos con el anterior.
                    Err(e) => tracing::warn!("TLS reload failed, keeping the current certificate: {e}"),
                }
            }
        });
    }

    if let Some(redirect_bind) = &cfg.http_redirect {
        serve_redirect(redirect_bind, cfg.base_url.clone()).await?;
    }

    let handle = axum_server::Handle::new();
    tokio::spawn({
        let handle = handle.clone();
        async move {
            tokio::signal::ctrl_c().await.ok();
            handle.graceful_shutdown(Some(std::time::Duration::from_secs(10)));
        }
    });
    axum_server::from_tcp_rustls(listener.into_std()?, tls_cfg)?
        .handle(handle)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

/// Plain-HTTP listener that 308s everything to `base_url`. The target comes from the
/// config and never from the Host header, so it cannot be turned into an open redirect.
async fn serve_redirect(bind: &str, base_url: String) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("redirecting http://{bind} to {base_url}");
    tokio::spawn(async move {
        let app = axum::Router::new().fallback(move |uri: axum::http::Uri| {
            let base = base_url.clone();
            async move {
                let tail = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
                axum::response::Redirect::permanent(&format!("{base}{tail}"))
            }
        });
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("the HTTP redirect listener stopped: {e}");
        }
    });
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
    // ponytail: QR a stderr para no ensuciar el pipe; solo si es una terminal.
    // Polaridad estándar (como `qrencode -t UTF8`): asume fondo claro; los
    // lectores de celular modernos igual decodifican el invertido.
    if std::io::stderr().is_terminal() {
        eprintln!("{}", QrCode::new(text.trim())?.render::<Dense1x2>().quiet_zone(true).build());
    }
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
