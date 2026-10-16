//! Terminal setup and the event loop.

use std::io::{self, Stdout};
use std::time::Duration;

use crossbeam_channel::{select, unbounded};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::{App, Config};
use crate::caps::{ImagePlan, Profile, TermCaps};
use crate::compat::{CompatBackend, Rendering};
use crate::icons::IconSet;
use crate::images::{ImageMode, ImageUi};
use crate::services::Services;
use crate::ui;

type Term = Terminal<CompatBackend<CrosstermBackend<Stdout>>>;

fn enter(mouse: bool, rendering: Rendering) -> io::Result<Term> {
    // Raw mode: Ctrl+C, Ctrl+Z, Ctrl+Q and Ctrl+S reach the program as ordinary keys
    // instead of signals or flow control.
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    if mouse {
        execute!(out, EnableMouseCapture)?;
    }
    Terminal::new(CompatBackend::new(CrosstermBackend::new(out), rendering))
}

/// Whether the terminal was asked to report keys the extended way (see [`ask_for_cmd_keys`]).
static ENHANCED_KEYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// On a Mac, ask a terminal that speaks the kitty keyboard protocol to report the Cmd key.
/// `true` when it will. The protocol is asked for nowhere else: other systems' keys already
/// work, and every terminal reads the protocol a little differently.
fn ask_for_cmd_keys() -> bool {
    if !cfg!(target_os = "macos")
        || !crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
    {
        return false;
    }
    let pushed = execute!(
        io::stdout(),
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )
    .is_ok();
    ENHANCED_KEYS.store(pushed, std::sync::atomic::Ordering::Relaxed);
    pushed
}

fn leave() {
    if ENHANCED_KEYS.swap(false, std::sync::atomic::Ordering::Relaxed) {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(io::stdout(), DisableMouseCapture);
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

/// Termination requests from outside (`kill`, closing the terminal) end the program
/// the orderly way: the running operation is cancelled, the terminal is restored.
/// `SIGINT` is deliberately absorbed: nothing a stray signal does should stop a copy.
#[cfg(unix)]
fn shutdown_flag() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let flag = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGHUP] {
        if let Err(e) = signal_hook::flag::register(sig, flag.clone()) {
            tracing::warn!("cannot watch signal {sig}: {e}");
        }
    }
    let ignored = Arc::new(AtomicBool::new(false));
    let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, ignored);
    flag
}

#[cfg(not(unix))]
fn shutdown_flag() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))
}

/// What the caller wants to know after the UI closes.
pub struct Outcome {
    /// The folder the user was in (for `cd`-on-quit).
    pub last_dir: std::path::PathBuf,
    /// When rada was asked to choose files: what the person chose, or `None` if they said no.
    pub chosen: Option<Vec<std::path::PathBuf>>,
}

/// Run the interface until the user quits. `caps` is what the terminal can do
/// ([`TermCaps::detect_here`]): it decides how the interface reaches the terminal, never
/// what the interface is.
pub fn run(mut cfg: Config, svc: Services, caps: TermCaps) -> io::Result<Outcome> {
    // Always restore the terminal, even on a panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        default_hook(info);
    }));

    for line in caps.describe() {
        tracing::info!("terminal — {line}");
    }
    // A terminal without mouse reports is never asked for them.
    cfg.mouse = cfg.mouse && caps.mouse;
    let mouse = cfg.mouse;
    // Without a symbol font the interface is drawn the same and its glyphs are replaced
    // on the way out, like the colours.
    let rendering = match cfg.icons {
        IconSet::UnicodeStd => caps.rendering().with_std_glyphs(),
        _ => caps.rendering(),
    };
    let mut terminal = enter(mouse, rendering)?;
    let result = event_loop(&mut terminal, cfg, svc, &caps);
    leave();
    let _ = std::panic::take_hook();
    result
}

fn event_loop(
    terminal: &mut Term,
    mut cfg: Config,
    svc: Services,
    caps: &TermCaps,
) -> io::Result<Outcome> {
    let basic = caps.profile == Profile::Basic;
    // The terminal is asked about its background before anything else reads the keyboard,
    // and before the graphics query below (one question at a time). Keys the user typed in
    // the meantime are given back and replayed.
    let mut replay: Vec<crossterm::event::KeyEvent> = Vec::new();
    // The basic profile asks the terminal nothing: the theme stays the dark stand-in.
    if let Some(a) = cfg.adaptive.take().filter(|_| !basic) {
        let t = std::time::Instant::now();
        let detection = crate::termtheme::detect_here(None);
        let light = detection.appearance.is_light();
        if let Some(theme) = crate::theme::Theme::variant(&a.name, light, a.depth) {
            cfg.theme = if a.transparent {
                theme.transparent()
            } else {
                theme
            };
        }
        tracing::info!(
            "terminal background — {} (in {:?})",
            detection.describe(),
            t.elapsed()
        );
        replay = crate::termtheme::keys_from_bytes(&detection.leftover);
    }
    // Terminal graphics are detected now: the query needs raw mode and must finish
    // before the keyboard reader starts.
    // Asking may take a moment: what was typed meanwhile is replayed too.
    let mut asked = |(ui, leftover): (ImageUi, Vec<u8>)| {
        replay.extend(crate::termtheme::keys_from_bytes(&leftover));
        ui
    };
    use ratatui_image::picker::ProtocolType;
    let image_ui = match cfg.image_mode {
        ImageMode::Auto if caps.images == ImagePlan::Off => None,
        ImageMode::Auto => {
            let t = std::time::Instant::now();
            let ui = asked(ImageUi::detect(caps));
            tracing::info!(
                "terminal graphics: {} (detected in {:?})",
                ui.protocol_name(),
                t.elapsed()
            );
            Some(ui)
        }
        ImageMode::Halfblocks => Some(ImageUi::halfblocks()),
        ImageMode::Kitty => Some(asked(ImageUi::for_protocol(
            ProtocolType::Kitty,
            caps.may_query,
        ))),
        ImageMode::Sixel => Some(asked(ImageUi::for_protocol(
            ProtocolType::Sixel,
            caps.may_query,
        ))),
        ImageMode::Iterm2 => Some(asked(ImageUi::for_protocol(
            ProtocolType::Iterm2,
            caps.may_query,
        ))),
        ImageMode::Off => None,
    };
    if let Some(u) = &image_ui {
        tracing::debug!("image protocol in use: {}", u.protocol_name());
    }
    // Asking for the Cmd key starts with a question to the terminal: not where none is asked.
    let cmd_keys = cfg.cmd_keys && caps.may_query && ask_for_cmd_keys();
    let mut app = App::new(cfg, svc, image_ui);
    app.keymap.prefer_cmd = cmd_keys;
    tracing::info!("Cmd key reported by the terminal: {cmd_keys}");
    for key in replay {
        app.on_key(key);
    }
    let shutdown = shutdown_flag();
    let core_rx = app.events();
    let resize_rx = app
        .image_ui
        .as_ref()
        .map(|u| u.results())
        .unwrap_or_else(crossbeam_channel::never);

    // Keyboard on its own thread, so slow drawing can never lose a key press.
    let (key_tx, key_rx) = unbounded::<Event>();
    std::thread::Builder::new()
        .name("rada-input".into())
        .spawn(move || {
            while let Ok(ev) = event::read() {
                if key_tx.send(ev).is_err() {
                    break;
                }
            }
        })?;

    let tick = Duration::from_millis(100);
    loop {
        // Draw as soon as something changed: no artificial frame limit, the batching
        // below already folds a burst of events into a single frame.
        // From nothing: the screen is cleared and every cell is sent again, so that what
        // another program wrote on the terminal does not stay.
        if app.take_full_redraw() {
            terminal.clear()?;
            app.dirty = true;
        }
        if app.dirty {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            app.dirty = false;
        }
        // The "copy path" command: the terminal puts the text on the clipboard, or the
        // system's own clipboard program does where the terminal would not.
        if let Some(text) = app.take_copied_text() {
            crate::clipboard::copy(&text, caps.osc52, basic, terminal.backend_mut().inner_mut());
        }
        if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            app.shutdown();
        }
        if app.should_quit {
            break;
        }
        select! {
            recv(key_rx) -> ev => match ev {
                Ok(ev) => handle_input(&mut app, ev),
                Err(_) => break,
            },
            recv(resize_rx) -> r => {
                if let Ok(r) = r {
                    app.on_image_resized(r);
                }
            }
            recv(core_rx) -> ev => {
                if let Ok(ev) = ev {
                    app.on_core_event(ev);
                }
            }
            default(tick) => app.tick(),
        }
        // Whatever piled up meanwhile belongs to the same frame.
        while let Ok(ev) = key_rx.try_recv() {
            handle_input(&mut app, ev);
        }
        while let Ok(ev) = core_rx.try_recv() {
            app.on_core_event(ev);
        }
    }
    Ok(Outcome {
        last_dir: app.cwd.clone(),
        chosen: app.chosen.take(),
    })
}

fn handle_input(app: &mut App, ev: Event) {
    match ev {
        Event::Key(k) => app.on_key(k),
        Event::Mouse(m) => app.on_mouse(m),
        Event::Resize(_, _) => app.dirty = true,
        _ => {}
    }
}
