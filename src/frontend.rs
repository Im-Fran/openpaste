use crate::{db::Paste, routes::AppState};
use axum::{
    extract::State,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};

#[cfg(feature = "frontend")]
#[derive(rust_embed::RustEmbed)]
#[folder = "assets/web"]
struct Assets;

/// Single-pass `{{key}}` substitution. Single pass on purpose: values are never
/// rescanned, so pasted content containing `{{...}}` cannot inject a placeholder.
// ponytail: 20 lines beat a template-engine dependency; swap in minijinja if the
// templates ever need loops or conditionals.
fn render(tpl: &str, vars: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(tpl.len() + 256);
    let mut rest = tpl;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(j) = after.find("}}") else {
            rest = &rest[i..];
            break;
        };
        let key = after[..j].trim();
        if let Some((_, v)) = vars.iter().find(|(k, _)| *k == key) {
            out.push_str(v);
        }
        rest = &after[j + 2..];
    }
    out.push_str(rest);
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn human_size(n: i64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{:.1} MB", n as f64 / (1024.0 * 1024.0)),
    }
}

pub async fn index(State(st): State<AppState>) -> Response {
    page("openpaste", "new.html", &[("base_url", st.cfg.base_url.clone())], &st)
}

pub fn paste_page(st: &AppState, p: &Paste) -> Response {
    let tpl = if p.is_binary() { "view_binary.html" } else { "view_text.html" };
    let name = p.filename.clone().unwrap_or_else(|| p.id.clone());
    page(
        &format!("{name} — openpaste"),
        tpl,
        &[
            ("id", esc(&p.id)),
            ("name", esc(&name)),
            ("content_type", esc(&p.content_type)),
            ("size", human_size(p.size)),
            ("url", esc(&format!("{}/paste/{}", st.cfg.base_url, p.id))),
            ("content", esc(p.content.as_deref().unwrap_or(""))),
        ],
        st,
    )
}

pub async fn asset(uri: Uri, State(st): State<AppState>) -> Response {
    let path = uri.path().trim_start_matches('/');
    match file(path).filter(|_| !path.ends_with(".html")) {
        Some(data) => (
            [(header::CONTENT_TYPE, mime_guess::from_path(path).first_or_octet_stream().to_string())],
            data,
        )
            .into_response(),
        None => not_found(&st),
    }
}

fn page(title: &str, body_tpl: &str, vars: &[(&str, String)], st: &AppState) -> Response {
    let (Some(layout), Some(body)) = (text(&file("layout.html")), text(&file(body_tpl))) else {
        // No assets bundled at build time: behave like the headless build.
        return missing(st).into_response();
    };
    let html = render(
        &layout,
        &[("title", esc(title)), ("body", render(&body, vars))],
    );
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

fn text(data: &Option<Vec<u8>>) -> Option<String> {
    data.as_ref().map(|d| String::from_utf8_lossy(d).into_owned())
}

#[cfg(feature = "frontend")]
fn file(path: &str) -> Option<Vec<u8>> {
    Assets::get(path).map(|f| f.data.into_owned())
}

#[cfg(not(feature = "frontend"))]
fn file(_path: &str) -> Option<Vec<u8>> {
    None
}

fn not_found(st: &AppState) -> Response {
    match file("layout.html") {
        Some(_) => (StatusCode::NOT_FOUND, "not found\n").into_response(),
        None => missing(st).into_response(),
    }
}

fn missing(st: &AppState) -> (StatusCode, String) {
    (
        StatusCode::OK,
        format!("openpaste — no frontend bundled, use the API:\n\n  echo hi | curl --data-binary @- {}\n", st.cfg.base_url),
    )
}

#[cfg(test)]
mod tests {
    use super::{esc, render};

    #[test]
    fn renders_and_never_rescans_values() {
        let vars = [("a", "{{b}}".to_string()), ("b", "boom".to_string())];
        assert_eq!(render("<p>{{a}}</p>", &vars), "<p>{{b}}</p>");
        assert_eq!(render("{{ a }}|{{missing}}|{{unclosed", &vars), "{{b}}||{{unclosed");
        assert_eq!(esc("<script>&\"x\""), "&lt;script&gt;&amp;&quot;x&quot;");
    }
}
