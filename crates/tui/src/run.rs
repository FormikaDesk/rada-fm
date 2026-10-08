//! Terminal setup and the event loop.

use std::io::{self, Stdout};
use std::time::Duration;

use crossbeam_channel::{select, unbounded};
use crossterm::event::{self, Event};
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

fn enter() -> io::Result<Term> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(out))
}

fn leave() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
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

    let mut terminal = enter()?;
    let result = event_loop(&mut terminal, cfg, svc);
    leave();
    let _ = std::panic::take_hook();
    result
}

fn event_loop(terminal: &mut Term, cfg: Config, svc: Services) -> io::Result<Outcome> {
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
    let mut app = App::new(cfg, svc, image_ui);
    let core_rx = app.events();
    let resize_rx = app
        .image_ui
        .as_ref()
        .map(|u| u.results())
        .unwrap_or_else(crossbeam_channel::never);

    // Keyboard on its own thread, so slow drawing can never lose a key press.
    let (key_tx, key_rx) = unbounded::<Event>();
    std::thread::Builder::new()
        .name("vela-input".into())
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
        Event::Resize(_, _) => app.dirty = true,
        _ => {}
    }
}
