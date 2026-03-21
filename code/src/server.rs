use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Notify;

use crate::database::Database;
use crate::timestamp;

/// Start the HTTP server on localhost with OS-assigned port.
/// Returns (port, shutdown_notify).
pub async fn start_server(
    config_dir: PathBuf,
    db_path: PathBuf,
    shutdown_notify: Arc<Notify>,
) -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("cannot bind: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("cannot get local addr: {}", e))?
        .port();

    let config_dir = Arc::new(config_dir);
    let shutdown = shutdown_notify.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _addr)) => {
                            let io = TokioIo::new(stream);
                            let config_dir = config_dir.clone();
                            let shutdown = shutdown.clone();
                            tokio::spawn(async move {
                                let svc = service_fn(move |req| {
                                    let config_dir = config_dir.clone();
                                    let shutdown = shutdown.clone();
                                    async move {
                                        handle_request(req, config_dir, shutdown).await
                                    }
                                });
                                let _ = http1::Builder::new().serve_connection(io, svc).await;
                            });
                        }
                        Err(_) => continue,
                    }
                }
                _ = shutdown.notified() => {
                    break;
                }
            }
        }
    });

    Ok(port)
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    config_dir: Arc<PathBuf>,
    shutdown: Arc<Notify>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    if method != hyper::Method::POST {
        return Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Full::new(Bytes::new()))
            .unwrap());
    }

    match path.as_str() {
        "/app-path" => {
            let canonical = config_dir
                .canonicalize()
                .unwrap_or_else(|_| config_dir.as_ref().clone());
            let path_str = canonical.to_string_lossy().to_string();
            let json = serde_json::to_string(&path_str).unwrap();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json)))
                .unwrap())
        }
        "/shutdown" => {
            let body_bytes = http_body_util::BodyExt::collect(req.into_body())
                .await
                .map(|c| c.to_bytes())
                .unwrap_or_default();

            let body_str = String::from_utf8_lossy(&body_bytes);

            let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body_str);
            match parsed {
                Ok(val) => {
                    if let Some(ts_str) = val.get("timestamp").and_then(|v| v.as_str()) {
                        if let Some(ts_us) = timestamp::parse_to_micros(ts_str) {
                            let now_us = chrono::Utc::now().timestamp_micros();
                            if (now_us - ts_us).abs() <= 5_000_000 {
                                let resp = Response::builder()
                                    .status(StatusCode::OK)
                                    .header("Content-Type", "application/json")
                                    .body(Full::new(Bytes::from(r#"{"shutting_down":true}"#)))
                                    .unwrap();
                                // Signal shutdown after response is sent
                                tokio::spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                    shutdown.notify_waiters();
                                    std::process::exit(0);
                                });
                                return Ok(resp);
                            }
                        }
                    }
                    Ok(Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .header("Content-Type", "application/json")
                        .body(Full::new(Bytes::from(r#"{"error":"invalid timestamp"}"#)))
                        .unwrap())
                }
                Err(_) => Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::new()))
                    .unwrap()),
            }
        }
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::new()))
            .unwrap()),
    }
}

/// Check if another instance is already running.
pub async fn check_existing_instance(
    db: &Database,
    config_dir: &std::path::Path,
) -> bool {
    let port_str = match db.get_config("serving-port") {
        Some(s) => s,
        None => return false,
    };

    let port: u16 = match port_str.parse() {
        Ok(p) => p,
        Err(_) => return false,
    };

    // Try to POST /app-path
    let addr = format!("127.0.0.1:{}", port);
    let Ok(stream) = tokio::net::TcpStream::connect(&addr).await else {
        return false;
    };

    let io = TokioIo::new(stream);
    let (mut sender, conn) = match hyper::client::conn::http1::handshake::<_, Full<Bytes>>(io).await {
        Ok(r) => r,
        Err(_) => return false,
    };

    tokio::spawn(async move {
        let _ = conn.await;
    });

    let req = Request::builder()
        .method(hyper::Method::POST)
        .uri("/app-path")
        .body(Full::new(Bytes::new()))
        .unwrap();

    let resp: Response<hyper::body::Incoming> = match sender.send_request(req).await {
        Ok(r) => r,
        Err(_) => return false,
    };

    let body: Bytes = match http_body_util::BodyExt::collect(resp.into_body()).await {
        Ok(c) => c.to_bytes(),
        Err(_) => return false,
    };

    let body_str = String::from_utf8_lossy(&body);
    let returned_path: String = match serde_json::from_str(&body_str) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let canonical = config_dir
        .canonicalize()
        .unwrap_or_else(|_| config_dir.to_path_buf());
    let canonical_str = canonical.to_string_lossy().to_string();

    returned_path == canonical_str
}
