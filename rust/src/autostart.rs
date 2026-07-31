//! Best-effort autostart: `.desktop` file on Linux, LaunchAgent on macOS,
//! HKCU Run registry entry on Windows.

use std::fs;
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::Command;

const APP_NAME: &str = "mewsic";

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn command_for_autostart() -> String {
    // Reuse the current executable + a `run` argument so login launches the
    // engine (web panel enabled, headless-safe).
    match std::env::current_exe() {
        Ok(exe) => format!("\"{}\" run", exe.to_string_lossy()),
        Err(_) => "mewsic run".to_string(),
    }
}

/// Apply the autostart setting. Failures are logged, never fatal.
pub fn apply(enabled: bool) {
    let result = apply_inner(enabled);
    if let Err(e) = result {
        crate::log::write(&format!("autostart error: {e}"));
    }
}

fn apply_inner(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        linux_autostart(enabled)?;
    }
    #[cfg(target_os = "macos")]
    {
        macos_autostart(enabled)?;
    }
    #[cfg(target_os = "windows")]
    {
        windows_autostart(enabled)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_autostart(enabled: bool) -> Result<(), String> {
    let dir = home().join(".config").join("autostart");
    let file = dir.join(format!("{APP_NAME}.desktop"));
    if !enabled {
        let _ = fs::remove_file(&file);
        return Ok(());
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let exec = command_for_autostart();
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=mewsic\nExec={exec}\nX-GNOME-Autostart-enabled=true\n"
    );
    fs::write(&file, content).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn macos_autostart(enabled: bool) -> Result<(), String> {
    let dir = home().join("Library").join("LaunchAgents");
    let label = "com.mewsic.autostart";
    let file = dir.join(format!("{label}.plist"));
    if !enabled {
        let _ = fs::remove_file(&file);
        return Ok(());
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \x20 <key>Label</key><string>{label}</string>\n\
         \x20 <key>ProgramArguments</key>\n\
         \x20 <array>\n\
         \x20 \x20 <string>{}</string>\n\
         \x20 \x20 <string>run</string>\n\
         \x20 </array>\n\
         \x20 <key>RunAtLoad</key><true/>\n\
         </dict>\n</plist>\n",
        exe.to_string_lossy()
    );
    fs::write(&file, content).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn windows_autostart(enabled: bool) -> Result<(), String> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    if !enabled {
        let _ = Command::new("reg")
            .args(["delete", key, "/v", APP_NAME, "/f"])
            .status();
        return Ok(());
    }
    let value = command_for_autostart();
    Command::new("reg")
        .args(["add", key, "/v", APP_NAME, "/t", "REG_SZ", "/d", &value, "/f"])
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}
