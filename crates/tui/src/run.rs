//! Terminal setup and the event loop.

use std::io::{self, Stdout};
use std::time::Duration;

use crossbeam_channel::{select, unbounded};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::{App, Config};
use crate::images::{ImageMode, ImageUi};
use crate::services::Services;
use crate::ui;

type Term = Terminal<CrosstermBackend<Stdout>>;

fn enter(mouse: bool) -> io::Result<Term> {
    // Raw mode: Ctrl+C, Ctrl+Z, Ctrl+Q and Ctrl+S reach the program as ordinary keys
    // instead of signals or flow control.
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    if mouse {
        execute!(out, EnableMouseCapture)?;
    }
    Terminal::new(CrosstermBackend::new(out))
}

fn leave() {
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
}

/// Run the interface until the user quits.
pub fn run(cfg: Config, svc: Services) -> io::Result<Outcome> {
    // Always restore the terminal, even on a panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        default_hook(info);
    }));

    let mouse = cfg.mouse;
    let mut terminal = enter(mouse)?;
    let result = event_loop(&mut terminal, cfg, svc);
    leave();
    let _ = std::panic::take_hook();
    result
}

fn event_loop(terminal: &mut Term, mut cfg: Config, svc: Services) -> io::Result<Outcome> {
    // The terminal is asked about its background before anything else reads the keyboard,
    // and before the graphics query below (one question at a time). Keys the user typed in
    // the meantime are given back and replayed.
    let mut replay: Vec<crossterm::event::KeyEvent> = Vec::new();
    if let Some(a) = cfg.adaptive.take() {
        let t = std::time::Instant::now();
        let detection = crate::termtheme::detect_here(None);
        let light = detection.appearance.is_light();
        if let Some(theme) = crate::theme::Theme::variant(&a.name, light, a.depth) {
            cfg.theme = theme;
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
    let image_ui = match cfg.image_mode {
        ImageMode::Auto => {
            let t = std::time::Instant::now();
            let ui = ImageUi::detect();
            tracing::info!(
                "terminal graphics: {} (detected in {:?})",
                ui.protocol_name(),
                t.elapsed()
            );
            Some(ui)
        }
        ImageMode::Halfblocks => Some(ImageUi::halfblocks()),
        ImageMode::Kitty => Some(ImageUi::with_protocol(
            ratatui_image::picker::ProtocolType::Kitty,
        )),
        ImageMode::Sixel => Some(ImageUi::with_protocol(
            ratatui_image::picker::ProtocolType::Sixel,
        )),
        ImageMode::Iterm2 => Some(ImageUi::with_protocol(
            ratatui_image::picker::ProtocolType::Iterm2,
        )),
        ImageMode::Off => None,
    };
    if let Some(u) = &image_ui {
        tracing::debug!("image protocol in use: {}", u.protocol_name());
    }
    let mut app = App::new(cfg, svc, image_ui);
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
        if app.dirty {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            app.dirty = false;
        }
        // The "copy path" command: the terminal puts the text on the clipboard.
        if let Some(text) = app.take_copied_text() {
            use std::io::Write;
            let out = terminal.backend_mut();
            let _ = out.write_all(crate::clipboard::osc52(&text).as_bytes());
            let _ = out.flush();
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
