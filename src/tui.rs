//! Terminal user interface: live dashboard, setup wizard and settings editor.
//!
//! Built on [`ratatui`] (with the crossterm backend), which owns layout,
//! resizing, wrapping and truncation — no hand-rolled ANSI alignment math.

use std::io::IsTerminal;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Gauge, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::connector;
use crate::engine;
use crate::state::AppContext;
use crate::util::format_seconds;

pub const PANEL_URL: &str = "http://localhost:8999";

type Term = Terminal<CrosstermBackend<std::io::Stdout>>;

/// The live ratatui terminal, created by [`enable_raw`] and torn down by
/// [`disable_raw`]. Only ever touched from the main thread.
static TERMINAL: Mutex<Option<Term>> = Mutex::new(None);

// ─── Terminal lifecycle ──────────────────────────────────────────────────────

pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Enter raw mode, the alternate screen and start ratatui.
pub fn enable_raw() {
    let term = ratatui::init();
    *TERMINAL.lock().unwrap() = Some(term);
}

/// Leave the alternate screen, restore the terminal and drop the UI.
pub fn disable_raw() {
    TERMINAL.lock().unwrap().take();
    ratatui::restore();
}

/// Draw one frame through the shared terminal (no-op when not initialized).
fn draw(f: impl FnOnce(&mut Frame<'_>)) {
    let mut guard = TERMINAL.lock().unwrap();
    if let Some(term) = guard.as_mut() {
        let _ = term.draw(f);
    }
}

// ─── Styled spans ────────────────────────────────────────────────────────────

fn cyan_bold(s: &str) -> Span<'static> {
    Span::styled(
        s.to_string(),
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )
}

fn dim(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(Color::DarkGray))
}

fn green(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(Color::Green))
}

fn red(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(Color::Red))
}

fn yellow(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(Color::Yellow))
}

fn bold(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().add_modifier(Modifier::BOLD))
}

// ─── Screen + prompt rendering ───────────────────────────────────────────────

/// A static screen: a title and a list of content lines.
struct Screen {
    title: String,
    lines: Vec<Line<'static>>,
}

impl Screen {
    fn new(title: &str) -> Screen {
        Screen {
            title: title.to_string(),
            lines: Vec::new(),
        }
    }

    fn push(&mut self, line: Line<'static>) {
        self.lines.push(line);
    }

    fn blank(&mut self) {
        self.lines.push(Line::from(""));
    }
}

/// The live state of one prompt: question text, current input and a hint.
struct Prompt<'a> {
    question: &'a str,
    input: &'a str,
    masked: bool,
    hint: String,
}

fn block_with_title(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Line::from(vec![
            Span::styled(
                " (=^･ω･^=) mewsic ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("· {title} "), Style::default().fg(Color::DarkGray)),
        ]))
        .title_alignment(Alignment::Center)
}

/// Render `screen` with an optional prompt box and an optional footer hint.
///
/// Layout (top to bottom): content, prompt box, prompt hint row, footer hint.
/// Each section gets its own row so nothing paints over anything else.
fn draw_screen(screen: &Screen, prompt: Option<&Prompt>, footer: &str) {
    draw(|f| {
        let area = f.area();
        let block = block_with_title(&screen.title);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let has_prompt = prompt.is_some();
        let has_footer = !footer.is_empty();
        let mut constraints = vec![Constraint::Min(1)];
        if has_prompt {
            constraints.push(Constraint::Length(3)); // prompt box
            constraints.push(Constraint::Length(1)); // prompt hint row
        }
        if has_footer {
            constraints.push(Constraint::Length(1)); // footer row
        }
        let chunks = Layout::vertical(constraints).split(inner);

        let mut idx = 0;
        f.render_widget(
            Paragraph::new(screen.lines.clone()).wrap(Wrap { trim: true }),
            chunks[idx],
        );
        idx += 1;

        if let Some(p) = prompt {
            // Prompt box: question + live input on one inner row.
            let pblock = Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Blue));
            let pinner = pblock.inner(chunks[idx]);
            f.render_widget(pblock, chunks[idx]);

            let display = if p.masked {
                "•".repeat(p.input.chars().count())
            } else {
                p.input.to_string()
            };
            let q = Line::from(vec![
                Span::styled(p.question.to_string(), Style::default().fg(Color::White)),
                Span::raw(" "),
                Span::styled(display.clone(), Style::default().fg(Color::Green)),
            ]);
            f.render_widget(Paragraph::new(q), pinner);

            // Cursor at the end of the input, inside the box (not on its border).
            let x = (pinner.x
                + p.question.chars().count() as u16
                + 1
                + display.chars().count() as u16)
            .min(pinner.right().saturating_sub(1));
            let y = pinner.y;
            f.set_cursor_position(Position::new(x, y));
            idx += 1;

            // Prompt hint row, below the box.
            let hint = Line::from(vec![Span::styled(
                p.hint.clone(),
                Style::default().fg(Color::DarkGray),
            )])
            .alignment(Alignment::Center);
            f.render_widget(Paragraph::new(hint), chunks[idx]);
            idx += 1;
        }

        if has_footer {
            let hint = Line::from(vec![Span::styled(
                footer.to_string(),
                Style::default().fg(Color::DarkGray),
            )])
            .alignment(Alignment::Center);
            f.render_widget(Paragraph::new(hint), chunks[idx]);
        }
    });
}

// ─── Raw-mode input helpers ──────────────────────────────────────────────────

fn read_key_blocking() -> Option<crossterm::event::KeyEvent> {
    loop {
        match event::read() {
            Ok(Event::Key(k)) if k.kind == crossterm::event::KeyEventKind::Press => {
                return Some(k)
            }
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

/// True when the key is Ctrl+C. Crossterm parses the raw `0x03` byte as
/// `Char('c')` with the CONTROL modifier (and some platforms report it as a
/// literal `\u{3}`), so we accept both forms.
fn is_ctrl_c(k: &crossterm::event::KeyEvent) -> bool {
    (matches!(k.code, KeyCode::Char('c') | KeyCode::Char('C'))
        && k.modifiers.contains(KeyModifiers::CONTROL))
        || matches!(k.code, KeyCode::Char('\u{3}'))
}

/// Read a single line of input, supporting backspace. Ctrl+C / Esc cancels.
fn prompt_text(screen: &Screen, question: &str, default: &str, masked: bool) -> Option<String> {
    let mut buf = String::new();
    loop {
        let hint = if default.is_empty() {
            "Enter to accept · Esc to cancel".to_string()
        } else {
            format!("Enter to accept · Esc to cancel · default: {default}")
        };
        let p = Prompt {
            question,
            input: &buf,
            masked,
            hint,
        };
        draw_screen(screen, Some(&p), "");
        match read_key_blocking() {
            Some(k) if k.code == KeyCode::Enter => break,
            Some(k) if is_ctrl_c(&k) || k.code == KeyCode::Esc => return None,
            Some(k) if k.code == KeyCode::Backspace || k.code == KeyCode::Delete => {
                buf.pop();
            }
            Some(k) => {
                if let KeyCode::Char(c) = k.code {
                    if !c.is_control() {
                        buf.push(c);
                    }
                }
            }
            None => {}
        }
    }
    Some(if buf.trim().is_empty() {
        default.to_string()
    } else {
        buf.trim().to_string()
    })
}

fn prompt_confirm(screen: &Screen, question: &str, default: bool) -> Option<bool> {
    loop {
        let suffix = if default { "Y/n" } else { "y/N" };
        let p = Prompt {
            question,
            input: "",
            masked: false,
            hint: format!("y = yes · n = no · Enter = {suffix} · Esc = cancel"),
        };
        draw_screen(screen, Some(&p), "");
        match read_key_blocking() {
            Some(k) if k.code == KeyCode::Char('y') || k.code == KeyCode::Char('Y') => {
                return Some(true)
            }
            Some(k) if k.code == KeyCode::Char('n') || k.code == KeyCode::Char('N') => {
                return Some(false)
            }
            Some(k) if k.code == KeyCode::Enter => return Some(default),
            Some(k) if is_ctrl_c(&k) || k.code == KeyCode::Esc => return None,
            _ => {}
        }
    }
}

/// Pick one of several choices by index number or name. The caller renders the
/// option list itself; `keys` are the match strings. Enter picks `default`.
fn prompt_choice<'a>(
    screen: &Screen,
    question: &str,
    keys: &[&'a str],
    default: Option<&'a str>,
) -> Option<&'a str> {
    loop {
        let default_hint = default.map(|d| format!(" · Enter = {d}")).unwrap_or_default();
        let p = Prompt {
            question,
            input: "",
            masked: false,
            hint: format!(
                "Enter 1-{} or name{default_hint} · Esc = cancel",
                keys.len()
            ),
        };
        draw_screen(screen, Some(&p), "");
        let input = match read_key_blocking() {
            Some(k) if is_ctrl_c(&k) || k.code == KeyCode::Esc => return None,
            Some(k) if k.code == KeyCode::Enter => {
                if let Some(d) = default {
                    return Some(d);
                }
                continue;
            }
            Some(k) => match k.code {
                KeyCode::Char(c) if !c.is_control() => c.to_string(),
                _ => continue,
            },
            None => continue,
        };
        let input = input.trim().to_ascii_lowercase();
        if let Ok(idx) = input.parse::<usize>() {
            if (1..=keys.len()).contains(&idx) {
                return Some(keys[idx - 1]);
            }
            continue;
        }
        for &key in keys {
            if key.eq_ignore_ascii_case(&input) {
                return Some(key);
            }
        }
        let mut prefixes = keys.iter().filter(|k| k.starts_with(&input));
        if let Some(&key) = prefixes.next() {
            if prefixes.next().is_none() {
                return Some(key);
            }
        }
    }
}

fn prompt_number(screen: &Screen, question: &str, default: u64, min: u64, max: u64) -> Option<u64> {
    let mut buf = String::new();
    loop {
        let p = Prompt {
            question,
            input: &buf,
            masked: false,
            hint: format!("Enter a number {min}–{max} · Enter = {default} · Esc = cancel"),
        };
        draw_screen(screen, Some(&p), "");
        match read_key_blocking() {
            Some(k) if k.code == KeyCode::Enter => {
                if buf.is_empty() {
                    return Some(default);
                }
                match buf.trim().parse::<u64>() {
                    Ok(v) if v >= min && v <= max => return Some(v),
                    _ => buf.clear(),
                }
            }
            Some(k) if is_ctrl_c(&k) || k.code == KeyCode::Esc => return None,
            Some(k) if k.code == KeyCode::Backspace || k.code == KeyCode::Delete => {
                buf.pop();
            }
            Some(k) => {
                if let KeyCode::Char(c) = k.code {
                    if c.is_ascii_digit() {
                        buf.push(c);
                    }
                }
            }
            None => {}
        }
    }
}

/// Wait for Enter (or cancel on Esc / Ctrl+C).
fn prompt_continue(screen: &Screen) -> Option<()> {
    draw_screen(screen, None, "Press Enter to continue · Esc to cancel");
    loop {
        match read_key_blocking() {
            Some(k) if k.code == KeyCode::Enter => return Some(()),
            Some(k) if is_ctrl_c(&k) || k.code == KeyCode::Esc => return None,
            _ => {}
        }
    }
}

// ─── First-run screens ───────────────────────────────────────────────────────

pub enum StartupMode {
    Web,
    Terminal,
}

/// First-run prompt: pick web panel or terminal setup.
pub fn choose_startup_mode() -> StartupMode {
    let mut screen = Screen::new("Choose setup mode");
    screen.blank();
    screen.push(Line::from(vec![cyan_bold("(=^･ω･^=) mewsic"), Span::raw("  terminal setup")]));
    screen.push(Line::from(dim("Configure the app without a browser.")));
    screen.blank();
    screen.push(Line::from(vec![
        Span::styled("●", Style::default().fg(Color::Green)),
        Span::raw("  token · view · timing · autostart"),
    ]));
    screen.blank();
    screen.push(Line::from(vec![cyan_bold("1"), Span::raw("  Web panel setup")]));
    screen.push(Line::from(vec![cyan_bold("2"), Span::raw("  Terminal setup")]));
    screen.blank();
    screen.push(Line::from(dim("Choose how to set up this launch.")));

    loop {
        match prompt_text(&screen, "Choose 1 for web or 2 for terminal", "2", false) {
            Some(a) if matches!(a.to_lowercase().as_str(), "1" | "web" | "w") => {
                return StartupMode::Web
            }
            Some(a) if matches!(a.to_lowercase().as_str(), "2" | "terminal" | "t") => {
                return StartupMode::Terminal
            }
            _ => {}
        }
    }
}

/// Screen shown when the user picks the web panel on first run.
pub fn show_web_panel_hint(panel_ready: bool) {
    let mut screen = Screen::new("Web panel");
    screen.blank();
    if panel_ready {
        screen.push(Line::from(green(&format!("Open {PANEL_URL} in your browser."))));
        screen.blank();
        screen.push(Line::from(dim(
            "Finish setup there — the engine picks up the token automatically.",
        )));
    } else {
        screen.push(Line::from(red("The web panel could not be started.")));
        screen.blank();
        screen.push(Line::from(dim(
            "Check the log, then run `mewsic web` to try again.",
        )));
    }
    let _ = prompt_continue(&screen);
}

// ─── Setup wizard ────────────────────────────────────────────────────────────

/// Full interactive setup: token, view, timing, autostart.
/// Returns `None` if the user cancelled.
pub fn run_setup_wizard(ctx: &AppContext) -> Option<()> {
    if !stdout_is_tty() {
        return None;
    }

    let mut settings = ctx.settings.read().unwrap().clone();

    // Welcome
    let mut welcome = Screen::new("Welcome");
    welcome.blank();
    welcome.push(Line::from(vec![cyan_bold("(=^･ω･^=) mewsic"), Span::raw("  terminal setup")]));
    welcome.push(Line::from(dim("Configure the app without a browser.")));
    welcome.blank();
    welcome.push(Line::from(vec![
        Span::styled("●", Style::default().fg(Color::Green)),
        Span::raw("  source · token · view · timing · autostart"),
    ]));
    prompt_continue(&welcome)?;

    // Step 1 — Playback source
    let mut source_screen = Screen::new("Source · Step 1/4");
    source_screen.push(Line::from(vec![dim("Step 1"), Span::raw("  "), bold("Music source")]));
    source_screen.push(Line::from(dim("Where mewsic reads the current track from.")));
    source_screen.blank();
    source_screen.push(Line::from(vec![cyan_bold("1"), Span::raw("  Spotify via Discord")]));
    source_screen.push(Line::from(dim("   Uses the Discord → Spotify connection (no local player).")));
    source_screen.push(Line::from(vec![cyan_bold("2"), Span::raw("  Last.fm / YouTube Music")]));
    source_screen.push(Line::from(dim(
        "   Follows your scrobbles — WebScrobbler or the YT Music desktop app.",
    )));
    source_screen.blank();

    let default_source = match settings.source {
        crate::config::Source::Spotify => "spotify",
        crate::config::Source::Lastfm => "lastfm",
    };
    let source_key =
        prompt_choice(&source_screen, "Choose your music source:", &["spotify", "lastfm"], Some(default_source))?;
    if let Some(parsed) = crate::config::Source::parse(source_key) {
        settings.source = parsed;
    }

    if settings.source == crate::config::Source::Lastfm {
        let mut lf = Screen::new("Source · Step 1/4");
        lf.push(Line::from(vec![dim("Step 1"), Span::raw("  "), bold("Last.fm credentials")]));
        lf.push(Line::from(dim(
            "Free API key at https://www.last.fm/api/account/create (non-commercial).",
        )));
        lf.blank();
        settings.lastfm.api_key =
            prompt_text(&lf, "Last.fm API key:", &settings.lastfm.api_key, false)?;
        settings.lastfm.username =
            prompt_text(&lf, "Last.fm username:", &settings.lastfm.username, false)?;
    }

    // Step 2 — Account (Discord token, optional)
    let mut account = Screen::new("Account · Step 2/4");
    account.push(Line::from(vec![dim("Step 2"), Span::raw("  "), bold("Discord token")]));
    account.push(Line::from(dim(
        "Optional — only needed to update your Discord status.",
    )));
    account.push(Line::from(dim("Leave empty to skip; you can add it later in settings.")));
    account.blank();

    let token = prompt_text(&account, "Enter your Discord token:", "", true)?;
    if !token.is_empty() {
        settings.token = token;
    }

    while !settings.token.is_empty() && !connector::validate_token(&settings.token) {
        let mut bad = Screen::new("Account · Step 2/4");
        bad.push(Line::from(yellow("That token did not validate. Try again.")));
        bad.push(Line::from(dim("Press Enter on an empty box to keep it unset.")));
        bad.blank();
        let next = prompt_text(&bad, "Enter your Discord token:", "", true)?;
        settings.token = next;
    }

    // Step 3 — Preview
    let mut preview = Screen::new("Preview · Step 3/5");
    preview.push(Line::from(vec![dim("Step 3"), Span::raw("  "), bold("Status style")]));
    preview.push(Line::from(dim("How the song text appears in Discord.")));
    preview.blank();

    settings.view.timestamp =
        prompt_confirm(&preview, "Show playback timestamp?", settings.view.timestamp)?;
    settings.view.label = prompt_confirm(&preview, "Show the Song lyrics label?", settings.view.label)?;
    settings.view.emoji = prompt_text(&preview, "Emoji (empty = none)", &settings.view.emoji, false)?;
    settings.view.auto_clear =
        prompt_confirm(&preview, "Clear status on song switch?", settings.view.auto_clear)?;

    let advanced =
        prompt_confirm(&preview, "Enable advanced custom template?", settings.view.advanced.enabled)?;
    settings.view.advanced.enabled = advanced;
    if advanced {
        settings.view.advanced.emoji =
            prompt_text(&preview, "Advanced emoji", &settings.view.advanced.emoji, false)?;
        settings.view.advanced.template =
            prompt_text(&preview, "Template", &settings.view.advanced.template, false)?;
    }

    // Step 4 — Sync
    let mut sync = Screen::new("Sync · Step 4/5");
    sync.push(Line::from(vec![dim("Step 4"), Span::raw("  "), bold("Timing")]));
    sync.push(Line::from(dim(
        "How early the status changes relative to each lyric line.",
    )));
    sync.blank();

    settings.timing.send_time_offset = prompt_number(
        &sync,
        "Send time offset (ms)",
        settings.timing.send_time_offset,
        0,
        10000,
    )?;
    settings.timing.enable_autooffset =
        prompt_confirm(&sync, "Enable autooffset?", settings.timing.enable_autooffset)?;
    settings.timing.autooffset =
        prompt_number(&sync, "Autooffset samples", settings.timing.autooffset as u64, 1, 20)? as usize;

    // Step 5 — Maintenance
    let mut maint = Screen::new("Maintenance · Step 5/5");
    maint.push(Line::from(vec![dim("Step 5"), Span::raw("  "), bold("Autostart")]));
    maint.push(Line::from(dim("Launch mewsic automatically when you log in.")));
    maint.blank();

    settings.update.auto_start =
        prompt_confirm(&maint, "Enable auto-start on login?", settings.update.auto_start)?;

    {
        *ctx.settings.write().unwrap() = settings.clone();
    }
    let _ = settings.save(&ctx.config_dir);
    crate::autostart::apply(settings.update.auto_start);

    prompt_continue(&summary_screen(&settings))?;
    Some(())
}

/// A `key: value` summary row, green when the value is "good".
fn kv(key: &str, value: &str, good: bool) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{key:<11}"),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            value.to_string(),
            if good {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
    ])
}

fn summary_screen(settings: &crate::config::Settings) -> Screen {
    let mut s = Screen::new("Setup complete");
    s.blank();
    s.push(Line::from(vec![cyan_bold("(=^･ω･^=) mewsic"), Span::raw("  setup complete")]));
    s.blank();
    s.push(kv(
        "Token",
        if settings.token.is_empty() { "missing" } else { "saved" },
        !settings.token.is_empty(),
    ));
    s.push(kv("Source", settings.source.label(), true));
    if settings.source == crate::config::Source::Lastfm {
        s.push(kv(
            "Last.fm",
            if settings.lastfm.username.is_empty() {
                "not configured"
            } else {
                &settings.lastfm.username
            },
            !settings.lastfm.username.is_empty(),
        ));
    }
    s.push(kv(
        "Timestamp",
        if settings.view.timestamp { "on" } else { "off" },
        settings.view.timestamp,
    ));
    s.push(kv(
        "Label",
        if settings.view.label { "on" } else { "off" },
        settings.view.label,
    ));
    s.push(kv(
        "Emoji",
        if settings.view.emoji.is_empty() {
            "none"
        } else {
            &settings.view.emoji
        },
        !settings.view.emoji.is_empty(),
    ));
    s.push(kv(
        "Advanced",
        if settings.view.advanced.enabled {
            "enabled"
        } else {
            "disabled"
        },
        settings.view.advanced.enabled,
    ));
    s.push(Line::from(vec![
        Span::styled("Offset     ", Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("{} ms", settings.timing.send_time_offset),
            Style::default().fg(Color::Yellow),
        ),
    ]));
    let autooffset_label = if settings.timing.enable_autooffset {
        format!("{} samples", settings.timing.autooffset)
    } else {
        "off".to_string()
    };
    s.push(kv(
        "Autooffset",
        &autooffset_label,
        settings.timing.enable_autooffset,
    ));
    s.push(kv(
        "Auto start",
        if settings.update.auto_start { "on" } else { "off" },
        settings.update.auto_start,
    ));
    s
}

/// Plain-text summary, printed after `setup` finishes (post-TUI).
pub fn summary(settings: &crate::config::Settings) -> String {
    let advanced = if settings.view.advanced.enabled {
        "enabled"
    } else {
        "disabled"
    };
    let mut out = String::from("Setup complete\n");
    out.push_str(&format!(
        "  Token:      {}\n",
        if settings.token.is_empty() { "missing" } else { "saved" }
    ));
    out.push_str(&format!("  Source:     {}\n", settings.source.label()));
    if settings.source == crate::config::Source::Lastfm {
        out.push_str(&format!(
            "  Last.fm:    {}\n",
            if settings.lastfm.username.is_empty() {
                "not configured"
            } else {
                &settings.lastfm.username
            }
        ));
    }
    out.push_str(&format!(
        "  Timestamp:  {}\n",
        if settings.view.timestamp { "on" } else { "off" }
    ));
    out.push_str(&format!(
        "  Label:      {}\n",
        if settings.view.label { "on" } else { "off" }
    ));
    out.push_str(&format!(
        "  Emoji:      {}\n",
        if settings.view.emoji.is_empty() {
            "none"
        } else {
            &settings.view.emoji
        }
    ));
    out.push_str(&format!("  Advanced:   {advanced}\n"));
    out.push_str(&format!(
        "  Offset:     {} ms\n",
        settings.timing.send_time_offset
    ));
    out.push_str(&format!(
        "  Autooffset: {}\n",
        if settings.timing.enable_autooffset {
            format!("{} samples", settings.timing.autooffset)
        } else {
            "off".to_string()
        }
    ));
    out.push_str(&format!(
        "  Auto start: {}\n",
        if settings.update.auto_start { "on" } else { "off" }
    ));
    out
}

// ─── Settings editor ─────────────────────────────────────────────────────────

fn notice_screen(title: &str, message: &str) -> Screen {
    let mut s = Screen::new(title);
    s.blank();
    s.push(Line::from(green(message)));
    s
}

/// Section-based settings editor (Ctrl+S from the dashboard).
pub fn run_settings_editor(ctx: &AppContext) -> Option<()> {
    if !stdout_is_tty() {
        return None;
    }

    loop {
        let mut menu = Screen::new("Settings editor");
        menu.blank();
        menu.push(Line::from(vec![cyan_bold("1"), Span::raw("  Account (Discord token)")]));
        menu.push(Line::from(vec![cyan_bold("2"), Span::raw("  Source (Spotify / Last.fm)")]));
        menu.push(Line::from(vec![cyan_bold("3"), Span::raw("  View (timestamp, label, advanced)")]));
        menu.push(Line::from(vec![cyan_bold("4"), Span::raw("  Timing (offset, autooffset)")]));
        menu.push(Line::from(vec![cyan_bold("5"), Span::raw("  Autostart")]));
        menu.push(Line::from(vec![cyan_bold("6"), Span::raw("  Update (manual check)")]));
        menu.blank();
        menu.push(Line::from(dim(
            "Pick a section to edit, or press Enter to save & exit.",
        )));

        let choice = prompt_text(&menu, "Choose 1-6 or Enter to save/exit", "", false)?;
        let c = choice.trim().to_lowercase();

        if c.is_empty() {
            ctx.settings.read().unwrap().save(&ctx.config_dir).ok();
            prompt_continue(&notice_screen("Saved", "Settings saved."))?;
            return Some(());
        }

        let mut settings = ctx.settings.read().unwrap().clone();

        match c.as_str() {
            "1" | "account" => {
                let mut scr = Screen::new("Account");
                scr.push(Line::from(dim(
                    "Change the Discord token used to update your status.",
                )));
                scr.blank();
                if let Some(t) = prompt_text(&scr, "Enter your Discord token:", "", true) {
                    if !t.is_empty() {
                        settings.token = t;
                    }
                }
            }
            "2" | "source" => {
                let mut scr = Screen::new("Source");
                scr.push(Line::from(dim("Where mewsic reads the current track from.")));
                scr.blank();
                scr.push(Line::from(vec![cyan_bold("1"), Span::raw("  Spotify via Discord")]));
                scr.push(Line::from(dim("   Uses the Discord → Spotify connection (no local player).")));
                scr.push(Line::from(vec![cyan_bold("2"), Span::raw("  Last.fm / YouTube Music")]));
                scr.push(Line::from(dim(
                    "   Follows your scrobbles — WebScrobbler or the YT Music desktop app.",
                )));
                scr.blank();
                let default_source = match settings.source {
                    crate::config::Source::Spotify => "spotify",
                    crate::config::Source::Lastfm => "lastfm",
                };
                let source_key = prompt_choice(
                    &scr,
                    "Choose your music source:",
                    &["spotify", "lastfm"],
                    Some(default_source),
                )?;
                if let Some(parsed) = crate::config::Source::parse(source_key) {
                    settings.source = parsed;
                }
                if settings.source == crate::config::Source::Lastfm {
                    settings.lastfm.api_key =
                        prompt_text(&scr, "Last.fm API key", &settings.lastfm.api_key, false)?;
                    settings.lastfm.username =
                        prompt_text(&scr, "Last.fm username", &settings.lastfm.username, false)?;
                }
            }
            "3" | "view" => {
                let mut scr = Screen::new("View");
                scr.push(Line::from(dim("Toggle how status text is composed.")));
                scr.blank();
                settings.view.timestamp =
                    prompt_confirm(&scr, "Show playback timestamp?", settings.view.timestamp)?;
                settings.view.label =
                    prompt_confirm(&scr, "Show the Song lyrics label?", settings.view.label)?;
                settings.view.emoji =
                    prompt_text(&scr, "Emoji (empty = none)", &settings.view.emoji, false)?;
                settings.view.auto_clear =
                    prompt_confirm(&scr, "Clear status on song switch?", settings.view.auto_clear)?;
                let adv =
                    prompt_confirm(&scr, "Enable advanced template?", settings.view.advanced.enabled)?;
                settings.view.advanced.enabled = adv;
                if adv {
                    settings.view.advanced.emoji =
                        prompt_text(&scr, "Advanced emoji", &settings.view.advanced.emoji, false)?;
                    settings.view.advanced.template =
                        prompt_text(&scr, "Template", &settings.view.advanced.template, false)?;
                }
            }
            "4" | "timing" => {
                let mut scr = Screen::new("Timing");
                scr.push(Line::from(dim("Status send offsets.")));
                scr.blank();
                settings.timing.send_time_offset = prompt_number(
                    &scr,
                    "Send time offset (ms)",
                    settings.timing.send_time_offset,
                    0,
                    10000,
                )?;
                settings.timing.enable_autooffset =
                    prompt_confirm(&scr, "Enable autooffset?", settings.timing.enable_autooffset)?;
                settings.timing.autooffset = prompt_number(
                    &scr,
                    "Autooffset samples",
                    settings.timing.autooffset as u64,
                    1,
                    20,
                )? as usize;
            }
            "5" | "autostart" => {
                let mut scr = Screen::new("Autostart");
                scr.push(Line::from(dim("Launch on login.")));
                scr.blank();
                settings.update.auto_start =
                    prompt_confirm(&scr, "Enable auto-start on login?", settings.update.auto_start)?;
            }
            "6" | "update" => {
                let mut scr = Screen::new("Update");
                scr.push(Line::from(dim(
                    "Updates are checked when you launch mewsic or run `mewsic update`.",
                )));
                scr.blank();
                prompt_continue(&scr)?;
            }
            _ => {
                let mut scr = Screen::new("Settings editor");
                scr.push(Line::from(yellow("Unknown choice — pick 1-6 or press Enter.")));
                prompt_continue(&scr)?;
                continue;
            }
        }

        {
            *ctx.settings.write().unwrap() = settings.clone();
        }
        let _ = settings.save(&ctx.config_dir);
        crate::autostart::apply(settings.update.auto_start);

        prompt_continue(&notice_screen("Saved", "Saved."))?;
    }
}

// ─── Live dashboard ──────────────────────────────────────────────────────────

fn info_row(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label:<7}"),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled("│ ", Style::default().fg(Color::DarkGray)),
        Span::raw(value.to_string()),
    ])
}

/// Render the live dashboard once (non-blocking).
pub fn render_dashboard(ctx: &AppContext) {
    let pb = engine::snapshot(ctx);
    let latency = engine::last_latency(ctx);
    let source = engine::last_source(ctx);
    let settings = ctx.settings.read().unwrap().clone();

    let song = if pb.song_name.is_empty() {
        "—".to_string()
    } else {
        pb.song_name.clone()
    };
    let artist = if pb.song_author.is_empty() {
        "—".to_string()
    } else {
        pb.song_author.clone()
    };
    let elapsed = format_seconds(pb.song_progress / 1000);
    let total = format_seconds(pb.song_duration / 1000);
    let lyrics = match &pb.current_line {
        Some(text) => text.clone(),
        None if pb.has_lyrics => "waiting for the next line…".to_string(),
        None => "fetching lyrics…".to_string(),
    };
    let pct = if pb.song_duration > 0 {
        ((pb.song_progress as f64 / pb.song_duration as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let playing = if pb.is_playing { "▶" } else { "⏸" };
    let emoji = if settings.view.emoji.is_empty() {
        String::new()
    } else {
        format!("{} ", settings.view.emoji)
    };

    draw(|f| {
        let area = f.area();
        let block = block_with_title("live");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks: [Rect; 5] = Layout::vertical([
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        // Song / artist / time / ping
        let rows = vec![
            info_row("Song", &format!("{playing} {emoji}{song}")),
            info_row("Artist", &artist),
            info_row("Time", &format!("{elapsed} / {total}")),
            info_row("Ping", &format!("{latency} ms")),
            Line::from(""),
        ];
        f.render_widget(Paragraph::new(rows), chunks[0]);

        // Progress gauge
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio(pct / 100.0)
            .label(format!(" {pct:.0}% "));
        f.render_widget(gauge, chunks[1]);

        // Lyrics + source
        let update_rows: Vec<Line<'static>> = {
            let st = ctx.shared.update.lock().unwrap();
            match &st.latest {
                Some(version) => vec![info_row(
                    "Update",
                    &format!("v{version}: {}", st.message.lines().next().unwrap_or("")),
                )],
                None => vec![],
            }
        };
        let mut lyr = vec![
            info_row("Lyrics", &lyrics),
            info_row(
                "Source",
                &format!("{} · {source}", settings.source.label()),
            ),
        ];
        lyr.extend(update_rows);
        f.render_widget(Paragraph::new(lyr).wrap(Wrap { trim: true }), chunks[2]);

        // Footer
        let footer = Paragraph::new(Line::from(Span::styled(
            "Ctrl+S settings · Ctrl+B web panel · Ctrl+C quit",
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center);
        f.render_widget(footer, chunks[4]);
    });
}

/// Attach a terminal dashboard to an already-running background daemon.
/// The daemon remains the sole owner of playback and Discord state; this TUI
/// mirrors its localhost web API state into the normal dashboard renderer.
pub fn run_remote_dashboard(ctx: &AppContext, _pid: u32) {
    enable_raw();
    let mut last_render = Instant::now() - Duration::from_secs(1);
    loop {
        if poll_remote_shortcut(ctx) {
            break;
        }
        if let Ok(response) = crate::net::agent()
            .get("http://127.0.0.1:8999/api/state")
            .timeout(Duration::from_secs(1))
            .call()
        {
            if response.status() == 200 {
                if let Ok(state) = response.into_json::<serde_json::Value>() {
                    apply_remote_state(ctx, &state);
                }
            }
        }
        let now = Instant::now();
        if now.duration_since(last_render) >= Duration::from_millis(250) {
            last_render = now;
            render_dashboard(ctx);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    disable_raw();
}

fn apply_remote_state(ctx: &AppContext, state: &serde_json::Value) {
    let mut playback = ctx.shared.playback.lock().unwrap();
    playback.song_name = state
        .get("song")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    playback.song_author = state
        .get("artist")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    playback.is_playing = state.get("playing").and_then(|v| v.as_bool()).unwrap_or(false);
    playback.song_progress = state.get("progress").and_then(|v| v.as_u64()).unwrap_or(0);
    playback.song_duration = state.get("duration").and_then(|v| v.as_u64()).unwrap_or(0);
    playback.has_lyrics = state
        .get("hasLyrics")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    playback.current_line = serde_json::from_value(state.get("line").cloned().unwrap_or_default())
        .ok()
        .filter(|line: &crate::state::LyricsLine| !line.text.is_empty());
    drop(playback);

    if let Some(source) = state.get("source").and_then(|v| v.as_str()) {
        *ctx.shared.lyric_source.lock().unwrap() = source.to_string();
    }
    if let Some(latency) = state.get("latency").and_then(|v| v.as_u64()) {
        ctx.shared.tracker.lock().unwrap().last_latency = latency;
    }
    if let Some(update) = state.get("update") {
        let mut current = ctx.shared.update.lock().unwrap();
        current.latest = update.get("latest").and_then(|v| v.as_str()).map(str::to_string);
        current.message = update
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
    }
}

fn poll_remote_shortcut(ctx: &AppContext) -> bool {
    match event::poll(Duration::from_millis(1)) {
        Ok(true) => match event::read() {
            Ok(Event::Key(k)) if k.kind == crossterm::event::KeyEventKind::Press => {
                if is_ctrl_c(&k) {
                    return true;
                }
                if k.code == KeyCode::Char('s')
                    && k.modifiers.contains(KeyModifiers::CONTROL)
                    && run_settings_editor(ctx).is_some()
                {
                    push_remote_settings(ctx);
                }
                false
            }
            _ => false,
        },
        _ => false,
    }
}

fn push_remote_settings(ctx: &AppContext) {
    let settings = ctx.settings.read().unwrap().clone();
    let Ok(body) = serde_json::to_string(&settings) else {
        return;
    };
    let _ = crate::net::agent()
        .post("http://127.0.0.1:8999/api/settings")
        .timeout(Duration::from_secs(2))
        .set("Content-Type", "application/json")
        .send_string(&body);
}

/// Non-blocking poll for dashboard shortcuts. Returns `true` if quit.
pub fn poll_shortcut(ctx: &AppContext) -> bool {
    let deadline = Instant::now() + Duration::from_millis(1);
    loop {
        match event::poll(Duration::ZERO) {
            Ok(true) => {
                if let Ok(Event::Key(k)) = event::read() {
                    if k.kind != crossterm::event::KeyEventKind::Press {
                        continue;
                    }
                    if is_ctrl_c(&k) {
                        return true;
                    }
                    match k.code {
                        KeyCode::Char('s') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            let _ = run_settings_editor(ctx);
                        }
                        KeyCode::Char('b') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                            crate::web::start(ctx);
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                if Instant::now() >= deadline {
                    return false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::state::Shared;
    use std::sync::{Arc, RwLock};

    #[test]
    fn remote_state_populates_dashboard_context() {
        let ctx = AppContext::new(
            Shared::new(),
            Arc::new(RwLock::new(Settings::default())),
            std::env::temp_dir(),
        );
        let state = serde_json::json!({
            "song": "Song",
            "artist": "Artist",
            "playing": true,
            "progress": 12_000,
            "duration": 180_000,
            "line": {"time": 11_500, "text": "hello"},
            "hasLyrics": true,
            "source": "cache",
            "latency": 42,
            "update": {"latest": "1.2.0", "message": "available"}
        });

        apply_remote_state(&ctx, &state);

        let playback = ctx.shared.playback.lock().unwrap().clone();
        assert_eq!(playback.song_name, "Song");
        assert_eq!(playback.song_author, "Artist");
        assert!(playback.is_playing);
        assert_eq!(playback.song_progress, 12_000);
        assert_eq!(playback.current_line.unwrap().text, "hello");
        assert_eq!(*ctx.shared.lyric_source.lock().unwrap(), "cache");
        assert_eq!(ctx.shared.tracker.lock().unwrap().last_latency, 42);
        assert_eq!(ctx.shared.update.lock().unwrap().latest.as_deref(), Some("1.2.0"));
    }
}
