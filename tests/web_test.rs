use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{anyhow, Result};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use bore_cli::{
    server::Server,
    shared::CONTROL_PORT,
    web::{router, WebState},
};
use serde_json::{json, Value};
use tokio::{net::TcpStream, sync::Mutex, task::JoinHandle, time};
use tower::ServiceExt;

static SERIAL_GUARD: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct ServerGuard {
    task: JoinHandle<Result<()>>,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn wait_for_control_port_closed() -> Result<()> {
    for _ in 0..100 {
        if TcpStream::connect(("localhost", CONTROL_PORT))
            .await
            .is_err()
        {
            return Ok(());
        }
        time::sleep(Duration::from_millis(10)).await;
    }
    Err(anyhow!("previous server did not release control port"))
}

async fn spawn_server(secret: Option<&str>) -> Result<ServerGuard> {
    wait_for_control_port_closed().await?;
    let task = tokio::spawn(Server::new(1024..=65535, secret).listen());

    for _ in 0..250 {
        if task.is_finished() {
            return match task.await {
                Ok(Ok(())) => Err(anyhow!("server exited before listening")),
                Ok(Err(err)) => Err(err),
                Err(err) => Err(err.into()),
            };
        }

        match TcpStream::connect(("localhost", CONTROL_PORT)).await {
            Ok(_) => return Ok(ServerGuard { task }),
            Err(_) => time::sleep(Duration::from_millis(10)).await,
        }
    }

    Err(anyhow!("server did not start listening on control port"))
}

async fn json_response(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    serde_json::from_slice(&bytes).expect("body should be JSON")
}

#[tokio::test]
async fn get_tunnels_returns_empty_array() {
    let app = router(WebState::default());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tunnels")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_response(response).await;
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn tunnel_crud_and_logs_api() -> Result<()> {
    let _guard = SERIAL_GUARD.lock().await;
    let _server = spawn_server(None).await?;
    let app = router(WebState::default());

    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tunnels")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "dev",
                        "local_port": 3000,
                        "to": "localhost",
                        "port": null,
                        "local_host": "127.0.0.1",
                        "secret": null
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await?;
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let create_body = json_response(create_response).await;
    let id = create_body["id"]
        .as_str()
        .expect("id should exist")
        .to_string();

    let start_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/tunnels/{id}/start"))
                .body(Body::empty())
                .unwrap(),
        )
        .await?;
    assert_eq!(start_response.status(), StatusCode::OK);

    let mut status = String::new();
    for _ in 0..250 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/tunnels")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await?;
        let body = json_response(response).await;
        status = body[0]["status"].as_str().unwrap_or_default().to_string();
        if status == "Running" {
            break;
        }
        time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, "Running", "tunnel did not reach Running state");

    let logs_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/tunnels/{id}/logs"))
                .body(Body::empty())
                .unwrap(),
        )
        .await?;
    assert_eq!(logs_response.status(), StatusCode::OK);
    let logs_body = json_response(logs_response).await;
    assert!(logs_body["logs"].as_array().expect("logs array").len() >= 2);

    let stop_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/tunnels/{id}/stop"))
                .body(Body::empty())
                .unwrap(),
        )
        .await?;
    assert_eq!(stop_response.status(), StatusCode::OK);

    let delete_response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/tunnels/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await?;
    assert_eq!(delete_response.status(), StatusCode::OK);

    Ok(())
}
