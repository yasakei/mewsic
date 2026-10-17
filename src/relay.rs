//! Lyrics relay (experimental) — a push-only WebSocket on the local web panel
//! that remote pages can follow the live song + lyric line through.
//!
//! Trigger: the Experiments card in the panel, or `[relay]` in settings.toml.
//!
//! Delivery: `cloudflared` quick tunnels forward to this panel — but only
//! `/song/ws` is reachable through them. Every other route (token, settings,
//! saves) remains guarded by the local-only host check in web.rs, so the
//! tunnel can never expose the control surface.
//!
//! Anti-nuke limits:
//! - global cap on concurrent clients (`relay.max_clients`, default 8) → 429
//! - optional shared `?token=` on the handshake → 401 (empty token = open)
//! - per-client inbound message rate cap → close 1008
//! - inbound frame payload cap (4 KiB) → close 1009
//! - server→client JSON snapshots only (~2/s while playing); a client that
//!   speaks anything beyond pings gets closed.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::state::AppContext;

pub const WS_PATH: &str = "/song/ws";
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_FRAME_PAYLOAD: usize = 4096;
/// Inbound messages per client per window; a legit client only pongs.
const RATE_MAX_MSGS: usize = 8;
const RATE_WINDOW_MS: u64 = 10_000;
/// Snapshot cadence; a playing track changes progress every second, so this
/// yields at most ~2 text frames/sec per client.
const PUSH_INTERVAL_MS: u64 = 400;
/// Half the keepalive of typical intermediaries; browsers don't ping us back.
const LIVENESS_PING_SECS: u64 = 25;

static CLIENTS: AtomicUsize = AtomicUsize::new(0);
static TUNNEL_URL: Mutex<Option<String>> = Mutex::new(None);
static TUNNEL_ERROR: Mutex<Option<String>> = Mutex::new(None);

pub fn clients() -> usize {
    CLIENTS.load(Ordering::SeqCst)
}

pub fn tunnel_url() -> Option<String> {
    TUNNEL_URL.lock().unwrap().clone()
}

/// Last tunnel failure (cloudflared missing, exited early, …) for the panel.
pub fn tunnel_error() -> Option<String> {
    TUNNEL_ERROR.lock().unwrap().clone()
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// RFC 6455 Sec-WebSocket-Accept: base64(SHA1(key + GUID)).
fn ws_accept(key: &str) -> String {
    use base64::Engine as _;
    use sha1::{Digest as _, Sha1};
    let mut sha = Sha1::new();
    sha.update(key.as_bytes());
    sha.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(sha.finalize())
}

/// Sliding-window inbound message rate for a single client.
#[derive(Default)]
struct MsgRate {
    stamps: Vec<Instant>,
}

impl MsgRate {
    /// true = allowed.
    fn allow(&mut self) -> bool {
        let now = Instant::now();
        self.stamps
            .retain(|t| now.duration_since(*t) < Duration::from_millis(RATE_WINDOW_MS));
        if self.stamps.len() >= RATE_MAX_MSGS {
            return false;
        }
        self.stamps.push(now);
        true
    }
}

/// Encode a server frame (unmasked, as required for server→client).
fn encode_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len();
    let mut out = Vec::with_capacity(len + 10);
    out.push(0x80 | opcode);
    match len {
        0..=125 => out.push(len as u8),
        126..=0xFFFF => {
            out.push(126);
            out.extend_from_slice(&(len as u16).to_be_bytes());
        }
        _ => {
            out.push(127);
            out.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

/// Read one client frame. Returns None on EOF/reset (peer went away — not an
/// error), Some((opcode, payload)) otherwise. Enforces the payload cap.
fn read_frame(reader: &mut impl Read) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut header = [0u8; 2];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if is_quiet(&e) => return Ok(None),
        Err(e) => return Err(e),
    }
    if header[0] & 0x80 != 0x80 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "fragmented frames not supported",
        ));
    }
    let opcode = header[0] & 0x0F;
    let masked = header[1] & 0x80 != 0;
    let len = match (header[1] & 0x7F) as u64 {
        126 => {
            let mut b = [0u8; 2];
            reader.read_exact(&mut b)?;
            u16::from_be_bytes(b) as u64
        }
        127 => {
            let mut b = [0u8; 8];
            reader.read_exact(&mut b)?;
            u64::from_be_bytes(b)
        }
        n => n,
    };
    if len > MAX_FRAME_PAYLOAD as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut payload = vec![0u8; len as usize];
    if len > 0 {
        reader.read_exact(&mut payload)?;
    }
    if masked {
        let mut key = [0u8; 4];
        reader.read_exact(&mut key)?;
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= key[i % 4];
        }
    }
    Ok(Some((opcode, payload)))
}

fn is_quiet(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::TimedOut
    )
}

/// Aggregated snapshot the relay pushes. Field names mirror the panel's
/// /api/state so consumers parse once.
fn snapshot_json(ctx: &AppContext) -> String {
    let pb = ctx.shared.playback.lock().unwrap();
    let body = json!({
        "song": pb.song_name,
        "artist": pb.song_author,
        "playing": pb.is_playing,
        "progress": pb.song_progress,
        "duration": pb.song_duration,
        "line": pb.current_line.as_ref().map(|l| l.text.clone()).unwrap_or_default(),
        "hasLyrics": pb.has_lyrics,
        "source": crate::engine::last_source(ctx),
        "ts": unix_ms(),
    });
    body.to_string()
}

/// Change detector for push cadence: progress at whole-second resolution so
/// we push on state/line changes and once per second while playing, not on
/// every 400ms tick.
fn fingerprint(payload: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    format!(
        "{}|{}|{}|{}|{}",
        v["song"].as_str().unwrap_or_default(),
        v["line"].as_str().unwrap_or_default(),
        v["playing"].as_bool().unwrap_or(false),
        v["duration"].as_u64().unwrap_or_default(),
        v["progress"].as_u64().unwrap_or_default() / 1000,
    )
}

/// Admission gate. Ok = proceed with the handshake; Err = (status, body).
/// The optional token travels in the tunnel URL query (`?token=...`).
fn gate(ctx: &AppContext, query: Option<&str>) -> Result<(), (u16, &'static str)> {
    let settings = ctx.settings.read().unwrap().clone();
    let relay = settings.relay;
    if !relay.enabled {
        return Err((404, "relay disabled"));
    }
    if !relay.token.is_empty() {
        let supplied = query
            .and_then(|q| {
                q.split('&')
                    .filter_map(|kv| kv.split_once('='))
                    .find_map(|(k, v)| (k == "token").then(|| v.to_string()))
            })
            .unwrap_or_default();
        if supplied != relay.token {
            return Err((401, "invalid token"));
        }
    }
    let max = relay.max_clients.clamp(1, 64) as usize;
    let mut current = CLIENTS.load(Ordering::SeqCst);
    loop {
        if current >= max {
            return Err((429, "connection limit"));
        }
        match CLIENTS.compare_exchange(
            current,
            current + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return Ok(()),
            Err(actual) => current = actual,
        }
    }
}

fn plain_response(stream: &mut TcpStream, status: u16, reason: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len(),
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.flush();
}

/// Entry from web.rs: owns the stream until the session ends (caller returns).
pub fn handle_ws(ctx: Arc<AppContext>, mut stream: TcpStream, headers: &[String], path: &str) {
    let query = path.split_once('?').map(|(_, q)| q);
    if let Err((status, body)) = gate(&ctx, query) {
        let reason = match status {
            404 => "Not Found",
            401 => "Unauthorized",
            _ => "Too Many Requests",
        };
        plain_response(&mut stream, status, reason, body);
        return;
    }

    let key = headers.iter().find_map(|h| {
        let (name, value) = h.split_once(':')?;
        name.trim_end()
            .eq_ignore_ascii_case("sec-websocket-key")
            .then(|| value.trim().to_string())
    });
    let Some(key) = key.filter(|k| k.len() >= 16) else {
        plain_response(&mut stream, 400, "Bad Request", "websocket handshake expected");
        return;
    };

    let accept = ws_accept(&key);
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n",
    );
    let writer = Arc::new(Mutex::new(stream));
    {
        let mut w = writer.lock().unwrap();
        let _ = w.write_all(head.as_bytes());
        let _ = w.flush();
    }

    #[derive(Clone)]
    struct Sender(Arc<Mutex<TcpStream>>);
    impl Sender {
        fn send(&self, opcode: u8, payload: &[u8]) -> bool {
            let mut w = self.0.lock().unwrap();
            w.write_all(&encode_frame(opcode, payload))
                .and_then(|_| w.flush())
                .is_ok()
        }
    }
    let sender = Sender(Arc::clone(&writer));

    // Reader thread: handles pings/close/rate-limits on the client side while
    // the pusher loop below owns the periodic snapshots.
    let stop = Arc::new(AtomicBool::new(false));
    let reader_stop = Arc::clone(&stop);
    let raw_reader = match writer.lock().unwrap().try_clone() {
        Ok(r) => r,
        Err(_) => {
            CLIENTS.fetch_sub(1, Ordering::SeqCst);
            return;
        }
    };
    let reader_sender = sender.clone();
    let reader_thread = std::thread::spawn(move || {
        let send = reader_sender;
        let mut rate = MsgRate::default();
        let mut raw = BufReader::new(raw_reader);
        loop {
            if reader_stop.load(Ordering::SeqCst) {
                return;
            }
            match read_frame(&mut raw) {
                Ok(None) => return,
                Ok(Some((opcode, payload))) => match opcode {
                    0x9 => {
                        // ping → pong, verbatim payload
                        if !rate.allow() {
                            break;
                        }
                        let _ = send.send(0xA, &payload);
                    }
                    0xA => {}     // pong: link is warm, nothing to do
                    0x8 => return, // client hangup
                    _ => {
                        // text/binary/unknown from a client: rate limited
                        if !rate.allow() {
                            break;
                        }
                    }
                },
                Err(_) => return,
            }
        }
        reader_stop.store(true, Ordering::SeqCst);
    });

    // Pusher loop: send on state change, ping for NAT/OSS liveness.
    let mut last_fingerprint = String::new();
    let mut last_ping = Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) {
            let _ = sender.send(0x8, &[0x03, 0xE8]); // close 1000 normal closure
            break;
        }
        let payload = snapshot_json(&ctx);
        let fp = fingerprint(&payload);
        if fp != last_fingerprint {
            last_fingerprint = fp;
            if !sender.send(0x1, payload.as_bytes()) {
                stop.store(true, Ordering::SeqCst);
                break;
            }
        }
        if last_ping.elapsed() >= Duration::from_secs(LIVENESS_PING_SECS) {
            last_ping = Instant::now();
            if !sender.send(0x9, b"ping") {
                stop.store(true, Ordering::SeqCst);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(PUSH_INTERVAL_MS));
    }
    stop.store(true, Ordering::SeqCst);
    let _ = reader_thread.join();
    CLIENTS.fetch_sub(1, Ordering::SeqCst);
}

/// Tunnel babysitter: keeps a quick cloudflared tunnel up while
/// settings.relay.enabled && relay.tunnel, restarting it if it dies. Exits
/// when the main quit flag drops so the child can't outlive Mewsic.
pub fn start(ctx: &AppContext, quit: Arc<AtomicBool>) {
    let ctx = Arc::new(AppContext::new(
        ctx.shared.clone(),
        ctx.settings.clone(),
        ctx.config_dir.clone(),
    ));
    std::thread::spawn(move || {
        let enabled = ctx.settings.read().unwrap().relay.enabled;
        if enabled {
            crate::log::write("relay: enabled — serving /song/ws with mewsic");
        }
        let mut child: Option<Child> = None;
        loop {
            if quit.load(Ordering::SeqCst) {
                if let Some(c) = child.as_mut() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                *TUNNEL_URL.lock().unwrap() = None;
                return;
            }
            let settings = ctx.settings.read().unwrap().clone();
            let named = settings.relay.tunnel_name.trim().to_string();
            let host = normalized_host(&settings.relay.tunnel_host);
            // named tunnel works with or without the quick-tunnel toggle on
            let wanted = settings.relay.enabled
                && (settings.relay.tunnel || !named.is_empty());
            if !wanted || tunnel_url().is_some() {
                if sleep_interruptible(&quit, Duration::from_secs(5)) {
                    return;
                }
                continue;
            }
            child = match spawn_cloudflared(&named) {
                Ok(c) => {
                    *TUNNEL_ERROR.lock().unwrap() = None;
                    if !named.is_empty() && host.is_empty() {
                        // it will still serve traffic; we just can't display it
                        *TUNNEL_ERROR.lock().unwrap() = Some(
                            "named tunnel running — set Custom host to display the link"
                                .to_string(),
                        );
                    }
                    Some(c)
                }
                Err(e) => {
                    let msg = if e.kind() == std::io::ErrorKind::NotFound {
                        "cloudflared not found — install it or add it to PATH".to_string()
                    } else {
                        format!("cloudflared failed to start: {e}")
                    };
                    *TUNNEL_ERROR.lock().unwrap() = Some(msg.clone());
                    crate::log::write(&format!("relay: {msg} ({e})"));
                    if sleep_interruptible(&quit, Duration::from_secs(60)) {
                        return;
                    }
                    continue;
                }
            };
            if let Some(c) = child.as_mut() {
                if !named.is_empty() && !host.is_empty() {
                    // named tunnels don't print their hostname; the link is
                    // whatever you routed DNS to. Set it up front.
                    let url = format!("https://{host}");
                    crate::log::write(&format!("relay: named tunnel at {url}"));
                    *TUNNEL_URL.lock().unwrap() = Some(url);
                }
                // Drain BOTH pipes: cloudflared logs (and the quick-tunnel URL)
                // go to stderr, and an unread pipe would fill up and stall it.
                let drain_out = c.stdout.take();
                let drain_err = c.stderr.take();
                let mut readers = Vec::new();
                if let Some(out) = drain_out {
                    readers.push(std::thread::spawn(move || scan_stream(out)));
                }
                if let Some(err) = drain_err {
                    readers.push(std::thread::spawn(move || scan_stream(err)));
                }
                let _ = c.wait(); // block until the tunnel process dies
                for r in readers {
                    let _ = r.join();
                }
                if tunnel_url().is_none() && tunnel_error().is_none() {
                    *TUNNEL_ERROR.lock().unwrap() =
                        Some("tunnel exited before publishing a link".to_string());
                }
                crate::log::write("relay: cloudflared exited");
                *TUNNEL_URL.lock().unwrap() = None;
                // brief cooldown before a respawn attempt
                if sleep_interruptible(&quit, Duration::from_secs(10)) {
                    if let Some(c) = child.as_mut() {
                        let _ = c.kill();
                    }
                    return;
                }
            }
        }
    });
}

fn spawn_cloudflared(tunnel_name: &str) -> std::io::Result<Child> {
    let mut cmd = Command::new("cloudflared");
    if tunnel_name.is_empty() {
        // throwaway quick tunnel: URL rotates every run
        cmd.args(["tunnel", "--url", "http://127.0.0.1:8999", "--no-autoupdate"]);
    } else {
        // customer's own named tunnel with a stable custom domain; credentials
        // are read from the default ~/.cloudflared config dir
        cmd.args([
            "tunnel",
            "run",
            "--url",
            "http://127.0.0.1:8999",
            tunnel_name,
        ]);
    }
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        // Don't flash a console window along with the app.
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
}

/// Forgiving hostname parse: strips scheme + trailing slash.
fn normalized_host(raw: &str) -> String {
    raw.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_lowercase()
}

/// Consumes one pipe end of a tunnel process until EOF, harvesting a
/// trycloudflare.com URL if one shows up. Keeps reading after the URL so the
/// pipe never fills up and blocks cloudflared.
fn scan_stream<R: Read>(pipe: R) {
    for line in BufReader::new(pipe).lines().map_while(|l| l.ok()) {
        if !line.contains("trycloudflare.com") {
            continue;
        }
        let Some(start) = line.rfind("https://") else {
            continue;
        };
        let rest = &line[start + 8..];
        let end = rest
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '.'))
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        let url = format!("https://{}", &rest[..end]);
        if !url.ends_with(".trycloudflare.com") {
            continue;
        }
        let mut guard = TUNNEL_URL.lock().unwrap();
        if guard.is_none() {
            crate::log::write(&format!("relay: tunnel ready at {url}"));
            *guard = Some(url);
        }
    }
}

fn sleep_interruptible(quit: &AtomicBool, d: Duration) -> bool {
    let step = Duration::from_millis(250);
    let mut waited = Duration::ZERO;
    while waited < d {
        if quit.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(step);
        waited += step;
    }
    quit.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6455 §1.3 worked example.
    #[test]
    fn accept_key_matches_rfc_example() {
        assert_eq!(
            ws_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn text_frames_encode_unmasked() {
        let frame = encode_frame(0x1, b"Hi");
        assert_eq!(frame, [0x81, 0x02, b'H', b'i']);

        let frame = encode_frame(0x1, &[b'a'; 700]);
        assert_eq!(&frame[..4], &[0x81, 126, 0x02, 0xBC]);
    }

    #[test]
    fn rate_limits_inside_window() {
        let mut rate = MsgRate::default();
        for _ in 0..RATE_MAX_MSGS {
            assert!(rate.allow(), "first burst must pass");
        }
        assert!(!rate.allow(), "burst over cap must be rejected");
    }

    #[test]
    fn fingerprint_ignores_subsecond_progress() {
        let a = fingerprint(r#"{"song":"s","line":"l","playing":true,"duration":100,"progress":1200}"#);
        let b = fingerprint(r#"{"song":"s","line":"l","playing":true,"duration":100,"progress":1600}"#);
        assert_eq!(a, b, "subsecond progress must not retrigger pushes");
    }
}
