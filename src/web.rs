//! Local web panel: a tiny dependency-free HTTP server that serves an embedded
//! settings page plus JSON endpoints. Uses plain `TcpListener` + threads.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use serde_json::json;

use crate::config::Settings;
use crate::connector;
use crate::engine;
use crate::state::AppContext;

pub const PANEL_PORT: u16 = 8999;
pub const PANEL_URL: &str = "http://localhost:8999";

static RUNNING: AtomicBool = AtomicBool::new(false);

const PANEL_HTML: &str = include_str!("../static/panel.html");

/// Start the panel server on a background thread (idempotent).
pub fn start(ctx: &AppContext) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let ctx = AppContext::new(
        ctx.shared.clone(),
        ctx.settings.clone(),
        ctx.config_dir.clone(),
    );
    thread::spawn(move || {
        if let Err(e) = serve(Arc::new(ctx)) {
            crate::log::write(&format!("panel server error: {e}"));
            RUNNING.store(false, Ordering::SeqCst);
        }
    });
}

fn serve(ctx: Arc<AppContext>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", PANEL_PORT))?;
    crate::log::write(&format!("web panel listening on {PANEL_URL}"));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let ctx = ctx.clone();
        thread::spawn(move || handle(stream, ctx));
    }
    Ok(())
}

fn handle(mut stream: TcpStream, ctx: Arc<AppContext>) {
    // Guard against a client that connects and stalls mid-request.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(&mut stream);

    // Read the request line + headers.
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut headers: Vec<String> = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line == "\r\n" {
            break;
        }
        headers.push(line.trim_end().to_string());
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    // Minimal body support for POST (read up to a few KB).
    let mut body_len = 0usize;
    for h in &headers {
        if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
            body_len = v.trim().parse().unwrap_or(0).min(64 * 1024);
        }
    }
    let mut body = Vec::with_capacity(body_len);
    if body_len > 0 {
        let mut chunk = vec![0u8; body_len];
        if reader.read_exact(&mut chunk).is_ok() {
            body = chunk;
        }
    }

    let response = route(&method, &path, &body, &ctx);

    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        response.status,
        response.reason,
        response.content_type,
        response.body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&response.body);
    let _ = stream.flush();
}

struct Response {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

fn route(method: &str, path: &str, body: &[u8], ctx: &Arc<AppContext>) -> Response {
    let html = |b: String| Response {
        status: 200,
        reason: "OK",
        content_type: "text/html; charset=utf-8",
        body: b.into_bytes(),
    };
    let json_response = |b: String| Response {
        status: 200,
        reason: "OK",
        content_type: "application/json; charset=utf-8",
        body: b.into_bytes(),
    };
    let not_found = || Response {
        status: 404,
        reason: "Not Found",
        content_type: "text/plain; charset=utf-8",
        body: b"not found".to_vec(),
    };

    match (method, path) {
        ("GET", "/") => html(PANEL_HTML.to_string()),

        ("GET", "/api/state") => {
            let pb = engine::snapshot(ctx);
            let source = engine::last_source(ctx);
            let latency = engine::last_latency(ctx);
            let tracker = ctx.shared.tracker.lock().unwrap();
            let auto = tracker.avg_latency();
            let update = ctx.shared.update.lock().unwrap().clone();
            let body = json!({
                "song": pb.song_name,
                "artist": pb.song_author,
                "playing": pb.is_playing,
                "progress": pb.song_progress,
                "duration": pb.song_duration,
                "line": pb.current_line.unwrap_or_default(),
                "hasLyrics": pb.has_lyrics,
                "source": source,
                "latency": latency,
                "autooffset": auto,
                "update": {
                    "latest": update.latest,
                    "message": update.message,
                },
            });
            json_response(body.to_string())
        }

        ("GET", "/api/settings") => {
            let settings = ctx.settings.read().unwrap();
            let has_token = !settings.token.is_empty();
            let mut value = serde_json::to_value(&*settings).unwrap_or_else(|_| json!({}));
            // Never hand the stored token back to the page; expose whether one
            // is configured instead.
            if let Some(obj) = value.as_object_mut() {
                obj.insert("token".into(), json!(""));
                obj.insert("hasToken".into(), json!(has_token));
            }
            json_response(value.to_string())
        }

        ("POST", "/api/settings") => {
            match serde_json::from_slice::<Settings>(body) {
                Ok(mut new_settings) => {
                    // The panel doesn't receive the stored token back, so an
                    // omitted `token` field must keep the existing one rather
                    // than clearing it.
                    if let Ok(raw) = serde_json::from_slice::<serde_json::Value>(body) {
                        match raw.get("token") {
                            Some(serde_json::Value::String(t)) => new_settings.token = t.clone(),
                            _ => {
                                new_settings.token = ctx.settings.read().unwrap().token.clone();
                            }
                        }
                    }
                    {
                        let mut settings = ctx.settings.write().unwrap();
                        *settings = new_settings.clone();
                    }
                    let _ = new_settings.save(&ctx.config_dir);
                    crate::autostart::apply(new_settings.update.auto_start);
                    json_response(json!({"ok": true}).to_string())
                }
                Err(e) => Response {
                    status: 400,
                    reason: "Bad Request",
                    content_type: "application/json; charset=utf-8",
                    body: json!({"ok": false, "error": e.to_string()}).to_string().into_bytes(),
                },
            }
        }

        ("POST", "/api/validate") => {
            let token = serde_json::from_slice::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v.get("token").and_then(|t| t.as_str()).map(String::from))
                .unwrap_or_default();
            let valid = connector::validate_token(&token);
            json_response(json!({"ok": valid}).to_string())
        }

        _ => not_found(),
    }
}
