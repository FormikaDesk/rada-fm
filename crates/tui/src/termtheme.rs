//! Is the terminal's background light or dark? Found in layers, cheapest and safest first:
//!
//! 1. what the user said (`appearance = "light"`, `--appearance`, `RADA_APPEARANCE`);
//! 2. the environment: `COLORFGBG`, the Linux console, Apple's Terminal following the
//!    system appearance;
//! 3. asking the terminal for its background colour (OSC 11), only where that is safe, with
//!    a short timeout, and before the keyboard reader starts;
//! 4. otherwise dark.
//!
//! The OSC 11 query is followed by a "primary device attributes" request that every
//! terminal answers: when that answer is in, any answer to the colour query is in too, so
//! nothing arrives late into the input stream. Bytes that are neither answer (a key
//! pressed in the meantime) are handed back, never swallowed. Tmux is only asked when it
//! lets the query through; GNU screen and dumb terminals are never asked.

use std::io;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub fn parse(s: &str) -> Option<Option<Appearance>> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Some(Appearance::Light)),
            "dark" => Some(Some(Appearance::Dark)),
            "auto" | "" => Some(None),
            _ => None,
        }
    }

    pub fn is_light(self) -> bool {
        self == Appearance::Light
    }

    pub fn name(self) -> &'static str {
        match self {
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }
}

/// How the answer was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Chosen in the configuration, on the command line or in the environment.
    Forced,
    /// `COLORFGBG=15;0`.
    ColorFgBg(String),
    /// A terminal whose look is known without asking: why, in words.
    KnownTerminal(&'static str),
    /// The terminal said so: its background colour.
    Osc11 { r: u8, g: u8, b: u8 },
    /// Nothing worked: the dark default.
    Fallback(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detection {
    pub appearance: Appearance,
    pub source: Source,
    /// Keys the user typed while the terminal was being asked (to be replayed).
    pub leftover: Vec<u8>,
}

impl Detection {
    /// One line for `rada doctor` and the log.
    pub fn describe(&self) -> String {
        let how = match &self.source {
            Source::Forced => "set by you (appearance / RADA_APPEARANCE)".to_string(),
            Source::ColorFgBg(v) => format!("from COLORFGBG={v}"),
            Source::KnownTerminal(why) => format!("known terminal: {why}"),
            Source::Osc11 { r, g, b } => {
                format!("the terminal's background is rgb({r}, {g}, {b}) (OSC 11)")
            }
            Source::Fallback(why) => format!("default, because {why}"),
        };
        format!("{}: {how}", self.appearance.name())
    }
}

// ------------------------------------------------------------------------------ the colour

/// Light or dark, for a background colour: by relative luminance, so that mid greys fall
/// on the dark side and pale ones on the light side.
pub fn appearance_of(r: u8, g: u8, b: u8) -> Appearance {
    let lin = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    if l > 0.3 {
        Appearance::Light
    } else {
        Appearance::Dark
    }
}

/// `rgb:RRRR/GGGG/BBBB` (one to four hex digits per channel), or `#RRGGBB`.
fn parse_colour(spec: &str) -> Option<(u8, u8, u8)> {
    let chan = |h: &str| -> Option<u8> {
        if h.is_empty() || h.len() > 4 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        let max = (1u32 << (4 * h.len())) - 1;
        Some(((v * 255 + max / 2) / max) as u8)
    };
    if let Some(rest) = spec
        .strip_prefix("rgb:")
        .or_else(|| spec.strip_prefix("rgba:"))
    {
        let mut it = rest.split('/');
        let (r, g, b) = (it.next()?, it.next()?, it.next()?);
        return Some((chan(r)?, chan(g)?, chan(b)?));
    }
    let hex = spec.strip_prefix('#')?;
    if hex.len() == 6 {
        return Some((chan(&hex[0..2])?, chan(&hex[2..4])?, chan(&hex[4..6])?));
    }
    None
}

// ------------------------------------------------------------------------------ the query

/// The two ends of the conversation with the terminal; a fake one stands in for tests.
pub trait QueryIo {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Wait up to `timeout` for input; the bytes that arrived, or `None` if none did.
    fn read_ready(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>>;
}

pub struct QueryResult {
    pub colour: Option<(u8, u8, u8)>,
    /// Everything that was neither answer: the user's keys.
    pub leftover: Vec<u8>,
    /// The terminal answered the attributes request (so it is alive and nothing is late).
    pub completed: bool,
}

const OSC11_QUERY: &[u8] = b"\x1b]11;?\x1b\\";
const DA1_QUERY: &[u8] = b"\x1b[c";

/// Where the first `needle`-delimited reply of a kind starts and ends in `buf`.
fn find_osc11(buf: &[u8]) -> Option<(usize, usize, &[u8])> {
    let start = buf.windows(5).position(|w| w == b"\x1b]11;")?;
    let body = start + 5;
    // Ends with BEL or ST (ESC \).
    let rel = buf[body..].iter().enumerate().find_map(|(i, &b)| match b {
        0x07 => Some((i, 1)),
        0x1b if buf.get(body + i + 1) == Some(&b'\\') => Some((i, 2)),
        _ => None,
    })?;
    let (len, term) = rel;
    Some((start, body + len + term, &buf[body..body + len]))
}

/// `ESC [ ? … c`
fn find_da1(buf: &[u8]) -> Option<(usize, usize)> {
    let start = buf.windows(3).position(|w| w == b"\x1b[?")?;
    let end = buf[start..].iter().position(|&b| b == b'c')? + start + 1;
    Some((start, end))
}

/// Ask for the background colour. `through_tmux` wraps the query for tmux's passthrough.
/// Reads nothing after `timeout`.
pub fn query_background(
    io: &mut dyn QueryIo,
    through_tmux: bool,
    timeout: Duration,
) -> io::Result<QueryResult> {
    let mut query = Vec::new();
    if through_tmux {
        // tmux forwards what is inside a DCS "tmux;" sequence, with every ESC doubled.
        query.extend_from_slice(b"\x1bPtmux;\x1b\x1b]11;?\x1b\x1b\\\x1b\\");
    } else {
        query.extend_from_slice(OSC11_QUERY);
    }
    query.extend_from_slice(DA1_QUERY);
    io.write_all(&query)?;

    let deadline = Instant::now() + timeout;
    let mut buf: Vec<u8> = Vec::new();
    let mut completed = false;
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match io.read_ready(deadline - now)? {
            None => break,
            Some(bytes) => {
                buf.extend_from_slice(&bytes);
                if find_da1(&buf).is_some() {
                    completed = true;
                    break;
                }
            }
        }
    }

    let mut colour = None;
    if let Some((s, e, body)) = find_osc11(&buf) {
        colour = std::str::from_utf8(body).ok().and_then(parse_colour);
        buf.drain(s..e);
    }
    if let Some((s, e)) = find_da1(&buf) {
        buf.drain(s..e);
    }
    Ok(QueryResult {
        colour,
        leftover: buf,
        completed,
    })
}

// ------------------------------------------------------------------------------ the cell size

/// What the terminal says about drawing pictures.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellQuery {
    /// Width and height of one character cell, in pixels.
    pub cell: Option<(u16, u16)>,
    /// The terminal lists Sixel among its attributes.
    pub sixel: bool,
    /// Everything that was not an answer: the user's keys.
    pub leftover: Vec<u8>,
    /// The terminal answered the attributes request.
    pub completed: bool,
}

const CELL_QUERY: &[u8] = b"\x1b[16t\x1b[14t";

/// Take the first `ESC [ params final` out of `buf`; its parameter bytes.
fn take_csi(buf: &mut Vec<u8>, last: u8) -> Option<Vec<u8>> {
    let mut from = 0;
    while let Some(at) = buf[from..].windows(2).position(|w| w == b"\x1b[") {
        let start = from + at;
        let body = start + 2;
        let len = buf[body..]
            .iter()
            .position(|b| !(0x30..=0x3f).contains(b))
            .unwrap_or(buf.len() - body);
        if buf.get(body + len) == Some(&last) {
            let params = buf[body..body + len].to_vec();
            buf.drain(start..body + len + 1);
            return Some(params);
        }
        from = body;
    }
    None
}

fn numbers(params: &[u8]) -> Vec<u32> {
    String::from_utf8_lossy(params)
        .trim_start_matches('?')
        .split(';')
        .filter_map(|n| n.parse().ok())
        .collect()
}

/// Ask for the size of a character cell in pixels (needed to draw a picture at the right
/// size) and whether the terminal draws Sixel. `grid` is the terminal's size in columns and
/// rows, for terminals that only report the size of the whole text area. As with the
/// background colour, the attributes request closes the conversation.
pub fn query_cell_size(
    io: &mut dyn QueryIo,
    grid: (u16, u16),
    timeout: Duration,
) -> io::Result<CellQuery> {
    let mut query = CELL_QUERY.to_vec();
    query.extend_from_slice(DA1_QUERY);
    io.write_all(&query)?;

    let deadline = Instant::now() + timeout;
    let mut buf: Vec<u8> = Vec::new();
    let mut completed = false;
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match io.read_ready(deadline - now)? {
            None => break,
            Some(bytes) => {
                buf.extend_from_slice(&bytes);
                if find_da1(&buf).is_some() {
                    completed = true;
                    break;
                }
            }
        }
    }

    let mut out = CellQuery {
        completed,
        ..CellQuery::default()
    };
    let px = |v: u32| u16::try_from(v).ok().filter(|v| *v > 0);
    // `ESC [ 6 ; height ; width t` is the cell, `ESC [ 4 ; height ; width t` the text area.
    let mut area = None;
    while let Some(params) = take_csi(&mut buf, b't') {
        match numbers(&params).as_slice() {
            [6, h, w] => out.cell = px(*w).zip(px(*h)),
            [4, h, w] => area = px(*w).zip(px(*h)),
            _ => {}
        }
    }
    if out.cell.is_none()
        && let Some((w, h)) = area
        && grid.0 > 0
        && grid.1 > 0
    {
        out.cell = px((w / grid.0) as u32).zip(px((h / grid.1) as u32));
    }
    if let Some((s, e)) = find_da1(&buf) {
        out.sixel = numbers(&buf[s + 2..e - 1]).contains(&4);
        buf.drain(s..e);
    }
    out.leftover = buf;
    Ok(out)
}

// ------------------------------------------------------------------------------ the layers

/// What the environment and the system can say without asking the terminal.
pub struct Env<'a> {
    pub var: &'a dyn Fn(&str) -> Option<String>,
    /// The system's own light/dark setting (macOS), for terminals that follow it.
    pub system_appearance: &'a dyn Fn() -> Option<Appearance>,
    pub is_tty: bool,
    /// Does tmux let a query through to the real terminal?
    pub tmux_passthrough: &'a dyn Fn() -> bool,
}

/// `COLORFGBG` is `fg;bg` (or `fg;default;bg`); the background is a palette index: 0–6
/// and 8 are dark colours, 7 and 9–15 are light ones.
fn colorfgbg(value: &str) -> Option<Appearance> {
    let bg: u8 = value.rsplit(';').next()?.trim().parse().ok()?;
    Some(if bg == 7 || (9..=15).contains(&bg) {
        Appearance::Light
    } else {
        Appearance::Dark
    })
}

/// Level 2: the environment.
pub fn from_environment(env: &Env) -> Option<(Appearance, Source)> {
    if let Some(v) = (env.var)("COLORFGBG")
        && let Some(a) = colorfgbg(&v)
    {
        return Some((a, Source::ColorFgBg(v)));
    }
    let term = (env.var)("TERM").unwrap_or_default();
    if term == "linux" {
        return Some((
            Appearance::Dark,
            Source::KnownTerminal("the Linux console is dark"),
        ));
    }
    if (env.var)("TERM_PROGRAM").as_deref() == Some("Apple_Terminal")
        && let Some(a) = (env.system_appearance)()
    {
        return Some((
            a,
            Source::KnownTerminal("Terminal.app follows the system appearance"),
        ));
    }
    None
}

/// Whether it is safe to ask the terminal at all, and whether through tmux.
/// `Err` is the reason it is not.
pub fn may_query(env: &Env) -> Result<bool, &'static str> {
    if (env.var)("RADA_NO_TERM_QUERY").is_some() {
        return Err("terminal queries are switched off (RADA_NO_TERM_QUERY)");
    }
    if !env.is_tty {
        return Err("input or output is not a terminal");
    }
    let term = (env.var)("TERM").unwrap_or_default();
    if term.is_empty() || term == "dumb" || term == "linux" {
        return Err("this terminal type does not answer");
    }
    if (env.var)("TMUX").is_some() || term.starts_with("tmux") {
        return if (env.tmux_passthrough)() {
            Ok(true)
        } else {
            Err("tmux does not let queries through (allow-passthrough is off)")
        };
    }
    if (env.var)("STY").is_some() || term.starts_with("screen") {
        return Err("GNU screen does not pass queries through");
    }
    Ok(false)
}

/// The whole decision. `io` is the terminal, when there is a safe way to ask it.
pub fn detect(
    forced: Option<Appearance>,
    env: &Env,
    io: Option<&mut dyn QueryIo>,
    timeout: Duration,
) -> Detection {
    let done = |appearance, source, leftover| Detection {
        appearance,
        source,
        leftover,
    };
    if let Some(a) = forced {
        return done(a, Source::Forced, Vec::new());
    }
    if let Some((a, s)) = from_environment(env) {
        return done(a, s, Vec::new());
    }
    let why = match (may_query(env), io) {
        (Ok(through_tmux), Some(io)) => match query_background(io, through_tmux, timeout) {
            Ok(r) => match r.colour {
                Some((red, green, blue)) => {
                    return done(
                        appearance_of(red, green, blue),
                        Source::Osc11 {
                            r: red,
                            g: green,
                            b: blue,
                        },
                        r.leftover,
                    );
                }
                None => {
                    return done(
                        Appearance::Dark,
                        Source::Fallback(if r.completed {
                            "the terminal did not report its background colour"
                        } else {
                            "the terminal did not answer in time"
                        }),
                        r.leftover,
                    );
                }
            },
            Err(_) => "the terminal could not be queried",
        },
        (Ok(_), None) => "there is no way to query the terminal here",
        (Err(why), _) => why,
    };
    done(Appearance::Dark, Source::Fallback(why), Vec::new())
}

// ------------------------------------------------------------------------------ the real thing

/// A terminal as a pair of file descriptors: where its answers come from and where the
/// questions go. Normally the process's own standard input and output.
#[cfg(unix)]
pub struct Tty {
    input: i32,
    output: i32,
}

#[cfg(unix)]
impl Tty {
    pub fn stdio() -> Tty {
        Tty {
            input: 0,
            output: 1,
        }
    }

    pub fn from_fds(input: i32, output: i32) -> Tty {
        Tty { input, output }
    }
}

#[cfg(unix)]
impl QueryIo for Tty {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut rest = bytes;
        while !rest.is_empty() {
            // SAFETY: `rest` is a valid slice and the descriptor stays open for the call.
            let n = unsafe { libc::write(self.output, rest.as_ptr().cast(), rest.len()) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            rest = &rest[n as usize..];
        }
        Ok(())
    }

    fn read_ready(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let fd = self.input;
        // select(2) rather than poll(2): poll does not work on terminals on macOS.
        // SAFETY: plain libc calls on a descriptor owned by the process; the fd_set and the
        // timeval live on the stack for the duration of the calls.
        unsafe {
            let mut set: libc::fd_set = std::mem::zeroed();
            libc::FD_ZERO(&mut set);
            libc::FD_SET(fd, &mut set);
            let mut tv = libc::timeval {
                tv_sec: timeout.as_secs() as libc::time_t,
                tv_usec: timeout.subsec_micros() as libc::suseconds_t,
            };
            let n = libc::select(
                fd + 1,
                &mut set,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut tv,
            );
            if n < 0 {
                let e = io::Error::last_os_error();
                return if e.kind() == io::ErrorKind::Interrupted {
                    Ok(None)
                } else {
                    Err(e)
                };
            }
            if n == 0 {
                return Ok(None);
            }
            let mut buf = [0u8; 256];
            let got = libc::read(fd, buf.as_mut_ptr().cast(), buf.len());
            if got < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Some(buf[..got as usize].to_vec()))
        }
    }
}

/// The Windows console as a terminal to talk to. While it lives, the console delivers the
/// terminal's answers as plain characters ("virtual terminal input"); the mode it found is
/// put back when it is dropped.
#[cfg(windows)]
pub struct WinConsole {
    input: windows_sys::Win32::Foundation::HANDLE,
    saved: u32,
}

#[cfg(windows)]
impl WinConsole {
    pub fn stdio() -> io::Result<WinConsole> {
        use windows_sys::Win32::System::Console::{
            ENABLE_VIRTUAL_TERMINAL_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE,
            SetConsoleMode,
        };
        // SAFETY: plain Win32 calls on the process's own standard input; `saved` lives on
        // the stack for the duration of the call that fills it.
        unsafe {
            let input = GetStdHandle(STD_INPUT_HANDLE);
            let mut saved = 0u32;
            if GetConsoleMode(input, &mut saved) == 0 {
                return Err(io::Error::last_os_error());
            }
            if SetConsoleMode(input, saved | ENABLE_VIRTUAL_TERMINAL_INPUT) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(WinConsole { input, saved })
        }
    }
}

#[cfg(windows)]
impl Drop for WinConsole {
    fn drop(&mut self) {
        // SAFETY: the handle is the process's standard input, still open.
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleMode(self.input, self.saved);
        }
    }
}

#[cfg(windows)]
impl QueryIo for WinConsole {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let mut out = io::stdout();
        out.write_all(bytes)?;
        out.flush()
    }

    fn read_ready(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Console::{INPUT_RECORD, ReadConsoleInputW};
        // The record type of a key press or release.
        const KEY_EVENT: u16 = 1;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        let mut units: Vec<u16> = Vec::new();
        // SAFETY: plain Win32 calls on the process's standard input. The records are read
        // into a buffer on the stack whose length is passed along, only the `count` first
        // ones are looked at, and the union is read as a key event only when the record
        // says it is one. Reading records (rather than bytes) never blocks once the handle
        // is signalled, whatever kind of event signalled it.
        unsafe {
            if WaitForSingleObject(self.input, ms) != WAIT_OBJECT_0 {
                return Ok(None);
            }
            let mut records: [INPUT_RECORD; 64] = std::mem::zeroed();
            let mut count = 0u32;
            if ReadConsoleInputW(
                self.input,
                records.as_mut_ptr(),
                records.len() as u32,
                &mut count,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            for record in &records[..count as usize] {
                if record.EventType != KEY_EVENT {
                    continue;
                }
                let key = record.Event.KeyEvent;
                let unit = key.uChar.UnicodeChar;
                if key.bKeyDown != 0 && unit != 0 {
                    units.push(unit);
                }
            }
        }
        // Possibly nothing (a resize, a focus change): the caller keeps waiting.
        Ok(Some(String::from_utf16_lossy(&units).into_bytes()))
    }
}

/// Talk to the real terminal, where this system has a way to. Call it with the terminal in
/// raw mode and before anything reads the keyboard.
pub fn with_terminal<R>(ask: impl FnOnce(&mut dyn QueryIo) -> R) -> Option<R> {
    #[cfg(unix)]
    {
        let mut tty = Tty::stdio();
        Some(ask(&mut tty))
    }
    #[cfg(windows)]
    {
        match WinConsole::stdio() {
            Ok(mut console) => Some(ask(&mut console)),
            Err(e) => {
                tracing::debug!("the console cannot be queried: {e}");
                None
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = ask;
        None
    }
}

/// `tmux show-options -gv allow-passthrough`: `on` or `all` lets queries through.
pub fn tmux_allows_passthrough() -> bool {
    std::process::Command::new("tmux")
        .args(["show-options", "-gv", "allow-passthrough"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .map(|o| matches!(String::from_utf8_lossy(&o.stdout).trim(), "on" | "all"))
        .unwrap_or(false)
}

/// macOS: is the system in dark mode? (`defaults` prints `Dark` only then.)
pub fn macos_system_appearance() -> Option<Appearance> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    Some(if String::from_utf8_lossy(&out.stdout).trim() == "Dark" {
        Appearance::Dark
    } else {
        Appearance::Light
    })
}

/// Detect with the real environment and terminal. Call it with the terminal in raw mode
/// and before anything reads the keyboard.
pub fn detect_here(forced: Option<Appearance>) -> Detection {
    use std::io::IsTerminal;
    // The Windows console sets no TERM: it is not a terminal that "does not answer".
    let var = |k: &str| match std::env::var(k).ok() {
        None if cfg!(windows) && k == "TERM" => Some("windows-console".to_string()),
        v => v,
    };
    let env = Env {
        var: &var,
        system_appearance: &macos_system_appearance,
        is_tty: io::stdin().is_terminal() && io::stdout().is_terminal(),
        tmux_passthrough: &tmux_allows_passthrough,
    };
    let timeout = Duration::from_millis(150);
    // Nothing is opened (and no console mode is touched) unless the terminal will be asked.
    if forced.is_some() || from_environment(&env).is_some() || may_query(&env).is_err() {
        return detect(forced, &env, None, timeout);
    }
    with_terminal(|io| detect(forced, &env, Some(io), timeout))
        .unwrap_or_else(|| detect(forced, &env, None, timeout))
}

/// The few keys a user can type in the moment the terminal is being asked, as key events.
pub fn keys_from_bytes(bytes: &[u8]) -> Vec<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    String::from_utf8_lossy(bytes)
        .chars()
        .filter_map(|c| {
            let code = match c {
                '\r' | '\n' => KeyCode::Enter,
                '\t' => KeyCode::Tab,
                '\x7f' => KeyCode::Backspace,
                c if !c.is_control() => KeyCode::Char(c),
                _ => return None,
            };
            Some(KeyEvent::new(code, KeyModifiers::NONE))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};

    /// A terminal that answers on a script: each entry arrives `after` the previous one.
    struct Fake {
        written: Vec<u8>,
        script: VecDeque<(Duration, Vec<u8>)>,
        waited: Duration,
    }

    impl Fake {
        fn new(script: Vec<(u64, Vec<u8>)>) -> Fake {
            Fake {
                written: Vec::new(),
                script: script
                    .into_iter()
                    .map(|(ms, b)| (Duration::from_millis(ms), b))
                    .collect(),
                waited: Duration::ZERO,
            }
        }

        fn unread(&self) -> usize {
            self.script.len()
        }
    }

    impl QueryIo for Fake {
        fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.written.extend_from_slice(bytes);
            Ok(())
        }

        fn read_ready(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
            match self.script.front() {
                Some((after, _)) if *after <= timeout => {
                    let (after, bytes) = self.script.pop_front().unwrap();
                    self.waited += after;
                    Ok(Some(bytes))
                }
                _ => {
                    self.waited += timeout;
                    if let Some(next) = self.script.front_mut() {
                        next.0 = next.0.saturating_sub(timeout);
                    }
                    Ok(None)
                }
            }
        }
    }

    const DA1: &[u8] = b"\x1b[?62;4c";

    fn reply(rgb: &str) -> Vec<u8> {
        format!("\x1b]11;rgb:{rgb}\x1b\\").into_bytes()
    }

    fn env_of(f: &dyn Fn(&str) -> Option<String>) -> Env<'_> {
        Env {
            var: f,
            system_appearance: &|| None,
            is_tty: true,
            tmux_passthrough: &|| false,
        }
    }

    fn run(
        vars: &[(&'static str, &'static str)],
        forced: Option<Appearance>,
        script: Vec<(u64, Vec<u8>)>,
    ) -> (Detection, usize, Vec<u8>) {
        let map: HashMap<&'static str, &'static str> = vars.iter().copied().collect();
        let get = |k: &str| map.get(k).map(|v| v.to_string());
        let env = env_of(&get);
        let mut fake = Fake::new(script);
        let d = detect(forced, &env, Some(&mut fake), Duration::from_millis(60));
        let unread = fake.unread();
        (d, unread, fake.written)
    }

    // ---- the colour ----

    #[test]
    fn colours_are_parsed_in_the_usual_spellings() {
        assert_eq!(parse_colour("rgb:ffff/ffff/ffff"), Some((255, 255, 255)));
        assert_eq!(parse_colour("rgb:0000/0000/0000"), Some((0, 0, 0)));
        assert_eq!(parse_colour("rgb:ff/80/00"), Some((255, 128, 0)));
        assert_eq!(parse_colour("rgb:f/0/f"), Some((255, 0, 255)));
        assert_eq!(parse_colour("rgba:1e1e/1e1e/2e2e/ffff"), Some((30, 30, 46)));
        assert_eq!(parse_colour("#1e1e2e"), Some((30, 30, 46)));
        assert_eq!(parse_colour("rgb:zz/00/00"), None);
        assert_eq!(parse_colour("rgb:ff/00"), None);
        assert_eq!(parse_colour("blue"), None);
    }

    #[test]
    fn backgrounds_are_told_apart_by_luminance() {
        assert_eq!(appearance_of(255, 255, 255), Appearance::Light);
        assert_eq!(appearance_of(239, 241, 245), Appearance::Light); // Latte
        assert_eq!(appearance_of(253, 246, 227), Appearance::Light); // Solarized light
        assert_eq!(appearance_of(0, 0, 0), Appearance::Dark);
        assert_eq!(appearance_of(30, 30, 46), Appearance::Dark); // Mocha
        assert_eq!(appearance_of(40, 44, 52), Appearance::Dark);
        assert_eq!(
            appearance_of(128, 128, 128),
            Appearance::Dark,
            "mid grey leans dark"
        );
    }

    // ---- the environment ----

    #[test]
    fn colorfgbg_names_the_background_by_palette_index() {
        assert_eq!(colorfgbg("15;0"), Some(Appearance::Dark));
        assert_eq!(colorfgbg("0;15"), Some(Appearance::Light));
        assert_eq!(colorfgbg("0;default;15"), Some(Appearance::Light));
        assert_eq!(colorfgbg("0;7"), Some(Appearance::Light));
        assert_eq!(colorfgbg("7;8"), Some(Appearance::Dark));
        assert_eq!(colorfgbg("nonsense"), None);
    }

    #[test]
    fn the_environment_is_enough_and_the_terminal_is_not_asked() {
        let (d, unread, written) = run(
            &[("TERM", "xterm-256color"), ("COLORFGBG", "0;15")],
            None,
            vec![(1, reply("0000/0000/0000"))],
        );
        assert_eq!(d.appearance, Appearance::Light);
        assert!(matches!(d.source, Source::ColorFgBg(_)));
        assert!(written.is_empty(), "nothing was written to the terminal");
        assert_eq!(unread, 1);
    }

    #[test]
    fn the_linux_console_is_dark_and_the_choice_of_the_user_beats_everything() {
        let (d, _, written) = run(&[("TERM", "linux")], None, vec![]);
        assert_eq!(d.appearance, Appearance::Dark);
        assert!(written.is_empty());
        let (d, _, _) = run(
            &[("TERM", "xterm"), ("COLORFGBG", "15;0")],
            Some(Appearance::Light),
            vec![],
        );
        assert_eq!(
            (d.appearance, d.source),
            (Appearance::Light, Source::Forced)
        );
    }

    #[test]
    fn apple_terminal_follows_the_system_appearance() {
        let map: HashMap<&'static str, &'static str> = [
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "Apple_Terminal"),
        ]
        .into();
        let get = |k: &str| map.get(k).map(|v| v.to_string());
        let env = Env {
            var: &get,
            system_appearance: &|| Some(Appearance::Light),
            is_tty: true,
            tmux_passthrough: &|| false,
        };
        let d = detect(None, &env, None, Duration::from_millis(10));
        assert_eq!(d.appearance, Appearance::Light);
        assert!(matches!(d.source, Source::KnownTerminal(_)));
    }

    // ---- asking the terminal ----

    #[test]
    fn a_light_background_reply_chooses_light() {
        let (d, _, written) = run(
            &[("TERM", "xterm-256color")],
            None,
            vec![(2, reply("ffff/ffff/ffff")), (1, DA1.to_vec())],
        );
        assert_eq!(d.appearance, Appearance::Light);
        assert_eq!(
            d.source,
            Source::Osc11 {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert!(d.leftover.is_empty());
        assert!(
            written.starts_with(b"\x1b]11;?"),
            "the query was sent: {written:?}"
        );
        assert!(
            written.ends_with(b"\x1b[c"),
            "followed by the attributes request"
        );
    }

    #[test]
    fn a_dark_background_reply_chooses_dark() {
        let (d, _, _) = run(
            &[("TERM", "xterm-256color")],
            None,
            vec![(2, reply("1e1e/1e1e/2e2e")), (1, DA1.to_vec())],
        );
        assert_eq!(d.appearance, Appearance::Dark);
        assert!(matches!(d.source, Source::Osc11 { .. }));
    }

    #[test]
    fn both_answers_in_one_read_and_with_a_bel_terminator_work() {
        let mut both = b"\x1b]11;rgb:ffff/ffff/ffff\x07".to_vec();
        both.extend_from_slice(DA1);
        let (d, _, _) = run(&[("TERM", "xterm")], None, vec![(1, both)]);
        assert_eq!(d.appearance, Appearance::Light);
    }

    #[test]
    fn no_answer_falls_back_to_dark_within_the_timeout() {
        let t = Instant::now();
        let (d, _, _) = run(&[("TERM", "xterm")], None, vec![]);
        assert_eq!(d.appearance, Appearance::Dark);
        assert!(matches!(d.source, Source::Fallback(_)));
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "{:?}",
            t.elapsed()
        );
    }

    #[test]
    fn a_terminal_that_answers_attributes_but_not_the_colour_is_dark() {
        let (d, _, _) = run(&[("TERM", "xterm")], None, vec![(1, DA1.to_vec())]);
        assert_eq!(d.appearance, Appearance::Dark);
        assert!(
            matches!(d.source, Source::Fallback(w) if w.contains("did not report")),
            "{:?}",
            d.source
        );
    }

    #[test]
    fn a_late_reply_is_left_alone_and_swallows_no_keys() {
        // The answer comes long after the timeout: nothing may be read for it.
        let (d, unread, _) = run(
            &[("TERM", "xterm")],
            None,
            vec![(400, reply("ffff/ffff/ffff")), (1, DA1.to_vec())],
        );
        assert_eq!(d.appearance, Appearance::Dark, "too late to count");
        assert!(d.leftover.is_empty());
        assert_eq!(unread, 2, "the late bytes were not read");
    }

    #[test]
    fn keys_typed_meanwhile_are_handed_back_not_swallowed() {
        let (d, _, _) = run(
            &[("TERM", "xterm")],
            None,
            vec![
                (1, b"jj".to_vec()),
                (2, reply("ffff/ffff/ffff")),
                (1, DA1.to_vec()),
            ],
        );
        assert_eq!(d.appearance, Appearance::Light);
        assert_eq!(d.leftover, b"jj");
        let keys = keys_from_bytes(&d.leftover);
        assert_eq!(keys.len(), 2);
    }

    // ---- where asking is not safe ----

    #[test]
    fn the_terminal_is_not_asked_where_that_is_not_safe() {
        for (vars, why) in [
            (vec![("TERM", "dumb")], "does not answer"),
            (vec![("TERM", "screen-256color")], "screen"),
            (vec![("TERM", "xterm"), ("STY", "1234.pts")], "screen"),
            (
                vec![("TERM", "xterm"), ("TMUX", "/tmp/tmux-1/default,1,0")],
                "tmux",
            ),
            (
                vec![("TERM", "xterm"), ("RADA_NO_TERM_QUERY", "1")],
                "switched off",
            ),
        ] {
            let (d, unread, written) = run(
                &vars,
                None,
                vec![(1, reply("ffff/ffff/ffff")), (1, DA1.to_vec())],
            );
            assert_eq!(d.appearance, Appearance::Dark, "{vars:?}");
            assert!(written.is_empty(), "{vars:?}: nothing may be written");
            assert_eq!(unread, 2, "{vars:?}");
            assert!(
                matches!(d.source, Source::Fallback(w) if w.contains(why)),
                "{vars:?}: {:?}",
                d.source
            );
        }
    }

    #[test]
    fn without_a_terminal_there_is_no_query() {
        let map: HashMap<&'static str, &'static str> = [("TERM", "xterm")].into();
        let get = |k: &str| map.get(k).map(|v| v.to_string());
        let env = Env {
            var: &get,
            system_appearance: &|| None,
            is_tty: false,
            tmux_passthrough: &|| false,
        };
        let mut fake = Fake::new(vec![(1, reply("ffff/ffff/ffff"))]);
        let d = detect(None, &env, Some(&mut fake), Duration::from_millis(60));
        assert_eq!(d.appearance, Appearance::Dark);
        assert!(fake.written.is_empty());
    }

    #[test]
    fn under_tmux_with_passthrough_the_query_is_wrapped() {
        let map: HashMap<&'static str, &'static str> =
            [("TERM", "tmux-256color"), ("TMUX", "x")].into();
        let get = |k: &str| map.get(k).map(|v| v.to_string());
        let env = Env {
            var: &get,
            system_appearance: &|| None,
            is_tty: true,
            tmux_passthrough: &|| true,
        };
        let mut fake = Fake::new(vec![(1, reply("ffff/ffff/ffff")), (1, DA1.to_vec())]);
        let d = detect(None, &env, Some(&mut fake), Duration::from_millis(60));
        assert_eq!(d.appearance, Appearance::Light);
        assert!(
            fake.written.starts_with(b"\x1bPtmux;"),
            "{:?}",
            fake.written
        );
    }

    /// The real descriptor code, over pipes: bytes written to one end come out of
    /// `read_ready`, silence times out, and a late write is not touched.
    #[cfg(unix)]
    #[test]
    fn the_real_tty_reads_what_arrives_and_only_what_arrives_in_time() {
        let pipe = || {
            let mut fds = [0i32; 2];
            // SAFETY: `fds` is a valid array of two ints.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            (fds[0], fds[1])
        };
        let put = |fd: i32, b: &[u8]| {
            // SAFETY: valid slice, open descriptor.
            assert_eq!(
                unsafe { libc::write(fd, b.as_ptr().cast(), b.len()) },
                b.len() as isize
            );
        };
        let (answers_r, answers_w) = pipe();
        let (questions_r, questions_w) = pipe();
        let mut tty = Tty::from_fds(answers_r, questions_w);

        // Silence: nothing, after about the timeout.
        let t = Instant::now();
        assert_eq!(tty.read_ready(Duration::from_millis(30)).unwrap(), None);
        assert!(t.elapsed() >= Duration::from_millis(25));

        // The question goes out whole.
        tty.write_all(b"\x1b]11;?\x1b\\").unwrap();
        let mut got = [0u8; 16];
        // SAFETY: valid buffer, open descriptor.
        let n = unsafe { libc::read(questions_r, got.as_mut_ptr().cast(), got.len()) };
        assert_eq!(&got[..n as usize], b"\x1b]11;?\x1b\\");

        // A full conversation through the whole detection.
        put(answers_w, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\\x1b[?62c");
        let r = query_background(&mut tty, false, Duration::from_millis(200)).unwrap();
        assert_eq!(r.colour, Some((255, 255, 255)));
        assert!(r.completed && r.leftover.is_empty());

        // An answer that comes after the timeout stays in the pipe, unread.
        let r = query_background(&mut tty, false, Duration::from_millis(20)).unwrap();
        assert!(r.colour.is_none() && !r.completed);
        put(answers_w, b"\x1b]11;rgb:0000/0000/0000\x1b\\");
        // SAFETY: closing descriptors this test opened.
        let mut left = [0u8; 64];
        let still = unsafe { libc::read(answers_r, left.as_mut_ptr().cast(), left.len()) };
        assert!(
            still > 0,
            "the late answer is still there for whoever reads next"
        );
        for fd in [answers_r, answers_w, questions_r, questions_w] {
            unsafe { libc::close(fd) };
        }
    }

    #[test]
    fn the_result_can_be_described_for_the_doctor() {
        let d = Detection {
            appearance: Appearance::Light,
            source: Source::Osc11 {
                r: 255,
                g: 255,
                b: 255,
            },
            leftover: vec![],
        };
        assert_eq!(
            d.describe(),
            "light: the terminal's background is rgb(255, 255, 255) (OSC 11)"
        );
    }

    #[test]
    fn the_cell_size_comes_from_the_cell_report() {
        let mut fake = Fake::new(vec![(
            5,
            b"\x1b[6;20;9t\x1b[4;600;900t\x1b[?61;4;22c".to_vec(),
        )]);
        let r = query_cell_size(&mut fake, (100, 30), Duration::from_millis(100)).unwrap();
        assert_eq!(r.cell, Some((9, 20)));
        assert!(r.sixel && r.completed);
        assert!(r.leftover.is_empty());
        assert!(
            fake.written.ends_with(b"\x1b[c"),
            "the attributes request closes it"
        );
    }

    #[test]
    fn the_cell_size_is_worked_out_from_the_text_area_when_that_is_all_there_is() {
        let mut fake = Fake::new(vec![(5, b"\x1b[4;600;900tq\x1b[?62c".to_vec())]);
        let r = query_cell_size(&mut fake, (100, 30), Duration::from_millis(100)).unwrap();
        assert_eq!(r.cell, Some((9, 20)));
        assert!(!r.sixel, "no attribute 4, no Sixel");
        assert_eq!(r.leftover, b"q", "a key typed meanwhile is given back");
    }

    #[test]
    fn a_terminal_that_reports_no_size_gives_none() {
        let mut fake = Fake::new(vec![(5, b"\x1b[?62;4c".to_vec())]);
        let r = query_cell_size(&mut fake, (100, 30), Duration::from_millis(100)).unwrap();
        assert_eq!(r.cell, None);
        assert!(r.sixel && r.completed);
        // And one that says nothing at all is not waited for beyond the timeout.
        let mut silent = Fake::new(vec![]);
        let r = query_cell_size(&mut silent, (100, 30), Duration::from_millis(50)).unwrap();
        assert_eq!(r, CellQuery::default());
    }

    #[test]
    fn a_zero_sized_cell_is_no_cell() {
        let mut fake = Fake::new(vec![(5, b"\x1b[6;0;0t\x1b[?62c".to_vec())]);
        let r = query_cell_size(&mut fake, (0, 0), Duration::from_millis(100)).unwrap();
        assert_eq!(r.cell, None);
    }
}
