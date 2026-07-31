#![forbid(unsafe_code)]

mod autostart;
mod config;
mod connector;
mod engine;
mod log;
mod lyrics;
mod net;
mod state;
mod sync;
mod tui;
mod util;
mod web;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use state::{AppContext, Shared};

const HELP: &str = "\
mewsic — keep your Discord status in sync with the song you're playing.

USAGE:
  mewsic                Run the dashboard + engine
  mewsic web            Run the engine with the web panel enabled
  mewsic setup          Interactive first-time setup
  mewsic settings       Edit settings interactively
  mewsic stop           Stop the running instance
  mewsic version        Print version

ENVIRONMENT:
  MEWSIC_CONFIG_DIR     Override the config directory (default ~/.config/mewsic)
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or("run");

    match command {
        "help" | "--help" | "-h" => {
            println!("{HELP}");
        }
        "version" | "--version" | "-V" => {
            println!("mewsic {}", env!("CARGO_PKG_VERSION"));
        }
        "stop" => stop(),
        "setup" => setup(),
        "settings" => settings(),
        "web" => run(true),
        "run" => run(false),
        other => {
            eprintln!("unknown command: {other}\n");
            println!("{HELP}");
            std::process::exit(2);
        }
    }
}

fn init_context() -> Arc<AppContext> {
    let dir = config::config_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        eprintln!("warning: could not create config dir {}", dir.display());
    }
    log::init(&dir);

    let settings = config::Settings::load(&dir);
    let shared = Shared::new();
    Arc::new(AppContext::new(shared, Arc::new(RwLock::new(settings)), dir))
}

fn run(with_web: bool) {
    let ctx = init_context();

    // Refuse to run a second live instance (mirrors the original start script).
    let pid_file = config::config_dir().join("mewsic.pid");
    if let Ok(raw) = std::fs::read_to_string(&pid_file) {
        if let Ok(pid) = raw.trim().parse::<u32>() {
            if process_alive(pid) {
                eprintln!("mewsic is already running (pid {pid}). Run `mewsic stop` first.");
                std::process::exit(1);
            }
        }
    }

    // Write a PID file so `mewsic stop` can find this instance. The guard
    // removes it on clean exit.
    let _pid = PidGuard::new(pid_file);

    // Enter the TUI session up front when interactive: the startup wizard and
    // settings screens render through ratatui, so the terminal must be ready
    // before any of them run.
    let interactive = tui::stdout_is_tty();
    if interactive {
        tui::enable_raw();
    }

    // First run with no token: offer a choice.
    let mut with_web = with_web;
    let token_empty = ctx.settings.read().unwrap().token.is_empty();
    if token_empty && interactive && !with_web {
        match tui::choose_startup_mode() {
            tui::StartupMode::Terminal => {
                if tui::run_setup_wizard(&ctx).is_none() {
                    tui::disable_raw();
                    return;
                }
            }
            tui::StartupMode::Web => {
                with_web = true;
                tui::show_web_panel_hint();
            }
        }
    }

    let engine = engine::Engine::new(ctx.clone());
    engine.spawn_poller();

    if with_web {
        web::start(&ctx);
    }

    if interactive {
        let mut last_tick = Instant::now();
        let mut last_render = Instant::now();
        loop {
            if tui::poll_shortcut(&ctx) {
                break;
            }
            let now = Instant::now();
            let delta = now.duration_since(last_tick).as_millis() as u64;
            last_tick = now;
            engine.tick(delta);

            if now.duration_since(last_render) >= Duration::from_millis(250) {
                last_render = now;
                tui::render_dashboard(&ctx);
            }
            thread_sleep_until(now + Duration::from_millis(16));
        }
        tui::disable_raw();
    } else {
        // Headless: keep ticking quietly until stopped.
        let mut last_tick = Instant::now();
        while !engine.quit().load(Ordering::SeqCst) {
            let now = Instant::now();
            let delta = now.duration_since(last_tick).as_millis() as u64;
            last_tick = now;
            engine.tick(delta);
            thread::sleep(Duration::from_millis(250));
        }
    }

    engine.shutdown();
    log::write("mewsic stopped");
}

/// Removes its PID file when dropped.
struct PidGuard(PathBuf);

impl PidGuard {
    fn new(path: PathBuf) -> PidGuard {
        let _ = std::fs::write(&path, std::process::id().to_string());
        PidGuard(path)
    }
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn thread_sleep_until(deadline: Instant) {
    let now = Instant::now();
    if deadline > now {
        thread::sleep(deadline - now);
    }
}

fn setup() {
    let ctx = init_context();
    if !tui::stdout_is_tty() {
        eprintln!("setup requires an interactive terminal.");
        std::process::exit(1);
    }
    tui::enable_raw();
    let ok = tui::run_setup_wizard(&ctx).is_some();
    tui::disable_raw();
    if ok {
        println!("{}", tui::summary(&ctx.settings.read().unwrap()));
        println!("Run `mewsic` to start.");
    } else {
        std::process::exit(1);
    }
}

fn settings() {
    let ctx = init_context();
    if !tui::stdout_is_tty() {
        eprintln!("settings requires an interactive terminal.");
        std::process::exit(1);
    }
    tui::enable_raw();
    let _ = tui::run_settings_editor(&ctx);
    tui::disable_raw();
}

fn stop() {
    let pid_file: PathBuf = config::config_dir().join("mewsic.pid");
    let raw = match std::fs::read_to_string(&pid_file) {
        Ok(r) => r,
        Err(_) => {
            println!("No running mewsic found.");
            return;
        }
    };
    let pid: u32 = match raw.trim().parse() {
        Ok(p) => p,
        Err(_) => {
            println!("Invalid pid file.");
            let _ = std::fs::remove_file(&pid_file);
            return;
        }
    };
    if !process_alive(pid) {
        println!("mewsic (pid {pid}) is not running.");
        let _ = std::fs::remove_file(&pid_file);
        return;
    }
    let ok = send_terminate(pid);
    if ok {
        println!("Sent termination signal to mewsic (pid {pid}).");
    } else {
        println!("Failed to signal mewsic (pid {pid}).");
    }
    let _ = std::fs::remove_file(&pid_file);
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn process_alive(pid: u32) -> bool {
    // Windows: tasklist returns a non-zero exit code when the pid is gone.
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()),
        Err(_) => false,
    }
}

#[cfg(unix)]
fn send_terminate(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn send_terminate(pid: u32) -> bool {
    std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
