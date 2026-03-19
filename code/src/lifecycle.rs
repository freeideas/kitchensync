use rusqlite::Connection;
use std::io::Read;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

use crate::database;
use crate::timestamp;

pub fn instance_check(conn: &Connection, config_path: &Path) -> Result<(), String> {
    let canonical = config_path
        .canonicalize()
        .unwrap_or_else(|_| config_path.to_path_buf());
    let canonical_str = canonical.to_string_lossy().to_string();

    if let Some(port_str) = database::get_config(conn, "serving-port") {
        if let Ok(port) = port_str.parse::<u16>() {
            if let Ok(response) = post_app_path(port) {
                let response = response.trim().trim_matches('"');
                if response == canonical_str {
                    println!("Already running against {}", canonical_str);
                    std::process::exit(0);
                }
            }
        }
    }
    Ok(())
}

pub fn start_server(
    conn: Arc<std::sync::Mutex<Connection>>,
    config_path: &Path,
    log_retention_days: u64,
) -> Result<(), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Cannot bind: {}", e))?;
    let port = listener.local_addr().unwrap().port();

    {
        let c = conn.lock().unwrap();
        database::set_config(&c, "serving-port", &port.to_string())?;
        database::log(&c, "info", "KitchenSync started", log_retention_days);
    }

    let canonical = config_path
        .canonicalize()
        .unwrap_or_else(|_| config_path.to_path_buf());
    let canonical_str = canonical.to_string_lossy().to_string();

    std::thread::spawn(move || {
        let server = tiny_http::Server::from_listener(listener, None).unwrap();
        for mut request in server.incoming_requests() {
            let url = request.url().to_string();
            let method = request.method().to_string();

            if method == "POST" && url == "/app-path" {
                let body = serde_json::json!(canonical_str);
                let response = tiny_http::Response::from_string(body.to_string())
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                            .unwrap(),
                    );
                request.respond(response).ok();
            } else if method == "POST" && url == "/shutdown" {
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).ok();
                let valid = if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(ts) = val.get("timestamp").and_then(|t| t.as_str()) {
                        timestamp::is_within_tolerance(ts, &timestamp::now(), 5)
                    } else {
                        false
                    }
                } else {
                    false
                };
                if valid {
                    let resp = serde_json::json!({"shutting_down": true});
                    let response = tiny_http::Response::from_string(resp.to_string());
                    request.respond(response).ok();
                    {
                        let c = conn.lock().unwrap();
                        database::log(&c, "info", "KitchenSync shutting down", log_retention_days);
                    }
                    std::process::exit(0);
                } else {
                    let response = tiny_http::Response::from_string("Invalid timestamp")
                        .with_status_code(400);
                    request.respond(response).ok();
                }
            } else {
                let response =
                    tiny_http::Response::from_string("Not found").with_status_code(404);
                request.respond(response).ok();
            }
        }
    });

    Ok(())
}

fn post_app_path(port: u16) -> Result<String, String> {
    let addr = format!("127.0.0.1:{}", port);
    let mut stream = std::net::TcpStream::connect_timeout(
        &addr.parse().unwrap(),
        std::time::Duration::from_secs(2),
    )
    .map_err(|e| e.to_string())?;

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();

    use std::io::{BufRead, BufReader, Write};
    write!(
        stream,
        "POST /app-path HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        port
    )
    .map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(&stream);
    let mut content_length: usize = 0;

    // Read headers
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
        if let Some(val) = trimmed.strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Read body
    let mut body = vec![0u8; content_length];
    std::io::Read::read_exact(&mut reader, &mut body).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&body).to_string())
}
