use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::io::{AsyncBufReadExt, BufReader};
use serde::{Deserialize, Serialize};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use tower_http::services::ServeDir;
use futures_util::{sink::SinkExt, stream::StreamExt};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    connections: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Message>>>>,
    processes: Arc<Mutex<HashMap<String, u32>>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ClientConfig {
    local_host: String,
    local_port: u16,
    to: String,
    port: u16,
    secret: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ServerConfig {
    min_port: u16,
    max_port: u16,
    secret: Option<String>,
    bind_addr: IpAddr,
    bind_tunnels: Option<IpAddr>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WebSocketMessage {
    msg_type: String,
    data: serde_json::Value,
    id: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    let app_state = AppState {
        connections: Arc::new(Mutex::new(HashMap::new())),
        processes: Arc::new(Mutex::new(HashMap::new())),
    };
    
    let app = Router::new()
        .route("/", get(root))
        .route("/ws", get(websocket_handler))
        .route("/api/client/start", post(start_client))
        .route("/api/server/start", post(start_server))
        .nest_service("/static", ServeDir::new("web_gui/static"))
        .with_state(app_state);
    
    let listener = match std::net::TcpListener::bind("127.0.0.1:3001") {
        Ok(listener) => {
            tracing::info!("Web GUI listening on http://localhost:3001");
            listener
        }
        Err(e) => {
            tracing::error!("Failed to bind to port 3001: {}", e);
            eprintln!("错误：端口 3001 已被占用，请停止其他正在运行的 bore-gui 进程");
            eprintln!("使用以下命令查找并停止占用端口的进程：");
            eprintln!("  lsof -i :3001");
            eprintln!("  kill -9 <PID>");
            std::process::exit(1);
        }
    };
    
    axum_server::from_tcp(listener)
      .serve(app.into_make_service())
      .await
      .unwrap();
}

async fn root() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", "/static/index.html")
        .body(axum::body::Empty::new())
        .unwrap()
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel();
    
    let conn_id = Uuid::new_v4().to_string();
    
    state.connections.lock().unwrap().insert(conn_id.clone(), tx);
    
    let send_task = async {
        let mut sender = sender;
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    };
    
    let recv_task = async {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(&text) {
                    match ws_msg.msg_type.as_str() {
                        "ping" => {
                            let pong = WebSocketMessage {
                                msg_type: "pong".to_string(),
                                data: serde_json::json!({}),
                                id: ws_msg.id,
                            };
                            let _ = state.connections.lock().unwrap()
                                .get(&conn_id)
                                .unwrap()
                                .send(Message::Text(serde_json::to_string(&pong).unwrap()));
                        }
                        _ => {}
                    }
                }
            }
        }
    };
    
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
    
    state.connections.lock().unwrap().remove(&conn_id);
}

async fn start_client(
    State(state): State<AppState>,
    Json(config): Json<ClientConfig>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = Uuid::new_v4().to_string();
    
    let exe_path = std::env::current_exe().unwrap();
    let bore_path = exe_path.parent().unwrap().join("bore");
    let mut cmd = Command::new(bore_path);
    cmd.args(&[
        "local",
        &config.local_port.to_string(),
        "--local-host", &config.local_host,
        "--to", &config.to,
        "--port", &config.port.to_string(),
    ]);
    
    if let Some(secret) = &config.secret {
        cmd.arg("--secret").arg(secret);
    }
    
    // Capture stdout to parse the assigned port
    cmd.stdout(Stdio::piped())
       .stderr(Stdio::piped());
    
    match cmd.spawn() {
        Ok(child) => {
            let mut child = child;
            let pid = child.id().unwrap_or(0);
            state.processes.lock().unwrap().insert(id.clone(), pid);
            
            // Spawn a task to monitor output
            let state_clone = state.clone();
            let id_clone = id.clone();
            let to_clone = config.to.clone();
            tokio::spawn(async move {
                if let Some(stdout) = child.stdout.take() {
                    let mut reader = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        // Parse port from different formats:
                        // 1. "listening at {to}:{port}"
                        // 2. "connected to server remote_port={port}"
                        let mut actual_port = None;
                        
                        if line.contains("listening at") {
                            if let Some(port_str) = line.split(':').last() {
                                if let Ok(port) = port_str.trim().parse::<u16>() {
                                    actual_port = Some(port);
                                }
                            }
                        } else if line.contains("remote_port=") {
                            if let Some(port_str) = line.split("remote_port=").last() {
                                if let Some(port_str) = port_str.split_whitespace().next() {
                                    if let Ok(port) = port_str.parse::<u16>() {
                                        actual_port = Some(port);
                                    }
                                }
                            }
                        }
                        
                        if let Some(port) = actual_port {
                            broadcast_message(
                                &state_clone,
                                WebSocketMessage {
                                    msg_type: "client_port_assigned".to_string(),
                                    data: serde_json::json!({
                                        "id": id_clone,
                                        "remote_port": port,
                                        "full_address": format!("{}:{}", to_clone, port)
                                    }),
                                    id: None,
                                },
                            ).await;
                        }
                        
                        // Broadcast all log lines
                        broadcast_message(
                            &state_clone,
                            WebSocketMessage {
                                msg_type: "client_log".to_string(),
                                data: serde_json::json!({
                                    "id": id_clone,
                                    "line": line
                                }),
                                id: None,
                            },
                        ).await;
                    }
                }
                
                // Handle stderr as well
                if let Some(stderr) = child.stderr.take() {
                    let mut reader = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        broadcast_message(
                            &state_clone,
                            WebSocketMessage {
                                msg_type: "client_log".to_string(),
                                data: serde_json::json!({
                                    "id": id_clone,
                                    "line": line,
                                    "is_error": true
                                }),
                                id: None,
                            },
                        ).await;
                    }
                }
                
                // Process exited
                let _ = child.wait().await;
                state_clone.processes.lock().unwrap().remove(&id_clone);
                broadcast_message(
                    &state_clone,
                    WebSocketMessage {
                        msg_type: "client_exited".to_string(),
                        data: serde_json::json!({ "id": id_clone }),
                        id: None,
                    },
                ).await;
            });
            
            broadcast_message(
                &state,
                WebSocketMessage {
                    msg_type: "client_started".to_string(),
                    data: serde_json::json!({
                        "id": id,
                        "config": config,
                        "pid": pid
                    }),
                    id: None,
                },
            ).await;
            
            Ok(Json(serde_json::json!({ "id": id, "pid": pid })))
        }
        Err(e) => {
            tracing::error!("Failed to start client: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn stop_client(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut processes = state.processes.lock().unwrap();
    
    if let Some(pid) = processes.remove(&id) {
        #[cfg(unix)]
        {
            use std::process::Command;
            let _ = Command::new("kill").arg(pid.to_string()).output();
        }
        
        #[cfg(windows)]
        {
            use std::process::Command;
            let _ = Command::new("taskkill").args(&["/F", "/PID", &pid.to_string()]).output();
        }
        
        broadcast_message(
            &state,
            WebSocketMessage {
                msg_type: "client_stopped".to_string(),
                data: serde_json::json!({ "id": id }),
                id: None,
            },
        ).await;
        
        Ok(Json(serde_json::json!({ "success": true })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn start_server(
    State(state): State<AppState>,
    Json(config): Json<ServerConfig>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = Uuid::new_v4().to_string();
    
    let exe_path = std::env::current_exe().unwrap();
    let bore_path = exe_path.parent().unwrap().join("bore");
    let mut cmd = Command::new(bore_path);
    cmd.args(&[
        "server",
        "--min-port", &config.min_port.to_string(),
        "--max-port", &config.max_port.to_string(),
        "--bind-addr", &config.bind_addr.to_string(),
    ]);
    
    if let Some(secret) = &config.secret {
        cmd.arg("--secret").arg(secret);
    }
    
    if let Some(bind_tunnels) = &config.bind_tunnels {
        cmd.arg("--bind-tunnels").arg(&bind_tunnels.to_string());
    }
    
    // Capture stdout to parse server output
    cmd.stdout(Stdio::piped())
       .stderr(Stdio::piped());
    
    match cmd.spawn() {
        Ok(child) => {
            let mut child = child;
            let pid = child.id().unwrap_or(0);
            state.processes.lock().unwrap().insert(id.clone(), pid);
            
            // Spawn a task to monitor output
            let state_clone = state.clone();
            let id_clone = id.clone();
            tokio::spawn(async move {
                if let Some(stdout) = child.stdout.take() {
                    let mut reader = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        // Broadcast all log lines
                        broadcast_message(
                            &state_clone,
                            WebSocketMessage {
                                msg_type: "server_log".to_string(),
                                data: serde_json::json!({
                                    "id": id_clone,
                                    "line": line
                                }),
                                id: None,
                            },
                        ).await;
                    }
                }
                
                // Handle stderr as well
                if let Some(stderr) = child.stderr.take() {
                    let mut reader = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        broadcast_message(
                            &state_clone,
                            WebSocketMessage {
                                msg_type: "server_log".to_string(),
                                data: serde_json::json!({
                                    "id": id_clone,
                                    "line": line,
                                    "is_error": true
                                }),
                                id: None,
                            },
                        ).await;
                    }
                }
                
                // Process exited
                let _ = child.wait().await;
                state_clone.processes.lock().unwrap().remove(&id_clone);
                broadcast_message(
                    &state_clone,
                    WebSocketMessage {
                        msg_type: "server_exited".to_string(),
                        data: serde_json::json!({ "id": id_clone }),
                        id: None,
                    },
                ).await;
            });
            
            broadcast_message(
                &state,
                WebSocketMessage {
                    msg_type: "server_started".to_string(),
                    data: serde_json::json!({
                        "id": id,
                        "config": config,
                        "pid": pid
                    }),
                    id: None,
                },
            ).await;
            
            Ok(Json(serde_json::json!({ "id": id, "pid": pid })))
        }
        Err(e) => {
            tracing::error!("Failed to start server: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn stop_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut processes = state.processes.lock().unwrap();
    
    if let Some(pid) = processes.remove(&id) {
        #[cfg(unix)]
        {
            use std::process::Command;
            let _ = Command::new("kill").arg(pid.to_string()).output();
        }
        
        #[cfg(windows)]
        {
            use std::process::Command;
            let _ = Command::new("taskkill").args(&["/F", "/PID", &pid.to_string()]).output();
        }
        
        broadcast_message(
            &state,
            WebSocketMessage {
                msg_type: "server_stopped".to_string(),
                data: serde_json::json!({ "id": id }),
                id: None,
            },
        ).await;
        
        Ok(Json(serde_json::json!({ "success": true })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn broadcast_message(state: &AppState, message: WebSocketMessage) {
    let msg = Message::Text(serde_json::to_string(&message).unwrap());
    let connections = state.connections.lock().unwrap();
    
    for tx in connections.values() {
        let _ = tx.send(msg.clone());
    }
}