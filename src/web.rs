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
use crate::relay;
use crate::state::AppContext;

pub const PANEL_PORT: u16 = 8999;
pub const PANEL_URL: &str = "http://localhost:8999";

static RUNNING: AtomicBool = AtomicBool::new(false);

const PANEL_HTML: &str = include_str!("../static/panel.html");

pub fn open_browser() {
    let url = PANEL_URL;
    if opener::open(url).is_err() {
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(url)
                .spawn()
                .or_else(|_| {
                    std::process::Command::new("gio")
                        .arg("open")
                        .arg(url)
                        .spawn()
                })
                .or_else(|_| {
                    std::process::Command::new("sensible-browser")
                        .arg(url)
                        .spawn()
                });
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
            // Never print to the terminal here: a live TUI screen would get
            // garbled. The TUI surfaces the failure; other callers check the
            // return value or the log.
            crate::log::write(&format!("panel server error: {e}"));
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

    // The relay WS is the only route meant to be reached through a tunnel, so
    // it bypasses the local-only host check (it has its own gate) and skips
    // body parsing — it upgrades the connection instead.
    if method == "GET" && path.split_once('?').map(|(p, _)| p).unwrap_or(&path) == relay::WS_PATH {
        crate::relay::handle_ws(ctx, stream, &headers, &path);
        return;
    }

    if let Some(response) = verify_local_request(&headers) {
        write_response(stream, response);
        return;
    }

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
    write_response(stream, response);
}

fn write_response(mut stream: TcpStream, response: Response) {
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

fn locate_header<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
    for h in headers {
        if let Some(end) = h.find(':') {
            if h[..end].trim_end().eq_ignore_ascii_case(name) {
                return Some(h[end + 1..].trim());
            }
        }
    }
    None
}

fn verify_local_request(headers: &[String]) -> Option<Response> {
    let forbidden = |reason: &'static str| Response {
        status: 403,
        reason: "Forbidden",
        content_type: "text/plain; charset=utf-8",
        body: reason.as_bytes().to_vec(),
    };

    if let Some(host) = locate_header(headers, "host") {
        if !host_is_local(host) {
            return Some(forbidden("forbidden host"));
        }
    }

    if let Some(origin) = locate_header(headers, "origin") {
        let local_origins = ["http://localhost:8999", "http://127.0.0.1:8999"];
        if !local_origins.contains(&origin) {
            return Some(forbidden("forbidden origin"));
        }
    }

    None
}

fn host_is_local(host: &str) -> bool {
    let host = host.trim();
    let name = if host.contains("::") && !host.starts_with('[') {
        host
    } else {
        match host.rsplit_once(':') {
            Some((n, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => n,
            _ => host,
        }
    };
    matches!(name, "127.0.0.1" | "localhost" | "[::1]" | "::1")
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

        ("GET", "/api/relay") => {
            let settings = ctx.settings.read().unwrap().relay.clone();
            let body = json!({
                "enabled": settings.enabled,
                "tunnel": settings.tunnel,
                "url": crate::relay::tunnel_url(),
                "error": crate::relay::tunnel_error(),
                "clients": crate::relay::clients(),
                "maxClients": settings.max_clients.clamp(1, 64),
                "hasToken": !settings.token.is_empty(),
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

    #[test]
    fn panel_supports_provider_ranking() {
        for hook in [
            "initProviderReorder",
            "collectLyricsProviders",
            "applyProviderOrder",
            "renderProviderRanks",
            "moveProvider",
        ] {
            assert!(PANEL_HTML.contains(hook), "panel must define {hook}");
        }
        assert!(PANEL_HTML.contains("dragstart"));
        assert!(
            PANEL_HTML.contains("plistOrder()"),
            "order must come from the DOM"
        );
    }

    #[test]
    fn panel_flags_experimental_providers_and_sources() {
        let lower = PANEL_HTML.to_lowercase();
        assert!(
            lower.contains("> last.fm transcript <span class=\"exp\""),
            "last.fm badge"
        );
        assert!(
            lower.contains("> youtube captions <span class=\"exp\""),
            "youtube badge"
        );
        assert!(
            lower.contains("value=\"local\">local player"),
            "local option"
        );
        assert!(
            lower.contains("— experimental"),
            "local source must be flagged in the dropdown"
        );
        assert_eq!(
            lower.matches("class=\"exp\"").count(),
            2,
            "exactly two badges"
        );
    }

    #[test]
    fn panel_provider_order_covers_every_builtin() {
        let mut seen = Vec::new();
        let needle = "class=\"pitem\" data-id=\"";
        let mut rest = PANEL_HTML;
        while let Some(i) = rest.find(needle) {
            let start = i + needle.len();
            let id: String = rest[start..].chars().take_while(|c| *c != '"').collect();
            seen.push(id.clone());
            rest = &rest[start + id.len()..];
        }
        for id in crate::config::LyricsSettings::BUILTIN {
            assert!(seen.iter().any(|s| s == id), "missing draggable row: {id}");
        }
        assert!(seen.contains(&"custom".to_string()));
        assert_eq!(seen.len(), 6, "one row per provider, no duplicates");
    }

    #[test]
    fn local_host_is_accepted() {
        assert!(host_is_local("127.0.0.1:8999"));
        assert!(host_is_local("localhost:8999"));
        assert!(host_is_local("127.0.0.1"));
        assert!(host_is_local("localhost"));
        assert!(host_is_local("[::1]:8999"));
        assert!(host_is_local("::1"));
        assert!(!host_is_local("evil.example.com"));
        assert!(!host_is_local("evil.example.com:8999"));
    }

    #[test]
    fn rebinding_and_cors_requests_are_forbidden() {
        let rebinding = vec!["Host: evil.example.com:8999".to_string()];
        let resp = verify_local_request(&rebinding).unwrap();
        assert_eq!(resp.status, 403);

        let cross_site = vec![
            "Host: 127.0.0.1:8999".to_string(),
            "Origin: https://evil.example.com".to_string(),
        ];
        let resp = verify_local_request(&cross_site).unwrap();
        assert_eq!(resp.status, 403);

        let null_origin = vec![
            "Host: 127.0.0.1:8999".to_string(),
            "Origin: null".to_string(),
        ];
        assert_eq!(verify_local_request(&null_origin).unwrap().status, 403);
    }

    #[test]
    fn same_origin_and_non_browser_requests_pass() {
        let same_origin = vec![
            "Host: localhost:8999".to_string(),
            "Origin: http://localhost:8999".to_string(),
        ];
        assert!(verify_local_request(&same_origin).is_none());

        let curl = vec!["Host: 127.0.0.1:8999".to_string()];
        assert!(verify_local_request(&curl).is_none());

        let other_spelling = vec![
            "Host: 127.0.0.1:8999".to_string(),
            "Origin: http://127.0.0.1:8999".to_string(),
        ];
        assert!(verify_local_request(&other_spelling).is_none());
    }

    #[test]
    fn foreign_tunnel_and_lan_hosts_are_not_local() {
        for host in [
            "lyrics.yasakei.dev",
            "lyrics.yasakei.dev:443",
            "example.com",
            "10.0.0.23",
            "192.168.1.10:8999",
            "",
        ] {
            assert!(
                !host_is_local(host),
                "{host:?} must not count as localhost"
            );
        }
        for host in ["127.0.0.1", "127.0.0.1:8999", "localhost", "localhost:8999", "[::1]:8999", "::1"] {
            assert!(host_is_local(host), "{host:?} must count as localhost");
        }
    }

    #[test]
    fn relay_status_route_reports_state() {
        use crate::config::RelaySettings;
        let ctx = crate::relay::tests::test_ctx(RelaySettings {
            enabled: true,
            max_clients: 3,
            token: "t".to_string(),
            tunnel: false,
            tunnel_name: String::new(),
            tunnel_host: String::new(),
        });
        let response = route("GET", "/api/relay", &[], &ctx);
        assert_eq!(response.status, 200);
        let v: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(v["enabled"], true);
        assert_eq!(v["maxClients"], 3);
        assert_eq!(v["hasToken"], true);
        assert_eq!(v["clients"], 0);
        assert_eq!(v["url"], serde_json::Value::Null, "no tunnel up yet");
    }

    #[test]
    fn panel_experiments_card_reports_relay_fields() {
        for needle in [
            "relayEnabled",
            "relayMax",
            "relayToken",
            "relayTunnel",
            "relayTunnelName",
            "relayTunnelHost",
            "relayLink",
            "relayMeta",
            "/api/relay",
            "pollRelay",
        ] {
            assert!(PANEL_HTML.contains(needle), "panel must expose {needle}");
        }
    }

    #[test]
    fn panel_hides_experiments_card_until_enabled() {
        assert!(
            PANEL_HTML.contains("id=\"expCard\" style=\"display:none\""),
            "experiments card must start hidden"
        );
        assert!(
            PANEL_HTML.contains("class=\"card span2\" aria-label=\"status\""),
            "status card must start full-width so the grid has no hole"
        );
        for needle in ["id=\"experiments\"", "updateExperimentsVisibility"] {
            assert!(PANEL_HTML.contains(needle), "panel must expose {needle}");
        }
    }

    // ── end-to-end: a real loopback WS session through handle() ────────────

    fn make_ctx(relay: crate::config::RelaySettings) -> (AppContext, Arc<std::sync::RwLock<Settings>>) {
        let settings = Arc::new(std::sync::RwLock::new(Settings {
            relay,
            ..Settings::default()
        }));
        (
            AppContext::new(crate::state::Shared::new(), Arc::clone(&settings), std::env::temp_dir()),
            settings,
        )
    }

    /// Read one *server→client* frame (unmasked, FIN set).
    fn read_ws_frame(r: &mut std::io::BufReader<TcpStream>) -> (u8, Vec<u8>) {
        let mut h = [0u8; 2];
        r.read_exact(&mut h)
            .expect("server must keep sending frames until close");
        assert_eq!(h[0] & 0x80, 0x80, "server frames must be final");
        assert_eq!(h[1] & 0x80, 0, "server→client frames must be unmasked");
        let len = match h[1] & 0x7F {
            126 => {
                let mut b = [0u8; 2];
                r.read_exact(&mut b).unwrap();
                u16::from_be_bytes(b) as usize
            }
            127 => panic!("tests never send 64-bit-length frames"),
            n => n as usize,
        };
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload).unwrap();
        (h[0] & 0x0F, payload)
    }

    /// Send one client frame — masked, as browsers must.
    fn send_client_frame(c: &TcpStream, opcode: u8, payload: &[u8]) {
        let mut c = c;
        let mask = [0x11, 0x99, 0xC3, 0x7E];
        let mut out = vec![0x80 | opcode, 0x80 | payload.len() as u8];
        out.extend_from_slice(&mask);
        out.extend(
            payload
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ mask[i % mask.len()]),
        );
        c.write_all(&out).unwrap();
    }

    /// Send a request, read the status line + head; returns (status, head, rest-reader).
    fn request_and_read_head(addr: std::net::SocketAddr, req: &str) -> (String, String, std::io::BufReader<TcpStream>) {
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(req.as_bytes()).unwrap();
        let mut r = std::io::BufReader::new(c);
        let mut status = String::new();
        r.read_line(&mut status).unwrap();
        let mut head = String::new();
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        (status, head, r)
    }

    #[test]
    fn relay_ws_end_to_end_over_loopback() {
        use crate::relay::tests::CLIENT_SERIALIZER;
        let _guard = CLIENT_SERIALIZER.lock().unwrap();
        let (ctx, settings) = make_ctx(crate::config::RelaySettings::default());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let ctx = Arc::new(ctx);
        let server_ctx = Arc::clone(&ctx);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let conn_ctx = Arc::clone(&server_ctx);
                thread::spawn(move || handle(stream, conn_ctx));
            }
        });

        // 1. relay disabled + NON-LOCAL host (like a tunnel traversal): must
        //    bypass the local-host check (404 from the gate, not 403 host)
        let req = "GET /song/ws HTTP/1.1\r\nHost: lyrics.yasakei.dev\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";
        let (status, _head, mut r) = request_and_read_head(addr, req);
        assert!(status.starts_with("HTTP/1.1 404"), "got {status}");
        let mut body = String::new();
        std::io::Read::read_to_string(&mut r, &mut body).unwrap();
        assert_eq!(body, "relay disabled");

        // 2. enable the relay live and reconnect through a tunnel-style host
        settings.write().unwrap().relay.enabled = true;
        let (status, head, mut r) = request_and_read_head(addr, req);
        assert!(status.starts_with("HTTP/1.1 101"), "got {status}");
        assert!(
            head.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
            "handshake must echo the RFC accept key"
        );
        assert!(head.contains("Upgrade: websocket"));

        // 3. first push: the current snapshot arrives as a JSON text frame
        let (op, payload) = read_ws_frame(&mut r);
        assert_eq!(op, 0x1);
        let first: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert!(first.get("song").is_some(), "snapshot shape: {first}");
        assert!(first["ts"].as_u64().unwrap() > 0);

        // 4. client ping → server pong, unmasked payload preserved
        send_client_frame(r.get_mut(), 0x9, b"hit");
        let (op, pong) = read_ws_frame(&mut r);
        assert_eq!(op, 0xA, "server must answer pings with pongs");
        assert_eq!(pong, b"hit");

        // 5. flood: over the inbound rate cap → server closes with 1000
        for _ in 0..9 {
            send_client_frame(r.get_mut(), 0x1, b"x");
        }
        let (op, closed) = read_ws_frame(&mut r);
        assert_eq!(op, 0x8, "rate offender must get a close frame");
        assert_eq!(
            closed,
            [0x03, 0xE8],
            "close payload 1000 (a misbehaving client can't amp up)"
        );
        drop(r);
    }
}

