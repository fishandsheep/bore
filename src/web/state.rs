#![allow(missing_docs)]

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::{
    sync::{mpsc, oneshot, Mutex, RwLock},
    task::JoinHandle,
};
use uuid::Uuid;

use crate::client::{run_local, LocalArgs, TunnelEvent};

const MAX_LOG_LINES: usize = 500;

/// Tunnel configuration accepted by the web API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub name: String,
    pub local_port: u16,
    pub to: String,
    pub port: Option<u16>,
    pub local_host: String,
    pub secret: Option<String>,
}

/// Public tunnel configuration returned by the web API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicTunnelConfig {
    pub name: String,
    pub local_port: u16,
    pub to: String,
    pub port: Option<u16>,
    pub local_host: String,
}

/// Tunnel lifecycle state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TunnelStatus {
    Stopped,
    Starting,
    Running,
    Failed,
}

/// Tunnel information returned by the web API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelInfo {
    pub id: String,
    pub config: PublicTunnelConfig,
    pub status: TunnelStatus,
    pub remote_port: Option<u16>,
    pub error: Option<String>,
    pub has_secret: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Shared web state.
#[derive(Debug, Clone, Default)]
pub struct WebState {
    tunnels: Arc<RwLock<HashMap<String, Arc<Mutex<TunnelRuntime>>>>>,
}

/// Structured state-layer error for HTTP mapping.
#[derive(Debug, Clone)]
pub struct StateError {
    pub status: StatusCode,
    pub message: String,
}

#[derive(Debug)]
struct TunnelRuntime {
    id: String,
    config: TunnelConfig,
    status: TunnelStatus,
    remote_port: Option<u16>,
    error: Option<String>,
    created_at: String,
    updated_at: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
    logs: VecDeque<String>,
}

impl WebState {
    pub async fn list_tunnels(&self) -> Vec<TunnelInfo> {
        let entries = {
            let tunnels = self.tunnels.read().await;
            tunnels.values().cloned().collect::<Vec<_>>()
        };

        let mut views = Vec::with_capacity(entries.len());
        for entry in entries {
            let runtime = entry.lock().await;
            views.push(runtime.view());
        }
        views.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        views
    }

    pub async fn create_tunnel(&self, config: TunnelConfig) -> Result<String, StateError> {
        let config = normalize_config(config)?;
        let now = now_rfc3339();
        let id = Uuid::new_v4().to_string();
        let runtime = TunnelRuntime {
            id: id.clone(),
            config,
            status: TunnelStatus::Stopped,
            remote_port: None,
            error: None,
            created_at: now.clone(),
            updated_at: now,
            shutdown_tx: None,
            handle: None,
            logs: VecDeque::new(),
        };
        self.tunnels
            .write()
            .await
            .insert(id.clone(), Arc::new(Mutex::new(runtime)));
        Ok(id)
    }

    pub async fn update_tunnel(&self, id: &str, config: TunnelConfig) -> Result<(), StateError> {
        let config = normalize_config(config)?;
        let entry = self.entry(id).await?;

        {
            let runtime = entry.lock().await;
            if matches!(
                runtime.status,
                TunnelStatus::Starting | TunnelStatus::Running
            ) {
                return Err(conflict("cannot edit a running tunnel"));
            }
        }

        let entries = {
            let tunnels = self.tunnels.read().await;
            tunnels.values().cloned().collect::<Vec<_>>()
        };
        let config_key = config.identity_key();
        for other in entries {
            let runtime = other.lock().await;
            if runtime.id != id && runtime.config.identity_key() == config_key {
                return Err(conflict("an identical tunnel already exists"));
            }
        }

        let mut runtime = entry.lock().await;
        let secret = config.secret.or_else(|| runtime.config.secret.clone());
        runtime.config = TunnelConfig { secret, ..config };
        runtime.remote_port = None;
        runtime.error = None;
        runtime.touch();
        runtime.push_log("tunnel config updated".to_string());
        Ok(())
    }

    pub async fn start_tunnel(&self, id: &str) -> Result<(), StateError> {
        let entries = {
            let tunnels = self.tunnels.read().await;
            let entry = tunnels
                .get(id)
                .cloned()
                .ok_or_else(|| not_found("tunnel not found"))?;
            let others = tunnels.values().cloned().collect::<Vec<_>>();
            (entry, others)
        };

        let (entry, others) = entries;
        let config_key = {
            let runtime = entry.lock().await;
            if matches!(
                runtime.status,
                TunnelStatus::Starting | TunnelStatus::Running
            ) {
                return Err(conflict("tunnel is already running"));
            }
            runtime.config.identity_key()
        };

        for other in others {
            let runtime = other.lock().await;
            if runtime.id != id
                && matches!(
                    runtime.status,
                    TunnelStatus::Starting | TunnelStatus::Running
                )
                && runtime.config.identity_key() == config_key
            {
                return Err(conflict("an identical tunnel is already running"));
            }
        }

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let entry_for_events = Arc::clone(&entry);
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let mut runtime = entry_for_events.lock().await;
                runtime.apply_event(event);
            }
        });

        let local_args = {
            let mut runtime = entry.lock().await;
            runtime.status = TunnelStatus::Starting;
            runtime.remote_port = None;
            runtime.error = None;
            runtime.touch();
            let tunnel_name = runtime.config.name.clone();
            runtime.push_log(format!("starting tunnel {}", tunnel_name));
            runtime.shutdown_tx = Some(shutdown_tx);
            LocalArgs::from(runtime.config.clone())
        };

        let handle = tokio::spawn(async move {
            let _ = run_local(
                local_args,
                async move {
                    let _ = shutdown_rx.await;
                },
                Some(event_tx),
            )
            .await;
        });

        let mut runtime = entry.lock().await;
        runtime.handle = Some(handle);
        Ok(())
    }

    pub async fn stop_tunnel(&self, id: &str) -> Result<(), StateError> {
        let entry = self.entry(id).await?;
        let (shutdown_tx, handle) = {
            let mut runtime = entry.lock().await;
            if !matches!(
                runtime.status,
                TunnelStatus::Starting | TunnelStatus::Running
            ) {
                return Ok(());
            }
            (runtime.shutdown_tx.take(), runtime.handle.take())
        };

        if let Some(shutdown_tx) = shutdown_tx {
            let _ = shutdown_tx.send(());
        }
        if let Some(handle) = handle {
            let _ = handle.await;
        }

        let mut runtime = entry.lock().await;
        if matches!(
            runtime.status,
            TunnelStatus::Starting | TunnelStatus::Running
        ) {
            runtime.status = TunnelStatus::Stopped;
            runtime.touch();
            runtime.push_log("tunnel stopped".to_string());
        }
        Ok(())
    }

    pub async fn delete_tunnel(&self, id: &str) -> Result<(), StateError> {
        let entry = self.entry(id).await?;
        {
            let runtime = entry.lock().await;
            if matches!(
                runtime.status,
                TunnelStatus::Starting | TunnelStatus::Running
            ) {
                return Err(conflict("cannot delete a running tunnel"));
            }
        }
        self.tunnels.write().await.remove(id);
        Ok(())
    }

    pub async fn logs(&self, id: &str) -> Result<Vec<String>, StateError> {
        let entry = self.entry(id).await?;
        let runtime = entry.lock().await;
        Ok(runtime.logs.iter().cloned().collect())
    }

    pub async fn get_tunnel(&self, id: &str) -> Result<TunnelInfo, StateError> {
        let entry = self.entry(id).await?;
        let runtime = entry.lock().await;
        Ok(runtime.view())
    }

    async fn entry(&self, id: &str) -> Result<Arc<Mutex<TunnelRuntime>>, StateError> {
        self.tunnels
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| not_found("tunnel not found"))
    }
}

impl TunnelRuntime {
    fn view(&self) -> TunnelInfo {
        TunnelInfo {
            id: self.id.clone(),
            config: self.config.public(),
            status: self.status,
            remote_port: self.remote_port,
            error: self.error.clone(),
            has_secret: self.config.secret.is_some(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }

    fn apply_event(&mut self, event: TunnelEvent) {
        match event {
            TunnelEvent::Log(message) => self.push_log(message),
            TunnelEvent::Started { remote_port } => {
                self.status = TunnelStatus::Running;
                self.remote_port = remote_port;
                self.error = None;
                self.touch();
                if let Some(remote_port) = remote_port {
                    self.push_log(format!("remote port assigned: {remote_port}"));
                }
            }
            TunnelEvent::Stopped => {
                self.status = TunnelStatus::Stopped;
                self.shutdown_tx = None;
                self.touch();
                self.push_log("tunnel stopped".to_string());
            }
            TunnelEvent::Failed(message) => {
                self.status = TunnelStatus::Failed;
                self.remote_port = None;
                self.error = Some(message.clone());
                self.shutdown_tx = None;
                self.touch();
                self.push_log(format!("error: {message}"));
            }
        }
    }

    fn push_log(&mut self, message: String) {
        self.logs.push_back(message);
        while self.logs.len() > MAX_LOG_LINES {
            self.logs.pop_front();
        }
        self.updated_at = now_rfc3339();
    }

    fn touch(&mut self) {
        self.updated_at = now_rfc3339();
    }
}

impl TunnelConfig {
    fn public(&self) -> PublicTunnelConfig {
        PublicTunnelConfig {
            name: self.name.clone(),
            local_port: self.local_port,
            to: self.to.clone(),
            port: self.port,
            local_host: self.local_host.clone(),
        }
    }

    fn identity_key(&self) -> (String, u16, String, Option<u16>) {
        (
            self.local_host.clone(),
            self.local_port,
            self.to.clone(),
            self.port,
        )
    }
}

impl From<TunnelConfig> for LocalArgs {
    fn from(value: TunnelConfig) -> Self {
        Self {
            local_port: value.local_port,
            local_host: value.local_host,
            to: value.to,
            port: value.port.unwrap_or(0),
            secret: value.secret,
        }
    }
}

fn normalize_config(mut config: TunnelConfig) -> Result<TunnelConfig, StateError> {
    config.name = config.name.trim().to_string();
    config.to = config.to.trim().to_string();
    config.local_host = config.local_host.trim().to_string();
    config.secret = config
        .secret
        .as_ref()
        .map(|secret| secret.trim().to_string())
        .filter(|secret| !secret.is_empty());

    if config.name.is_empty() {
        return Err(bad_request("name cannot be empty"));
    }
    if config.local_port == 0 {
        return Err(bad_request("local_port must be a valid port"));
    }
    if config.to.is_empty() {
        return Err(bad_request("to cannot be empty"));
    }
    if config.local_host.is_empty() {
        return Err(bad_request("local_host cannot be empty"));
    }
    if matches!(config.port, Some(0)) {
        return Err(bad_request("port must be a valid port"));
    }

    Ok(config)
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn bad_request(message: impl Into<String>) -> StateError {
    StateError {
        status: StatusCode::BAD_REQUEST,
        message: message.into(),
    }
}

fn not_found(message: impl Into<String>) -> StateError {
    StateError {
        status: StatusCode::NOT_FOUND,
        message: message.into(),
    }
}

fn conflict(message: impl Into<String>) -> StateError {
    StateError {
        status: StatusCode::CONFLICT,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{TunnelConfig, TunnelStatus, WebState, MAX_LOG_LINES};

    fn config(name: &str) -> TunnelConfig {
        TunnelConfig {
            name: name.to_string(),
            local_port: 3000,
            to: "bore.pub".to_string(),
            port: Some(9000),
            local_host: "127.0.0.1".to_string(),
            secret: None,
        }
    }

    #[tokio::test]
    async fn create_tunnel() {
        let state = WebState::default();
        let id = state
            .create_tunnel(config("dev"))
            .await
            .expect("create should work");
        let tunnels = state.list_tunnels().await;
        assert_eq!(tunnels.len(), 1);
        assert_eq!(tunnels[0].id, id);
        assert_eq!(tunnels[0].status, TunnelStatus::Stopped);
    }

    #[tokio::test]
    async fn delete_tunnel() {
        let state = WebState::default();
        let id = state
            .create_tunnel(config("dev"))
            .await
            .expect("create should work");
        state.delete_tunnel(&id).await.expect("delete should work");
        assert!(state.list_tunnels().await.is_empty());
    }

    #[tokio::test]
    async fn delete_running_tunnel_returns_error() {
        let state = WebState::default();
        let id = state
            .create_tunnel(config("dev"))
            .await
            .expect("create should work");
        {
            let entry = state.entry(&id).await.expect("entry should exist");
            let mut runtime = entry.lock().await;
            runtime.status = TunnelStatus::Running;
        }
        let err = state
            .delete_tunnel(&id)
            .await
            .expect_err("delete should fail");
        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn starting_running_tunnel_returns_error() {
        let state = WebState::default();
        let id = state
            .create_tunnel(config("dev"))
            .await
            .expect("create should work");
        {
            let entry = state.entry(&id).await.expect("entry should exist");
            let mut runtime = entry.lock().await;
            runtime.status = TunnelStatus::Running;
        }
        let err = state
            .start_tunnel(&id)
            .await
            .expect_err("start should fail");
        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn logs_cap_at_500_lines() {
        let state = WebState::default();
        let id = state
            .create_tunnel(config("dev"))
            .await
            .expect("create should work");
        let entry = state.entry(&id).await.expect("entry should exist");
        {
            let mut runtime = entry.lock().await;
            for index in 0..(MAX_LOG_LINES + 10) {
                runtime.push_log(format!("line-{index}"));
            }
        }
        let logs = state.logs(&id).await.expect("logs should exist");
        assert_eq!(logs.len(), MAX_LOG_LINES);
        assert_eq!(logs.first().expect("first log"), "line-10");
    }
}
