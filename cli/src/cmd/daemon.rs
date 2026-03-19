use crate::config;
use crate::focus;
use anyhow::Result;
use axum::{extract::Json, http::StatusCode, response::IntoResponse, routing::{get, post}, Router};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Deserialize)]
struct FocusRequest {
    app: Option<String>,
    window_id: Option<String>,
    tab_id: Option<String>,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
}

pub fn run() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_server())
}

async fn run_server() -> Result<()> {
    let pid_file = config::pid_file();

    // Ensure config dir exists
    if let Some(parent) = pid_file.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write PID file
    fs::write(&pid_file, std::process::id().to_string())?;

    let app = Router::new()
        .route("/health", get(health))
        .route("/focus", post(handle_focus));

    let port = config::daemon_port();
    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("[zestfuld] Listening on localhost:{}", port);

    // Graceful shutdown on SIGTERM/SIGINT
    let pid_file_clone = pid_file.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(pid_file_clone))
        .await?;

    // Cleanup PID file
    let _ = fs::remove_file(&pid_file);

    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(StatusResponse {
        status: "ok".to_string(),
    })
}

async fn handle_focus(Json(req): Json<FocusRequest>) -> impl IntoResponse {
    let app = match req.app {
        Some(app) if !app.is_empty() => app,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "app is required"})),
            );
        }
    };

    eprintln!(
        "[zestfuld] Focus: app={} window_id={} tab_id={}",
        app,
        req.window_id.as_deref().unwrap_or(""),
        req.tab_id.as_deref().unwrap_or("")
    );

    if let Err(e) = focus::handle_focus(
        &app,
        req.window_id.as_deref(),
        req.tab_id.as_deref(),
    )
    .await
    {
        eprintln!("[zestfuld] Focus error: {}", e);
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

async fn shutdown_signal(pid_file: std::path::PathBuf) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    eprintln!("[zestfuld] Shutting down...");
    let _ = fs::remove_file(&pid_file);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/health", get(health))
            .route("/focus", post(handle_focus))
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_focus_missing_app() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/focus")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "app is required");
    }

    #[tokio::test]
    async fn test_focus_empty_app() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/focus")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"app":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_focus_valid_request() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/focus")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"app":"kitty","window_id":"1","tab_id":"my-tab"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_focus_invalid_json() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/focus")
                    .header("content-type", "application/json")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        // axum returns 422 for deserialization errors
        assert!(response.status().is_client_error());
    }

    #[tokio::test]
    async fn test_focus_with_only_app() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/focus")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"app":"Terminal"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
