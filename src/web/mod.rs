//! Local web console for managing tunnels.

use std::net::SocketAddr;

use anyhow::Result;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tokio::net::TcpListener;

/// HTTP API routes and handlers.
pub mod api;
/// In-memory state and tunnel metadata.
pub mod state;
pub mod tunnel;

pub use state::WebState;

const INDEX_HTML: &str = include_str!("static/index.html");
const APP_JS: &str = include_str!("static/app.js");
const STYLE_CSS: &str = include_str!("static/style.css");

/// Starts the local web console server.
pub async fn serve(addr: SocketAddr) -> Result<()> {
    if !addr.ip().is_loopback() {
        eprintln!(
            "WARNING: Web console is exposed on a non-loopback address. This version has no authentication."
        );
    }

    let state = WebState::default();
    let listener = TcpListener::bind(addr).await?;
    println!("Bore web console listening on http://{addr}");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

/// Builds the web console router.
pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .nest("/api", api::router())
        .with_state(state)
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn app_js() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        APP_JS,
    )
}

async fn style_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        STYLE_CSS,
    )
}
