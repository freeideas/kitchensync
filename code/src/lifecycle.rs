use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::database::Database;
use crate::timestamp;

pub struct LifecycleServer {
    shutdown: Arc<AtomicBool>,
    port: u16,
    config_path: String,
    listener_thread: Option<thread::JoinHandle<()>>,
}

impl LifecycleServer {
    pub fn start(db: &Database, config_path: &str) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("Cannot bind lifecycle server: {}", e))?;
        let port = listener.local_addr().unwrap().port();
        listener
            .set_nonblocking(false)
            .ok();

        // Upsert serving-port
        db.set_config("serving-port", &port.to_string());

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();
        let config_path_clone = config_path.to_string();

        // Set a short timeout so we can check shutdown flag
        listener
            .set_nonblocking(true)
            .ok();

        let handle = thread::spawn(move || {
            loop {
                if shutdown_clone.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let path = config_path_clone.clone();
                        let sd = shutdown_clone.clone();
                        thread::spawn(move || {
                            handle_request(stream, &path, &sd);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });

        Ok(LifecycleServer {
            shutdown,
            port,
            config_path: config_path.to_string(),
            listener_thread: Some(handle),
        })
    }

    /// Linger for the specified duration, then shut down.
    pub fn linger(self, duration: Duration) {
        let start = Instant::now();
        while start.elapsed() < duration {
            if self.shutdown.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        self.shutdown.store(true, Ordering::Relaxed);
    }

    pub fn stop(self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// Check if another instance is running.
pub fn check_existing_instance(port: u16, config_path: &str) -> bool {
    let addr = format!("127.0.0.1:{}", port);
    let stream = match TcpStream::connect_timeout(
        &addr.parse().unwrap(),
        Duration::from_secs(2),
    ) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let mut stream = stream;
    let request = format!(
        "POST /app-path HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 0\r\n\r\n",
        port
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();

    let _response = String::new();
    let mut reader = BufReader::new(&stream);

    // Read status line
    let mut status_line = String::new();
    if reader.read_line(&mut status_line).is_err() {
        return false;
    }
    if !status_line.contains("200") {
        return false;
    }

    // Read headers
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(val) = trimmed.strip_prefix("Content-Length: ") {
            content_length = val.parse().unwrap_or(0);
        }
    }

    // Read body
    let mut body = vec![0u8; content_length];
    if reader.read_exact(&mut body).is_err() {
        return false;
    }
    let body_str = String::from_utf8_lossy(&body);

    // Parse JSON response - should be a JSON string (the config path)
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_str) {
        if let Some(returned_path) = val.as_str() {
            return returned_path == config_path;
        }
    }
    false
}

fn handle_request(mut stream: TcpStream, config_path: &str, shutdown: &AtomicBool) {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }

    // Read headers
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(val) = trimmed.to_lowercase().strip_prefix("content-length: ") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Read body
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        let _ = reader.read_exact(&mut body);
    }

    if request_line.starts_with("POST /app-path") {
        let response_body = serde_json::to_string(config_path).unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        let _ = stream.write_all(response.as_bytes());
    } else if request_line.starts_with("POST /shutdown") {
        let body_str = String::from_utf8_lossy(&body);

        if content_length == 0 || body_str.is_empty() {
            send_response(&mut stream, 400, r#"{"error": "invalid request"}"#);
            return;
        }

        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body_str);
        match parsed {
            Err(_) => {
                send_response(&mut stream, 400, r#"{"error": "invalid JSON"}"#);
            }
            Ok(val) => {
                if let Some(ts) = val.get("timestamp").and_then(|v| v.as_str()) {
                    if timestamp::within_tolerance(ts, &timestamp::now(), 5.0) {
                        let response = r#"{"shutting_down": true}"#;
                        let http = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            response.len(),
                            response
                        );
                        let _ = stream.write_all(http.as_bytes());
                        let _ = stream.flush();
                        shutdown.store(true, Ordering::Relaxed);
                        // Give a moment for the response to flush, then exit
                        thread::sleep(Duration::from_millis(50));
                        std::process::exit(0);
                    } else {
                        send_response(&mut stream, 403, r#"{"error": "invalid timestamp"}"#);
                    }
                } else {
                    send_response(&mut stream, 403, r#"{"error": "invalid timestamp"}"#);
                }
            }
        }
    } else {
        send_response(&mut stream, 404, r#"{"error": "not found"}"#);
    }
}

fn send_response(stream: &mut TcpStream, status: u16, body: &str) {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        status,
        status_text,
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}
