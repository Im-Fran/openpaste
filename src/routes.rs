use crate::{config::Config, db::{Db, Paste}, storage::Blobs};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use axum::extract::DefaultBodyLimit;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

const ALPHABET: [char; 62] = [
    'a','b','c','d','e','f','g','h','i','j','k','l','m','n','o','p','q','r','s','t','u','v','w','x','y','z',
    'A','B','C','D','E','F','G','H','I','J','K','L','M','N','O','P','Q','R','S','T','U','V','W','X','Y','Z',
    '0','1','2','3','4','5','6','7','8','9',
];

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: Db,
    pub blobs: Blobs,
}

pub struct AppError(StatusCode, String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, format!("{}\n", self.1)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        let e = e.into();
        tracing::error!("{e:#}");
        AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

fn not_found() -> AppError {
    AppError(StatusCode::NOT_FOUND, "paste not found".into())
}

pub fn router(state: AppState) -> Router {
    let limit = state.cfg.max_size;
    let mut app = Router::new()
        .route("/", post(create).put(create))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/pastes", post(create))
        .route("/api/pastes/{id}", get(api_get))
        .route("/paste/{id}", get(view))
        .route("/paste/{id}/raw", get(raw))
        .route("/paste/{id}/download", get(download))
        .route("/{filename}", put(create_named));

    app = if state.cfg.headless {
        app.route("/", get(help)).fallback(help)
    } else {
        app.route("/", get(crate::frontend::index)).fallback(crate::frontend::asset)
    };

    app.layer(DefaultBodyLimit::max(limit))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn help(State(st): State<AppState>) -> impl IntoResponse {
    format!(
        "openpaste (headless)\n\n  echo hi | curl --data-binary @- {0}\n  curl -T file.bin {0}/file.bin\n\n  GET {0}/paste/<id>/raw\n  GET {0}/paste/<id>/download\n",
        st.cfg.base_url
    )
}

async fn create_named(
    state: State<AppState>,
    Path(filename): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    store(state, Some(filename), headers, body).await
}

async fn create(state: State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, AppError> {
    let name = headers.get("x-filename").and_then(|v| v.to_str().ok()).map(str::to_string);
    store(state, name, headers, body).await
}

async fn store(
    State(st): State<AppState>,
    filename: Option<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    if body.is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "empty body".into()));
    }
    if body.len() > st.cfg.max_size {
        return Err(AppError(StatusCode::PAYLOAD_TOO_LARGE, format!("max {} bytes", st.cfg.max_size)));
    }

    // Text if it is valid UTF-8 without NUL bytes; everything else is treated as a binary blob.
    let text = std::str::from_utf8(&body).ok().filter(|s| !s.contains('\0'));
    let filename = filename.map(|f| f.rsplit('/').next().unwrap_or("file").to_string());

    let content_type = match (&filename, &text) {
        (Some(f), _) => mime_guess::from_path(f).first_or_octet_stream().to_string(),
        (None, Some(_)) => "text/plain; charset=utf-8".into(),
        (None, None) => "application/octet-stream".into(),
    };

    let id = nanoid::nanoid!(8, &ALPHABET);
    let mut paste = Paste {
        id: id.clone(),
        content: text.map(str::to_string),
        storage_key: None,
        filename,
        content_type,
        size: body.len() as i64,
        created_at: std::time::UNIX_EPOCH.elapsed().map(|d| d.as_secs() as i64).unwrap_or(0),
    };

    if paste.content.is_none() {
        st.blobs.put(&id, body).await?;
        paste.storage_key = Some(id.clone());
    }
    st.db.insert(&paste).await?;

    let url = format!("{}/paste/{}", st.cfg.base_url, id);
    let wants_json = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.contains("application/json"));

    Ok(if wants_json {
        (StatusCode::CREATED, Json(serde_json::json!({
            "id": id,
            "url": url,
            "raw_url": format!("{url}/raw"),
            "download_url": format!("{url}/download"),
            "size": paste.size,
            "binary": paste.is_binary(),
        }))).into_response()
    } else {
        (StatusCode::CREATED, format!("{url}\n")).into_response()
    })
}

async fn api_get(State(st): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let p = st.db.get(&id).await?.ok_or_else(not_found)?;
    Ok(Json(p).into_response())
}

async fn body_of(st: &AppState, p: &Paste) -> Result<Bytes, AppError> {
    Ok(match (&p.content, &p.storage_key) {
        (Some(c), _) => Bytes::from(c.clone()),
        (_, Some(k)) => st.blobs.get(k).await?,
        _ => Bytes::new(),
    })
}

async fn raw(State(st): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let p = st.db.get(&id).await?.ok_or_else(not_found)?;
    let ct = if p.is_binary() { p.content_type.clone() } else { "text/plain; charset=utf-8".into() };
    Ok(([(header::CONTENT_TYPE, ct)], body_of(&st, &p).await?).into_response())
}

async fn download(State(st): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let p = st.db.get(&id).await?.ok_or_else(not_found)?;
    let name = p.filename.clone().unwrap_or_else(|| p.id.clone());
    Ok((
        [
            (header::CONTENT_TYPE, p.content_type.clone()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", name.replace('"', ""))),
        ],
        body_of(&st, &p).await?,
    )
        .into_response())
}

async fn view(state: State<AppState>, path: Path<String>) -> Result<Response, AppError> {
    if state.0.cfg.headless {
        return raw(state, path).await;
    }
    Ok(crate::frontend::index(state).await.into_response())
}
