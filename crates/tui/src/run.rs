//! Terminal setup and the event loop.

use std::io::{self, IsTerminal, Stdout};
use std::sync::Arc;
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
use ratatui::backend::{Backend, CrosstermBackend};

use crate::app::{App, Config};
use crate::caps::{ImagePlan, Profile, TermCaps};
use crate::compat::{CompatBackend, Rendering};
use crate::icons::IconSet;
use crate::images::{ImageMode, ImageUi};
use crate::input::{self, Input, InputGate};
use crate::services::Services;
use crate::theme::Theme;
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
    crate::termbg::restore(&mut io::stdout());
}

/// Paint the terminal's own background with the theme's, so that the margin the terminal
/// keeps around the grid is not a frame of another colour. [`leave`] gives it back.
fn match_terminal_background(theme: &Theme, caps: &TermCaps) {
    let mut out = io::stdout();
    let Some(rgb) = crate::termbg::wanted(theme, caps, out.is_terminal()) else {
        return;
    };
    if let Err(e) = crate::termbg::set(&mut out, rgb) {
        tracing::warn!("cannot set the terminal background: {e}");
    }
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

/// Wait for a key press on the terminal, in a line of its own: for a window that would
/// disappear at once, and with it a message the person has not read yet. Keys typed before
/// are dropped (they were not meant for this). Without a terminal it returns at once.
pub fn wait_for_key() {
    if enable_raw_mode().is_err() {
        return;
    }
    while event::poll(Duration::ZERO).unwrap_or(false) {
        let _ = event::read();
    }
    loop {
        match event::read() {
            Ok(Event::Key(k)) if k.kind != event::KeyEventKind::Release => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = disable_raw_mode();
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
    match_terminal_background(&cfg.theme, caps);
    let mut app = App::new(cfg, svc, image_ui);
    app.settings.keymap.prefer_cmd = cmd_keys;
    tracing::info!("Cmd key reported by the terminal: {cmd_keys}");
    for key in replay {
        app.on_key(key);
    }
    let shutdown = shutdown_flag();
    let core_rx = app.events();
    let resize_rx = app
        .preview
        .image_ui
        .as_ref()
        .map(|u| u.results())
        .unwrap_or_else(crossbeam_channel::never);

    // Keyboard on its own thread, so slow drawing can never lose a key press. The gate stops
    // it whenever the terminal is asked something (see `input`).
    let (key_tx, key_rx) = unbounded::<Input>();
    let gate = InputGate::new();
    input::spawn(input::Terminal, key_tx, gate.clone())?;
    // Whether clearing the screen may ask the terminal where the cursor is.
    let mut asks_cursor = true;
    // Frames in a row that could not be sent to the terminal.
    let mut failed = 0u32;

    let tick = Duration::from_millis(100);
    loop {
        // Draw as soon as something changed: no artificial frame limit, the batching
        // below already folds a burst of events into a single frame.
        // From nothing: the screen is cleared and every cell is sent again, so that what
        // another program wrote on the terminal does not stay.
        if app.redraw.take_full_redraw() {
            app.redraw.dirty = true;
            if let Err(e) = clear_screen(terminal, &gate, &mut asks_cursor) {
                survive(e, "clear the screen", &mut failed)?;
                app.redraw.full_redraw = true;
            }
        }
        if app.redraw.dirty {
            match terminal.draw(|f| ui::draw(f, &mut app)).map(|_| ()) {
                Ok(()) => {
                    app.redraw.dirty = false;
                    failed = 0;
                }
                Err(e) => {
                    survive(e, "draw the screen", &mut failed)?;
                    // What the terminal shows is unknown now: from nothing, next time.
                    app.redraw.full_redraw = true;
                }
            }
        }
        // The "copy path" command: the terminal puts the text on the clipboard, or the
        // system's own clipboard program does where the terminal would not.
        if let Some(text) = app.ops.take_copied_text() {
            crate::clipboard::copy(&text, caps.osc52, basic, terminal.backend_mut().inner_mut());
        }
        if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            app.shutdown();
        }
        if app.session.should_quit {
            break;
        }
        select! {
            recv(key_rx) -> ev => match ev {
                Ok(ev) => handle_input(&mut app, ev)?,
                Err(_) => return Err(io::Error::other("the keyboard reader stopped")),
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
            handle_input(&mut app, ev)?;
        }
        while let Ok(ev) = core_rx.try_recv() {
            app.on_core_event(ev);
        }
    }
    Ok(Outcome {
        last_dir: app.tab.cwd.clone(),
        chosen: app.session.chosen.take(),
    })
}

fn handle_input(app: &mut App, input: Input) -> io::Result<()> {
    match input {
        Input::Event(Event::Key(k)) => app.on_key(k),
        Input::Event(Event::Mouse(m)) => app.on_mouse(m),
        Input::Event(Event::Resize(_, _)) => app.on_terminal_resized(),
        Input::Event(_) => {}
        Input::Lost(why) => {
            return Err(io::Error::other(format!(
                "the keyboard can no longer be read: {why}"
            )));
        }
    }
    Ok(())
}

/// Clear the terminal so that the next frame sends every cell again.
///
/// ratatui's `clear` remembers where the cursor was and puts it back, which it learns by
/// asking the terminal (`ESC [ 6 n`) and waiting for the answer on the keyboard's own channel.
/// So the keyboard reader is stopped for the question. A terminal that does not answer, or
/// answers too late, is not asked again: the screen is cleared without knowing the cursor,
/// which a full-screen interface does not need (it places the cursor itself on every frame).
fn clear_screen<B>(
    terminal: &mut Terminal<B>,
    gate: &Arc<InputGate>,
    asks_cursor: &mut bool,
) -> io::Result<()>
where
    B: Backend<Error = io::Error>,
{
    if *asks_cursor {
        let paused = gate.pause();
        let cleared = terminal.clear();
        drop(paused);
        match cleared {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::warn!(
                    "the terminal did not say where the cursor is ({e}); \
                     from now on the screen is cleared without asking"
                );
                *asks_cursor = false;
            }
        }
    }
    // The same clearing, and the buffers forgotten, without a question.
    let size = terminal.size()?;
    terminal.resize(size.into())
}

/// More failed frames than this in a row, and the terminal is taken to be unusable.
const MAX_FAILED_FRAMES: u32 = 50;

/// Something went wrong while talking to the terminal. Almost always it is a passing thing
/// (a question that got no answer, a write cut short): it is written in the log and the
/// screen is drawn again from nothing. Only a terminal that is gone, or that keeps failing,
/// ends the program.
fn survive(e: io::Error, what: &str, failed: &mut u32) -> io::Result<()> {
    *failed += 1;
    if is_fatal(&e) || *failed >= MAX_FAILED_FRAMES {
        return Err(e);
    }
    tracing::warn!("cannot {what} ({failed} in a row): {e}; trying again");
    Ok(())
}

/// The terminal itself is gone (closed, hung up), as opposed to not answering.
fn is_fatal(e: &io::Error) -> bool {
    use io::ErrorKind::{
        BrokenPipe, ConnectionAborted, ConnectionReset, NotConnected, UnexpectedEof,
    };
    #[cfg(unix)]
    if e.raw_os_error() == Some(libc::EIO) {
        return true;
    }
    matches!(
        e.kind(),
        BrokenPipe | ConnectionAborted | ConnectionReset | NotConnected | UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::EventSource;
    use ratatui::backend::{ClearType, TestBackend, WindowSize};
    use ratatui::buffer::Cell;
    use ratatui::layout::{Position, Size};
    use ratatui::widgets::Paragraph;

    /// A terminal that never answers the question of where the cursor is, as crossterm
    /// reports it after two seconds, and that counts how often it was asked.
    struct Mute {
        inner: TestBackend,
        asked: std::sync::Arc<std::sync::atomic::AtomicU32>,
    }

    impl Backend for Mute {
        type Error = io::Error;

        fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
        where
            I: Iterator<Item = (u16, u16, &'a Cell)>,
        {
            self.inner.draw(content).map_err(io::Error::other)
        }
        fn hide_cursor(&mut self) -> io::Result<()> {
            self.inner.hide_cursor().map_err(io::Error::other)
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            self.inner.show_cursor().map_err(io::Error::other)
        }
        fn get_cursor_position(&mut self) -> io::Result<Position> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(io::Error::other(
                "The cursor position could not be read within a normal duration",
            ))
        }
        fn set_cursor_position<P: Into<Position>>(&mut self, p: P) -> io::Result<()> {
            self.inner.set_cursor_position(p).map_err(io::Error::other)
        }
        fn clear(&mut self) -> io::Result<()> {
            self.inner.clear().map_err(io::Error::other)
        }
        fn clear_region(&mut self, t: ClearType) -> io::Result<()> {
            self.inner.clear_region(t).map_err(io::Error::other)
        }
        fn size(&self) -> io::Result<Size> {
            self.inner.size().map_err(io::Error::other)
        }
        fn window_size(&mut self) -> io::Result<WindowSize> {
            self.inner.window_size().map_err(io::Error::other)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush().map_err(io::Error::other)
        }
    }

    fn mute() -> (Terminal<Mute>, std::sync::Arc<std::sync::atomic::AtomicU32>) {
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let backend = Mute {
            inner: TestBackend::new(20, 3),
            asked: asked.clone(),
        };
        (Terminal::new(backend).unwrap(), asked)
    }

    fn row(t: &Terminal<Mute>, y: u16) -> String {
        (0..20)
            .map(|x| t.backend().inner.buffer()[(x, y)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// Something that stands in for the keyboard thread: counts how often it looks.
    struct Counting(std::sync::Arc<std::sync::atomic::AtomicU32>);

    impl EventSource for Counting {
        fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(timeout.min(Duration::from_millis(5)));
            Ok(false)
        }
        fn read(&mut self) -> io::Result<Event> {
            unreachable!()
        }
    }

    #[test]
    fn a_terminal_that_does_not_answer_the_cursor_question_is_cleared_all_the_same() {
        let (mut term, asked) = mute();
        term.draw(|f| f.render_widget(Paragraph::new("left over"), f.area()))
            .unwrap();
        assert_eq!(row(&term, 0), "left over");
        let gate = InputGate::new();
        let mut asks = true;
        // Neither an error nor a stop: the screen is cleared.
        clear_screen(&mut term, &gate, &mut asks).unwrap();
        assert_eq!(row(&term, 0), "");
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(!asks, "it asks again");
        // The next clearing does not ask any more (and does not wait two seconds for it).
        term.draw(|f| f.render_widget(Paragraph::new("again"), f.area()))
            .unwrap();
        clear_screen(&mut term, &gate, &mut asks).unwrap();
        assert_eq!(row(&term, 0), "");
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        // And the next frame is drawn in full.
        term.draw(|f| f.render_widget(Paragraph::new("new"), f.area()))
            .unwrap();
        assert_eq!(row(&term, 0), "new");
    }

    #[test]
    fn the_keyboard_is_stopped_while_the_terminal_is_asked_and_goes_on_after() {
        let looks = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (tx, rx) = unbounded::<Input>();
        let gate = InputGate::new();
        input::spawn(Counting(looks.clone()), tx, gate.clone()).unwrap();
        // The backend is asked while the reader must be standing still: its question is the
        // moment the count is looked at.
        struct Watching {
            mute: Mute,
            looks: std::sync::Arc<std::sync::atomic::AtomicU32>,
            seen: std::sync::Arc<std::sync::Mutex<Vec<u32>>>,
        }
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let w = Watching {
            mute: Mute {
                inner: TestBackend::new(10, 2),
                asked,
            },
            looks: looks.clone(),
            seen: seen.clone(),
        };
        impl Backend for Watching {
            type Error = io::Error;
            fn draw<'a, I>(&mut self, c: I) -> io::Result<()>
            where
                I: Iterator<Item = (u16, u16, &'a Cell)>,
            {
                self.mute.draw(c)
            }
            fn hide_cursor(&mut self) -> io::Result<()> {
                self.mute.hide_cursor()
            }
            fn show_cursor(&mut self) -> io::Result<()> {
                self.mute.show_cursor()
            }
            fn get_cursor_position(&mut self) -> io::Result<Position> {
                let before = self.looks.load(std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(150));
                let after = self.looks.load(std::sync::atomic::Ordering::SeqCst);
                self.seen.lock().unwrap().extend([before, after]);
                Ok(Position::ORIGIN)
            }
            fn set_cursor_position<P: Into<Position>>(&mut self, p: P) -> io::Result<()> {
                self.mute.set_cursor_position(p)
            }
            fn clear(&mut self) -> io::Result<()> {
                self.mute.clear()
            }
            fn clear_region(&mut self, t: ClearType) -> io::Result<()> {
                self.mute.clear_region(t)
            }
            fn size(&self) -> io::Result<Size> {
                self.mute.size()
            }
            fn window_size(&mut self) -> io::Result<WindowSize> {
                self.mute.window_size()
            }
            fn flush(&mut self) -> io::Result<()> {
                self.mute.flush()
            }
        }
        let mut term = Terminal::new(w).unwrap();
        let mut asks = true;
        clear_screen(&mut term, &gate, &mut asks).unwrap();
        assert!(asks, "an answer was given");
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[0], seen[1], "the reader looked meanwhile: {seen:?}");
        // After the question it reads again.
        let after = looks.load(std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        assert!(looks.load(std::sync::atomic::Ordering::SeqCst) > after);
        drop(rx);
    }

    #[test]
    fn failures_to_talk_to_the_terminal_are_survived_until_it_is_gone_or_hopeless() {
        let mut failed = 0;
        let timeout =
            io::Error::other("The cursor position could not be read within a normal duration");
        for _ in 0..(MAX_FAILED_FRAMES - 1) {
            survive(
                io::Error::other(timeout.to_string()),
                "clear the screen",
                &mut failed,
            )
            .unwrap();
        }
        assert!(survive(io::Error::other("x"), "clear the screen", &mut failed).is_err());
        // A terminal that is gone does not get another chance.
        let mut failed = 0;
        for kind in [io::ErrorKind::BrokenPipe, io::ErrorKind::UnexpectedEof] {
            assert!(survive(io::Error::from(kind), "draw the screen", &mut failed).is_err());
        }
        #[cfg(unix)]
        assert!(survive(io::Error::from_raw_os_error(libc::EIO), "draw", &mut 0).is_err());
        // A timeout of any kind is not that.
        assert!(!is_fatal(&io::Error::from(io::ErrorKind::TimedOut)));
        assert!(!is_fatal(&timeout));
    }
}
