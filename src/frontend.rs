use crate::routes::AppState;
use axum::{
    extract::State,
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};

#[cfg(feature = "frontend")]
#[derive(rust_embed::RustEmbed)]
#[folder = "web/dist"]
struct Assets;

pub async fn index(state: State<AppState>) -> Response {
    serve("index.html", state).await
}

pub async fn asset(uri: Uri, state: State<AppState>) -> Response {
    serve(uri.path().trim_start_matches('/'), state).await
}

#[cfg(feature = "frontend")]
async fn serve(path: &str, State(st): State<AppState>) -> Response {
    let file = Assets::get(path).or_else(|| Assets::get("index.html"));
    match file {
        Some(f) => (
            [(axum::http::header::CONTENT_TYPE, mime_guess::from_path(path).first_or_octet_stream().to_string())],
            f.data.into_owned(),
        )
            .into_response(),
        // No dist/ was bundled at build time: behave like the headless build.
        None => missing(&st).into_response(),
    }
}

#[cfg(not(feature = "frontend"))]
async fn serve(_path: &str, State(st): State<AppState>) -> Response {
    missing(&st).into_response()
}

fn missing(st: &AppState) -> (StatusCode, String) {
    (
        StatusCode::OK,
        format!("openpaste — no frontend bundled, use the API:\n\n  echo hi | curl --data-binary @- {}\n", st.cfg.base_url),
    )
}
