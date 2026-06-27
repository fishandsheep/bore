//! Local web console for managing tunnels.

use std::{future::Future, net::SocketAddr};

use anyhow::{anyhow, Result};
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tokio::{net::TcpListener, sync::oneshot};

/// HTTP API routes and handlers.
pub mod api;
/// In-memory state and tunnel metadata.
pub mod state;
pub mod tunnel;

pub use state::{
    is_loopback_host, SessionInfo, SessionMode, SystemTunnelRole, SystemTunnelSpec, TunnelConfig,
    TunnelInfo, TunnelKind, TunnelStatus, WebState,
};

const INDEX_HTML: &str = include_str!("static/index.html");
const APP_JS: &str = include_str!("static/app.js");
const STYLE_CSS: &str = include_str!("static/style.css");

/// Web HTTP server config.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    /// Local address to bind HTTP server to.
    pub addr: SocketAddr,
    /// Session metadata exposed through `/api/session`.
    pub session: SessionInfo,
}

/// Starts local web console server.
pub async fn serve(config: ServeConfig) -> Result<()> {
    if !config.addr.ip().is_loopback() {
        eprintln!(
            "WARNING: Web console is exposed on a non-loopback address. This version has no authentication."
        );
    }
    let state = WebState::new(config.session.clone());
    serve_with_state(config, state, tokio::signal::ctrl_c()).await
}

/// Starts web console server with provided state and shutdown signal.
pub async fn serve_with_state<S>(config: ServeConfig, state: WebState, shutdown: S) -> Result<()>
where
    S: Future<Output = std::result::Result<(), std::io::Error>> + Send + 'static,
{
    let listener = TcpListener::bind(config.addr).await?;
    println!(
        "Bore web console listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            let _ = shutdown.await;
        })
        .await?;
    Ok(())
}

/// Runs web console plus required system tunnels.
pub async fn run_managed<S>(
    config: ServeConfig,
    state: WebState,
    system_tunnels: Vec<SystemTunnelSpec>,
    shutdown: S,
) -> Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    let mut system_ids = Vec::with_capacity(system_tunnels.len());
    for spec in system_tunnels {
        let id = state
            .create_system_tunnel(spec)
            .await
            .map_err(|err| anyhow!(err.message))?;
        state
            .start_tunnel(&id)
            .await
            .map_err(|err| anyhow!(err.message))?;
        system_ids.push(id);
    }

    for id in &system_ids {
        state
            .wait_for_running(id)
            .await
            .map_err(|err| anyhow!(err.message))?;
    }

    if let Some(url) = state.session().await.web_remote_url {
        println!("Remote web console: {url}");
    }
    if let Some(endpoint) = state.session().await.ssh_remote_endpoint {
        println!("Remote SSH tunnel: {endpoint}");
    }

    let (serve_shutdown_tx, serve_shutdown_rx) = oneshot::channel::<()>();
    let mut serve_task = tokio::spawn(serve_with_state(config, state.clone(), async move {
        let _ = serve_shutdown_rx.await;
        Ok::<(), std::io::Error>(())
    }));
    let monitor_state = state.clone();
    let monitor_ids = system_ids.clone();
    let mut monitor_task =
        tokio::spawn(async move { monitor_state.monitor_locked_tunnels(&monitor_ids).await });

    tokio::pin!(shutdown);

    let result = tokio::select! {
        _ = &mut shutdown => Ok(()),
        server = &mut serve_task => match server {
            Ok(result) => result,
            Err(err) => Err(anyhow!(err)),
        },
        monitor = &mut monitor_task => match monitor {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(anyhow!(err.message)),
            Err(err) => Err(anyhow!(err)),
        },
    };

    let _ = serve_shutdown_tx.send(());
    state.shutdown_all().await;
    monitor_task.abort();
    result
}

/// Builds web console router.
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
