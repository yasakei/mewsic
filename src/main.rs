#![forbid(unsafe_code)]

mod autostart;
mod config;
mod connector;
mod credential;
mod engine;
mod lastfm;
mod log;
mod lyrics;
mod net;
mod romanize;
mod state;
mod sync;
mod tui;
mod update;
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
  mewsic                    Run the dashboard + engine
  mewsic web                Run the engine with the web panel enabled
  mewsic background         Run detached in the background — keeps playing
                            after this terminal closes
  mewsic setup              Interactive first-time setup
  mewsic settings           Edit settings interactively
  mewsic stop               Stop the running foreground instance
  mewsic kill background    Stop the background instance
  mewsic kill autostart     Disable autostart (start-on-login)
  mewsic update             Check for and install the latest release
  mewsic update check       Check for a newer release without installing
  mewsic uninstall          Disable autostart and remove the mewsic binary
  mewsic version            Print version

ENVIRONMENT:
  MEWSIC_CONFIG_DIR     Override the config directory (default ~/.config/mewsic)
";

#[cfg(windows)]
const DETACHED_SPAWN_FLAGS: u32 = 0x0000_0200 | 0x0000_0008;

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
        "romanize" => romanize_command(args.get(1)),
        "setup" => setup(),
        "settings" => settings(),
        "web" => run(true),
        "run" => run(false),
        "background" => background(),
        "uninstall" => uninstall(),
        "update" => update_command(args.get(1).map(|s| s.as_str())),
        "kill" => kill(args.get(1).map(|s| s.as_str())),
        "_background" => background_child(),
        "_apply-update" => apply_update_helper(args.get(1)),
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
    romanize::init(&dir);

    let settings = config::Settings::load(&dir);
    let shared = Shared::new();
    Arc::new(AppContext::new(
        shared,
        Arc::new(RwLock::new(settings)),
        dir,
    ))
}

fn run(with_web: bool) {
    let ctx = init_context();
    let interactive = tui::stdout_is_tty();

    if interactive && !with_web {
        offer_startup_update(&ctx);
    }

    if let Some((pid, file)) = running_instance() {
        if file == "background.pid" && !with_web && interactive {
            tui::run_remote_dashboard(&ctx, pid);
            return;
        }
        eprintln!(
            "mewsic is already running (pid {pid}, {file}). Run `mewsic stop` or `mewsic kill background` first."
        );
        std::process::exit(1);
    }

    let _pid = PidGuard::new(config::config_dir().join("mewsic.pid"));

    if interactive {
        tui::enable_raw();
    }

    let mut with_web = with_web;
    let needs_setup = {
        let s = ctx.settings.read().unwrap();
        s.token.is_empty() && (s.lastfm.api_key.is_empty() || s.lastfm.username.is_empty())
    };
    if needs_setup && interactive && !with_web {
        match tui::choose_startup_mode() {
            tui::StartupMode::Terminal => {
                if tui::run_setup_wizard(&ctx).is_none() {
                    tui::disable_raw();
                    return;
                }
            }
            tui::StartupMode::Web => {
                with_web = true;
                let panel_ready = web::start(&ctx);
                tui::show_web_panel_hint(panel_ready);
            }
        }
    }

    let engine = engine::Engine::new(ctx.clone());
    engine.spawn_poller();

    if interactive {
        if with_web {
            web::start(&ctx);
        }
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
        run_headless(&ctx, &engine, with_web);
    }

    engine.shutdown();
    log::write("mewsic stopped");
}

fn run_headless(ctx: &Arc<AppContext>, engine: &Arc<engine::Engine>, with_web: bool) {
    if with_web {
        web::start(ctx);
    }
    let mut last_tick = Instant::now();
    while !engine.quit().load(Ordering::SeqCst) {
        let now = Instant::now();
        let delta = now.duration_since(last_tick).as_millis() as u64;
        last_tick = now;
        engine.tick(delta);
        thread::sleep(Duration::from_millis(250));
    }
}

fn running_instance() -> Option<(u32, String)> {
    for name in ["mewsic.pid", "background.pid"] {
        let path = config::config_dir().join(name);
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(pid) = raw.trim().parse::<u32>() {
                if process_alive(pid) {
                    return Some((pid, name.to_string()));
                }
            }
        }
    }
    None
}

fn background() {
    if let Some((pid, _file)) = running_instance() {
        eprintln!(
            "mewsic is already running (pid {pid}). Run `mewsic stop` or `mewsic kill background` first."
        );
        std::process::exit(1);
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("could not locate the mewsic executable: {e}");
            std::process::exit(1);
        }
    };
    let cfg_dir = config::config_dir();
    let _ = std::fs::create_dir_all(&cfg_dir);

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("_background")
        .current_dir(&cfg_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(DETACHED_SPAWN_FLAGS);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("could not start the background engine: {e}");
            std::process::exit(1);
        }
    };
    let pid = child.id();

    let deadline = Instant::now() + Duration::from_millis(1000);
    loop {
        if child.try_wait().ok().flatten().is_some() {
            eprintln!(
                "background engine exited immediately. Check {} for details.",
                cfg_dir.join("log.txt").display()
            );
            std::process::exit(1);
        }
        let confirmed = std::fs::read_to_string(cfg_dir.join("background.pid"))
            .map(|raw| raw.trim().parse::<u32>() == Ok(pid))
            .unwrap_or(false);
        if confirmed || Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    println!("Background engine started (pid {pid}).");
    println!(
        "It keeps playing after this terminal closes — stop it with `mewsic kill background`."
    );
}

fn background_child() {
    let ctx = init_context();
    if let Some((pid, _file)) = running_instance() {
        crate::log::write(&format!(
            "background start refused: instance already running (pid {pid})"
        ));
        std::process::exit(1);
    }
    let _pid = PidGuard::new(config::config_dir().join("background.pid"));
    crate::log::write(&format!(
        "background engine started (pid {})",
        std::process::id()
    ));

    let engine = engine::Engine::new(ctx.clone());
    engine.spawn_poller();
    run_headless(&ctx, &engine, true);
    engine.shutdown();
    crate::log::write("background engine stopped");
}

fn kill(target: Option<&str>) {
    match target {
        Some("background") => kill_background(),
        Some("autostart") => kill_autostart(),
        _ => {
            eprintln!("usage: mewsic kill <background | autostart>\n");
            println!("{HELP}");
            std::process::exit(2);
        }
    }
}

fn kill_background() {
    let pid_file: PathBuf = config::config_dir().join("background.pid");
    let raw = match std::fs::read_to_string(&pid_file) {
        Ok(r) => r,
        Err(_) => {
            println!("No background mewsic found.");
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
        println!("background mewsic (pid {pid}) is not running.");
        let _ = std::fs::remove_file(&pid_file);
        return;
    }
    if send_terminate(pid) {
        println!("Sent termination signal to background mewsic (pid {pid}).");
    } else {
        println!("Failed to signal background mewsic (pid {pid}).");
    }
    let _ = std::fs::remove_file(&pid_file);
}

fn kill_autostart() {
    let ctx = init_context();
    {
        let mut settings = ctx.settings.write().unwrap();
        settings.update.auto_start = false;
        let _ = settings.save(&ctx.config_dir);
    }
    crate::autostart::apply(false);
    println!("Autostart disabled — mewsic won't start on login anymore.");
    if let Some((pid, _file)) = running_instance() {
        println!(
            "A running instance (pid {pid}) keeps playing until stopped with `mewsic kill background` or `mewsic stop`."
        );
    }
}

fn uninstall() {
    let ctx = init_context();
    {
        let mut settings = ctx.settings.write().unwrap();
        settings.update.auto_start = false;
        let _ = settings.save(&ctx.config_dir);
    }
    crate::autostart::apply(false);
    stop_all_instances();

    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("could not locate the mewsic executable: {e}");
            std::process::exit(1);
        }
    };

    match remove_current_executable(&exe) {
        Ok(()) => {
            println!("Removed mewsic from {}.", exe.display());
            println!("Settings were kept at {}.", config::config_dir().display());
        }
        Err(e) => {
            eprintln!("could not remove {}: {e}", exe.display());
            #[cfg(unix)]
            eprintln!("Try running `sudo mewsic uninstall` if it was installed system-wide.");
            std::process::exit(1);
        }
    }
}

fn stop_all_instances() {
    for name in ["mewsic.pid", "background.pid"] {
        let pid_file = config::config_dir().join(name);
        let pid = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|raw| raw.trim().parse::<u32>().ok());
        if let Some(pid) = pid {
            if pid != std::process::id() && process_alive(pid) {
                let _ = send_terminate(pid);
            }
        }
        let _ = std::fs::remove_file(pid_file);
    }
}

#[cfg(unix)]
fn remove_current_executable(exe: &std::path::Path) -> std::io::Result<()> {
    std::fs::remove_file(exe)
}

#[cfg(windows)]
fn remove_current_executable(exe: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    let pending = exe.with_extension("uninstalling.exe");
    std::fs::rename(exe, &pending)?;
    let script = "Start-Sleep -Milliseconds 500; Remove-Item -LiteralPath $args[0] -Force";
    let spawned = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script, "--"])
        .arg(&pending)
        .creation_flags(DETACHED_SPAWN_FLAGS)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match spawned {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = std::fs::rename(&pending, exe);
            Err(e)
        }
    }
}

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

fn update_command(sub: Option<&str>) {
    let ctx = init_context();
    let state = match sub {
        Some("check") | Some("--check") => update::check_only(&ctx),
        Some(other) => {
            eprintln!("unknown update subcommand: {other}\n");
            println!("{HELP}");
            std::process::exit(2);
        }
        _ => update::run_update(&ctx, true),
    };
    println!("{}", state.message);
}

fn offer_startup_update(ctx: &AppContext) {
    let state = update::check_only(ctx);
    let Some(version) = state.latest else {
        return;
    };

    println!("A new version (v{version}) is available. Download and install it now? [Y/n]");
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return;
    }
    if answer.trim().is_empty() || answer.trim().eq_ignore_ascii_case("y") {
        println!("Downloading and installing v{version}...");
        let result = update::run_update(ctx, true);
        println!("{}", result.message);
    }
}

fn apply_update_helper(staged: Option<&String>) {
    let Some(staged) = staged else {
        std::process::exit(2);
    };
    if let Err(e) = update::apply_staged_as_admin(std::path::Path::new(staged)) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn romanize_command(text: Option<&String>) {
    use std::io::Write;
    romanize::init(&config::config_dir());
    let print = |line: &str| {
        let _ = writeln!(std::io::stdout(), "{}", romanize::romanize(line));
    };
    match text {
        Some(t) => print(t),
        None => {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                print(&line.unwrap_or_default());
            }
        }
    }
}

fn stop() {
    let pid_file: PathBuf = config::config_dir().join("mewsic.pid");
    let raw = match std::fs::read_to_string(&pid_file) {
        Ok(r) => r,
        Err(_) => {
            if let Some((pid, _file)) = running_instance() {
                println!(
                    "No foreground instance. A background instance is running (pid {pid}) — use `mewsic kill background`."
                );
            } else {
                println!("No running mewsic found.");
            }
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
