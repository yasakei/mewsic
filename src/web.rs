use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::json;

use crate::config::Settings;
use crate::connector;
use crate::engine;
use crate::state::AppContext;

pub const PANEL_PORT: u16 = 8999;
pub const PANEL_URL: &str = "http://localhost:8999";

static RUNNING: AtomicBool = AtomicBool::new(false);

const PANEL_HTML: &str = include_str!("../static/panel.html");

pub fn open_browser() {
    // OS-specific handler via `opener` (xdg-open/open/start) — best-effort, no crash on headless
    let url = PANEL_URL;
    if opener::open(url).is_err() {
        // Fallback manual OS handlers if opener fails (e.g. minimal container)
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(url)
                .spawn()
                .or_else(|_| std::process::Command::new("gio").arg("open").arg(url).spawn())
                .or_else(|_| std::process::Command::new("sensible-browser").arg(url).spawn());
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(url).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", url])
                .spawn();
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        }
    }
}

pub fn start(ctx: &AppContext) -> bool {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return true;
    }
    let listener = match TcpListener::bind(("127.0.0.1", PANEL_PORT)) {
        Ok(listener) => listener,
        Err(e) => {
            RUNNING.store(false, Ordering::SeqCst);
            crate::log::write(&format!("panel server error: {e}"));
            eprintln!("could not start web panel at {PANEL_URL}: {e}");
            return false;
        }
    };
    let ctx = AppContext::new(
        ctx.shared.clone(),
        ctx.settings.clone(),
        ctx.config_dir.clone(),
    );
    thread::spawn(move || {
        if let Err(e) = serve(listener, Arc::new(ctx)) {
            crate::log::write(&format!("panel server error: {e}"));
            RUNNING.store(false, Ordering::SeqCst);
        }
    });
    true
}

fn serve(listener: TcpListener, ctx: Arc<AppContext>) -> std::io::Result<()> {
    crate::log::write(&format!("web panel listening on {PANEL_URL}"));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let ctx = ctx.clone();
        thread::spawn(move || handle(stream, ctx));
    }
    Ok(())
}

fn handle(mut stream: TcpStream, ctx: Arc<AppContext>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(&mut stream);

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
            if let Some(obj) = value.as_object_mut() {
                obj.insert("token".into(), json!(""));
                obj.insert("hasToken".into(), json!(has_token));
            }
            json_response(value.to_string())
        }

        ("POST", "/api/settings") => match serde_json::from_slice::<Settings>(body) {
            Ok(mut new_settings) => {
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
                body: json!({"ok": false, "error": e.to_string()})
                    .to_string()
                    .into_bytes(),
            },
        },

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_script_does_not_bind_removed_auto_check_control() {
        assert!(!PANEL_HTML.contains("autoCheck"));
        assert!(PANEL_HTML.contains("pollState();"));
    }
}
