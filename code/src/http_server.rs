use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tiny_http::{Server, Request, Response, StatusCode, Header};
use serde::{Deserialize, Serialize};

use crate::timestamp;

/// Check if another instance is running at the given port.
pub fn check_instance(app_path: &PathBuf, port: u16) -> bool {
    let addr = format!("127.0.0.1:{}", port);

    match TcpStream::connect(&addr) {
        Ok(mut stream) => {
            // Send POST /app-path
            let request = format!(
                "POST /app-path HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 0\r\n\r\n",
                port
            );

            if stream.write_all(request.as_bytes()).is_err() {
                return false;
            }

            // Read response
            let mut response = String::new();
            let mut buf = [0u8; 1024];
            if let Ok(n) = stream.read(&mut buf) {
                response = String::from_utf8_lossy(&buf[..n]).to_string();
            }

            // Parse response body (JSON string with path)
            if let Some(body_start) = response.find("\r\n\r\n") {
                let body = response[body_start + 4..].trim();
                // Remove quotes from JSON string
                let remote_path = body.trim_matches('"');
                let local_path = app_path.to_string_lossy();

                if remote_path == local_path {
                    // Same instance is running
                    return true;
                }
            }

            false
        }
        Err(_) => false,
    }
}

/// Start the HTTP server on an ephemeral port.
pub fn start_server(app_path: &PathBuf) -> (Server, u16) {
    let server = Server::http("127.0.0.1:0").expect("Failed to start HTTP server");
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
    (server, port)
}

/// Run the HTTP server loop.
pub fn run_server(server: Server, shutdown_flag: Arc<AtomicBool>) {
    let app_path = std::env::current_exe()
        .expect("Failed to get executable path")
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_exe().unwrap());

    loop {
        if shutdown_flag.load(Ordering::SeqCst) {
            break;
        }

        // Use non-blocking receive with timeout
        match server.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(Some(request)) => {
                handle_request(request, &app_path, &shutdown_flag);
            }
            Ok(None) => continue,
            Err(_) => continue,
        }
    }
}

fn handle_request(mut request: Request, app_path: &PathBuf, shutdown_flag: &Arc<AtomicBool>) {
    let url = request.url().to_string();

    match url.as_str() {
        "/app-path" => {
            let path_str = app_path.to_string_lossy();
            let body = format!("\"{}\"", path_str.replace('\\', "\\\\"));
            let response = Response::from_string(body)
                .with_status_code(StatusCode(200))
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
            let _ = request.respond(response);
        }
        "/shutdown" => {
            // Read body
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);

            // Parse JSON
            #[derive(Deserialize)]
            struct ShutdownRequest {
                timestamp: String,
            }

            match serde_json::from_str::<ShutdownRequest>(&body) {
                Ok(req) => {
                    // Validate timestamp
                    if timestamp::is_within_5_seconds(&req.timestamp) {
                        let response_body = r#"{"shutting_down": true}"#;
                        let response = Response::from_string(response_body)
                            .with_status_code(StatusCode(200))
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);

                        shutdown_flag.store(true, Ordering::SeqCst);
                    } else {
                        let response = Response::from_string("Invalid timestamp")
                            .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                    }
                }
                Err(_) => {
                    let response = Response::from_string("Invalid request body")
                        .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                }
            }
        }
        _ => {
            let response = Response::from_string("Not Found")
                .with_status_code(StatusCode(404));
            let _ = request.respond(response);
        }
    }
}
