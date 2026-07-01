pub mod api;
pub mod sse;

use crate::config::Config;
use crate::hub::Hub;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Router};
use rust_embed::RustEmbed;
use std::sync::Arc;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

#[derive(Clone)]
pub struct AppState {
    pub hub: Hub,
    pub cfg: Arc<Config>,
}

fn asset(path: &str) -> Response {
    match Assets::get(path) {
        Some(f) => ([(header::CONTENT_TYPE, mime_for(path))], f.data.into_owned()).into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn mime_for(path: &str) -> &'static str {
    if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".js") {
        "application/javascript"
    } else if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { asset("index.html") }))
        .route(
            "/assets/*path",
            get(|axum::extract::Path(p): axum::extract::Path<String>| async move { asset(&p) }),
        )
        .route("/api/state.json", get(api::state_handler))
        .route("/api/catalog.json", get(api::catalog_handler))
        .route("/api/stream", get(sse::stream_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(state)
}

pub async fn serve(cfg: Config, hub: Hub) -> anyhow::Result<()> {
    let addr = format!("{}:{}", cfg.http_bind, cfg.http_port);
    let state = AppState { hub, cfg: Arc::new(cfg) };
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("http/SCADA UI listening on {addr}");
    axum::serve(listener, router(state)).await?;
    Ok(())
}
