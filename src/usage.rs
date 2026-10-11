use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::state::AppContext;

pub const USAGE_ENDPOINT: &str = "https://mewsic.yasakei.dev/v1/usage";

pub fn report_in_background(ctx: &Arc<AppContext>) {
    if !enabled(ctx) {
        return;
    }
    let ctx = Arc::clone(ctx);
    std::thread::spawn(move || {
        let payload = serde_json::json!({
            "install_id": install_id(&ctx.config_dir),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "version": env!("CARGO_PKG_VERSION"),
        });
        let started = std::time::Instant::now();
        let result = crate::net::usage_agent()
            .post(USAGE_ENDPOINT)
            .send_json(&payload);
        if let Err(e) = result {
            crate::log::write(&format!(
                "telemetry report failed ({:.0} ms): {e}",
                started.elapsed().as_millis()
            ));
        }
    });
}

fn enabled(ctx: &AppContext) -> bool {
    if std::env::var_os("MEWSIC_NO_TELEMETRY").is_some() {
        return false;
    }
    ctx.settings.read().unwrap().usage.enabled
}

fn install_id(dir: &Path) -> String {
    let path = dir.join("install_id");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim();
        if !existing.is_empty() {
            return existing.to_string();
        }
    }
    let id = format!("me{}", hex(&rand_bytes()));
    let _ = std::fs::write(&path, &id);
    id
}

fn rand_bytes() -> [u8; 16] {
    #[cfg(unix)]
    {
        if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let mut buf = [0u8; 16];
            if file.read_exact(&mut buf).is_ok() {
                return buf;
            }
        }
    }
    let mut hasher = Sha256::new();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    hasher.update(now.as_nanos().to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update((&now as *const _ as usize).to_le_bytes());
    let digest = hasher.finalize();
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&digest[..16]);
    buf
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_is_16_bytes() {
        let a = rand_bytes();
        let b = rand_bytes();
        assert_eq!(a.len(), 16);
        assert_eq!(b.len(), 16);
        assert_ne!(a, b, "two draws should differ");
    }

    #[test]
    fn install_id_is_stable_and_anonymous() {
        let dir = std::env::temp_dir().join(format!("mewsic-usage-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let id1 = install_id(&dir);
        let id2 = install_id(&dir);
        assert_eq!(id1, id2, "install id must persist");
        assert!(id1.starts_with("me"), "install id should be namespaced");
        assert!(id1.len() > 32, "install id should carry entropy");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn telemetry_defaults_to_enabled() {
        assert!(crate::config::UsageSettings::default().enabled);
    }
}
