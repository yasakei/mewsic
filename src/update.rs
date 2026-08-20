//! Auto-updater.
//!
//! Checks the GitHub releases of the CI-built artifacts for a newer build of
//! this platform, downloads it, verifies it against the release's
//! `checksums.txt`, and replaces the running binary in place. The check runs
//! in the background when `update.auto_check` is on, or on demand via
//! `mewsic update` / `mewsic update check`.
//!
//! Replacing the executable is safe while running: on Unix the old inode keeps
//! executing until the process exits; on Windows renaming an open executable
//! is permitted, so the same rename-away trick works. Installers (NSIS on
//! Windows) are the fallback when the install directory needs elevation.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::state::{AppContext, UpdateState};

const USER_AGENT: &str = "mewsic-updater";
const LATEST_API: &str = "https://api.github.com/repos/yasakei/mewsic/releases/latest";
/// Asset name of the sha256 manifest uploaded with every release.
const CHECKSUM_ASSET: &str = "checksums.txt";
/// Windows-only fallback: the NSIS installer rebuilt with each release.
#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_INSTALLER_ASSET: &str = "mewsic-setup.exe";
/// How often the background checker re-queries GitHub.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Name of the artifact GitHub publishes for this platform, or `None` when the
/// current platform isn't released.
pub fn asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("mewsic-x86_64-pc-windows-msvc.exe"),
        ("linux", "x86_64") => Some("mewsic-x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("mewsic-aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("mewsic-x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("mewsic-aarch64-apple-darwin"),
        _ => None,
    }
}

/// A GitHub release as served by `/releases/latest`.
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

impl Release {
    /// Tag without the leading `v` ("v1.0.3" -> "1.0.3").
    fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// `true` when `next` is strictly newer than `current`. Compares numeric
/// components (1.0.10 > 1.0.2); a leading `v` and suffixes are ignored.
pub fn is_newer(next: &str, current: &str) -> bool {
    let n = version_parts(next);
    let c = version_parts(current);
    for i in 0..n.len().max(c.len()) {
        let a = n.get(i).copied().unwrap_or(0);
        let b = c.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

/// Leading decimal numbers of each `.`/`-`/`+`-separated component, so
/// pre-release suffixes and build metadata don't skew the comparison.
fn version_parts(v: &str) -> Vec<u64> {
    v.trim_start_matches('v')
        .split(['.', '-', '+'])
        .map(|s| {
            s.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap_or(0))
        .collect()
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .timeout_write(Duration::from_secs(30))
        .build()
}

fn get(url: &str) -> Result<ureq::Response, String> {
    agent()
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())
}

/// The newest release, or `None` when the repository has no releases yet.
pub fn latest_release() -> Result<Option<Release>, String> {
    let resp = get(LATEST_API)?;
    match resp.status() {
        200 => serde_json::from_reader(resp.into_reader())
            .map(Some)
            .map_err(|e| format!("bad release payload: {e}")),
        404 => Ok(None),
        s => Err(format!("github api returned HTTP {s}")),
    }
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let resp = get(url)?;
    if resp.status() != 200 {
        return Err(format!("download returned HTTP {}", resp.status()));
    }
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| format!("download failed: {e}"))?;
    Ok(buf)
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Look up the expected sha256 of `asset` in a `checksums.txt` manifest
/// (one `<hash>  <name>` per line, as written by the release workflow).
fn expected_sha256(checksums: &str, asset: &str) -> Option<String> {
    checksums.lines().find_map(|line| {
        let mut it = line.split_whitespace();
        match (it.next(), it.next()) {
            (Some(hash), Some(name)) if name == asset => {
                Some(hash.trim().to_ascii_lowercase())
            }
            _ => None,
        }
    })
}

fn update_dir(ctx: &AppContext) -> PathBuf {
    ctx.config_dir.join("update")
}

/// Outcome of trying to install a verified update.
#[derive(Debug)]
pub enum ApplyOutcome {
    /// New binary is in place on disk; effective on the next launch.
    Replaced,
    /// Windows: the elevated NSIS installer was launched (manual runs only).
    #[cfg_attr(not(windows), allow(dead_code))]
    NeedsElevation,
    /// Downloaded + verified, kept at `PathBuf` awaiting a manual install.
    Staged(PathBuf),
}

/// Download the platform asset of `release`, verify it against the release's
/// `checksums.txt`, and stage it in the config dir. Returns the staged path.
fn download_and_verify(
    ctx: &AppContext,
    release: &Release,
    asset_name: &str,
) -> Result<PathBuf, String> {
    let checksums = match release.asset(CHECKSUM_ASSET) {
        Some(a) => String::from_utf8(download(&a.browser_download_url)?)
            .map_err(|e| format!("bad checksums.txt: {e}"))?,
        None => return Err("release is missing checksums.txt".to_string()),
    };
    let expected = expected_sha256(&checksums, asset_name)
        .ok_or_else(|| format!("no checksum for {asset_name} in checksums.txt"))?;

    let asset = release
        .asset(asset_name)
        .ok_or_else(|| format!("release has no {asset_name} asset"))?;
    let data = download(&asset.browser_download_url)?;
    let actual = sha256_hex(&data);
    if actual != expected {
        return Err(format!(
            "checksum mismatch for {asset_name} (got {actual}, expected {expected})"
        ));
    }

    let dir = update_dir(ctx);
    let _ = std::fs::create_dir_all(&dir);
    let staged = dir.join(format!("{asset_name}.{}", release.version()));
    std::fs::write(&staged, &data).map_err(|e| format!("could not stage the update: {e}"))?;
    Ok(staged)
}

/// Swap `staged` in over the running executable: move the current binary to a
/// `.old` sibling, copy the new one into place, then drop the `.old`.
fn install_in_place(exe: &Path, staged: &Path) -> Result<(), String> {
    let dir = exe
        .parent()
        .ok_or_else(|| "cannot locate the executable's directory".to_string())?;
    let old = dir.join(format!(
        "{}.old",
        exe.file_name().unwrap_or_default().to_string_lossy()
    ));

    // Probe directory writability without touching the running binary.
    let probe = dir.join(".mewsic-update-probe");
    std::fs::write(&probe, b"")
        .map_err(|e| format!("install directory is not writable ({e})"))?;
    let _ = std::fs::remove_file(&probe);

    std::fs::rename(exe, &old).map_err(|e| format!("could not move the current binary aside: {e}"))?;
    if let Err(e) = std::fs::copy(staged, exe) {
        // Roll the old binary back before reporting failure.
        let _ = std::fs::rename(&old, exe);
        return Err(format!("could not copy the update into place: {e}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_file(&old);
    Ok(())
}

/// What to do when [`install_in_place`] can't write to the install directory.
fn fallback_install(
    ctx: &AppContext,
    release: &Release,
    manual: bool,
    staged: &Path,
) -> Result<ApplyOutcome, String> {
    #[cfg(windows)]
    {
        if !manual {
            // Never pop a UAC prompt from a background check.
            return Ok(ApplyOutcome::Staged(staged.to_path_buf()));
        }
        let checksums = match release.asset(CHECKSUM_ASSET) {
            Some(a) => String::from_utf8(download(&a.browser_download_url)?)
                .map_err(|e| format!("bad checksums.txt: {e}"))?,
            None => return Err("release is missing checksums.txt".to_string()),
        };
        let expected = expected_sha256(&checksums, WINDOWS_INSTALLER_ASSET).ok_or_else(|| {
            format!("no checksum for {WINDOWS_INSTALLER_ASSET} in checksums.txt")
        })?;
        let asset = release
            .asset(WINDOWS_INSTALLER_ASSET)
            .ok_or_else(|| format!("release has no {WINDOWS_INSTALLER_ASSET} asset"))?;
        let data = download(&asset.browser_download_url)?;
        if sha256_hex(&data) != expected {
            return Err(format!("checksum mismatch for {WINDOWS_INSTALLER_ASSET}"));
        }
        let dest = update_dir(ctx).join(format!("{WINDOWS_INSTALLER_ASSET}.{}", release.version()));
        std::fs::write(&dest, &data).map_err(|e| format!("could not stage the installer: {e}"))?;
        std::process::Command::new(&dest)
            .arg("/S") // NSIS silent install — the OS shows the UAC prompt.
            .spawn()
            .map_err(|e| format!("could not launch the installer: {e}"))?;
        Ok(ApplyOutcome::NeedsElevation)
    }
    #[cfg(not(windows))]
    {
        let _ = (ctx, release, manual);
        Ok(ApplyOutcome::Staged(staged.to_path_buf()))
    }
}

/// Check GitHub for a newer release and install it when one exists. `manual`
/// is set for `mewsic update` (allows elevating via the Windows installer).
pub fn run_update(ctx: &AppContext, manual: bool) -> UpdateState {
    let current = env!("CARGO_PKG_VERSION");
    let Some(asset) = asset_name() else {
        return record(
            ctx,
            UpdateState {
                latest: None,
                message: format!(
                    "auto-update is not available on {}/{}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ),
            },
        );
    };

    let state = match latest_release() {
        Ok(Some(release)) => {
            let version = release.version().to_string();
            if !is_newer(&version, current) {
                UpdateState {
                    latest: None,
                    message: format!("up to date (v{current})"),
                }
            } else {
                let outcome = (|| -> Result<ApplyOutcome, String> {
                    let staged = download_and_verify(ctx, &release, asset)?;
                    match install_in_place(&current_exe()?, &staged) {
                        Ok(()) => Ok(ApplyOutcome::Replaced),
                        Err(err) => fallback_install(ctx, &release, manual, &staged)
                            .map_err(|fb| format!("{err}; {fb}")),
                    }
                })();
                match outcome {
                    Ok(ApplyOutcome::Replaced) => UpdateState {
                        latest: Some(version.clone()),
                        message: format!("v{version} installed — restart mewsic to use it"),
                    },
                    Ok(ApplyOutcome::NeedsElevation) => UpdateState {
                        latest: Some(version.clone()),
                        message: format!(
                            "v{version}: elevated installer launched, follow the UAC prompt"
                        ),
                    },
                    Ok(ApplyOutcome::Staged(path)) => {
                        let exe = std::env::current_exe()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        #[cfg(windows)]
                        let message = format!(
                            "v{version} downloaded to {}. Install it over {} (or run `mewsic update` as an admin).",
                            path.display(),
                            exe
                        );
                        #[cfg(not(windows))]
                        let message = format!(
                            "v{version} downloaded to {}. Install it with:\n  sudo install -m 755 {} {}",
                            path.display(),
                            path.display(),
                            exe
                        );
                        UpdateState {
                            latest: Some(version.clone()),
                            message,
                        }
                    }
                    Err(e) => UpdateState {
                        latest: Some(version.clone()),
                        message: format!("v{version} available but the install failed: {e}"),
                    },
                }
            }
        }
        Ok(None) => UpdateState {
            latest: None,
            message: "no releases found".to_string(),
        },
        Err(e) => UpdateState {
            latest: None,
            message: format!("update check failed: {e}"),
        },
    };
    record(ctx, state)
}

/// `mewsic update check`: report whether a newer release exists, download
/// nothing.
pub fn check_only(ctx: &AppContext) -> UpdateState {
    let current = env!("CARGO_PKG_VERSION");
    let state = match asset_name() {
        None => UpdateState {
            latest: None,
            message: format!(
                "auto-update is not available on {}/{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        },
        Some(_) => match latest_release() {
            Ok(Some(release)) if is_newer(release.version(), current) => UpdateState {
                latest: Some(release.version().to_string()),
                message: format!(
                    "v{} is available (you're on v{current}) — run `mewsic update` to install",
                    release.version()
                ),
            },
            Ok(Some(_)) => UpdateState {
                latest: None,
                message: format!("up to date (v{current})"),
            },
            Ok(None) => UpdateState {
                latest: None,
                message: "no releases found".to_string(),
            },
            Err(e) => UpdateState {
                latest: None,
                message: format!("update check failed: {e}"),
            },
        },
    };
    record(ctx, state)
}

fn record(ctx: &AppContext, state: UpdateState) -> UpdateState {
    *ctx.shared.update.lock().unwrap() = state.clone();
    crate::log::write(&format!("update: {}", state.message.replace('\n', " ")));
    state
}

fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("cannot locate the executable: {e}"))
}

/// Run the updater in a detached thread: one check right after startup, then
/// every [`CHECK_INTERVAL`] while `update.auto_check` stays enabled. The loop
/// is killed when the process exits.
pub fn spawn_checker(ctx: Arc<AppContext>) {
    thread::spawn(move || {
        // A short stagger so startup (settings load, token fetch) settles
        // before we touch the network — but the check still runs moments
        // after mewsic opens.
        thread::sleep(Duration::from_secs(2));
        loop {
            if ctx.settings.read().unwrap().update.auto_check {
                run_update(&ctx, false);
            }
            thread::sleep(CHECK_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_handles_components_numerically() {
        assert!(is_newer("1.0.3", "1.0.2"));
        assert!(is_newer("v1.2.0", "1.1.9"));
        assert!(is_newer("1.0.10", "1.0.9"));
        assert!(!is_newer("1.0.2", "1.0.2"));
        assert!(!is_newer("1.0.2", "1.0.3"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("1.5", "1.5.0"));
        assert!(is_newer("1.5.1", "1.5"));
    }

    #[test]
    fn version_suffixes_are_ignored() {
        assert!(!is_newer("1.0.2-beta", "1.0.2"));
        assert!(is_newer("1.0.3-rc.1", "1.0.2"));
    }

    #[test]
    fn asset_is_known_on_supported_platforms() {
        let name = asset_name();
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => assert_eq!(name, Some("mewsic-x86_64-pc-windows-msvc.exe")),
            ("linux", "x86_64") => assert_eq!(name, Some("mewsic-x86_64-unknown-linux-gnu")),
            ("linux", "aarch64") => assert_eq!(name, Some("mewsic-aarch64-unknown-linux-gnu")),
            ("macos", "x86_64") => assert_eq!(name, Some("mewsic-x86_64-apple-darwin")),
            ("macos", "aarch64") => assert_eq!(name, Some("mewsic-aarch64-apple-darwin")),
            _ => assert_eq!(name, None),
        }
    }

    #[test]
    fn checksum_manifest_lookup() {
        let manifest =
            "abc123  mewsic-x86_64-unknown-linux-gnu\ndef456  mewsic-setup.exe\n";
        assert_eq!(
            expected_sha256(manifest, "mewsic-x86_64-unknown-linux-gnu"),
            Some("abc123".into())
        );
        assert_eq!(
            expected_sha256(manifest, "mewsic-setup.exe"),
            Some("def456".into())
        );
        assert_eq!(
            expected_sha256(manifest, "mewsic-aarch64-unknown-linux-gnu"),
            None
        );
    }

    #[test]
    fn install_in_place_swaps_the_binary() {
        let dir =
            std::env::temp_dir().join(format!("mewsic-update-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let exe = dir.join("mewsic");
        let staged = dir.join("mewsic-staged");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(&staged, b"new").unwrap();

        install_in_place(&exe, &staged).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert!(!dir.join("mewsic.old").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}