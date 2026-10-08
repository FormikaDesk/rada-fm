//! vela: a terminal file manager you can trust.

mod config;
mod demo;
mod fonts;
mod shell;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use vela_core::fs::LocalFs;
use vela_core::journal::Journal;
use vela_core::platform::{self, Dirs};
use vela_tui::{Config, Services, Theme};

#[derive(Parser, Debug)]
#[command(
    name = "vela",
    version,
    about = "A terminal file manager you can trust: every operation shows a plan first, can be undone, and keeps going when one file fails."
)]
struct Cli {
    /// Folder to open (default: the current folder).
    path: Option<PathBuf>,

    /// Icons: `auto` (default: Nerd Font if the terminal uses one), `nerd`, `unicode` or `none`.
    #[arg(long, value_name = "SET")]
    icons: Option<String>,

    /// Image previews: `auto` (default), `halfblocks`, `kitty`, `sixel`, `iterm2` or `off`.
    #[arg(long, value_name = "MODE")]
    images: Option<String>,

    /// Colour theme: vela, catppuccin or tokyo-night.
    #[arg(long, value_name = "NAME")]
    theme: Option<String>,

    /// Developer: start in a ready-made scene (palette[:query], plan, progress).
    #[arg(long, value_name = "SCENE", hide = true)]
    demo: Option<String>,

    /// Do not capture the mouse (the terminal keeps selecting text).
    #[arg(long)]
    no_mouse: bool,

    /// Key bindings: `vim+classic` (default), `vim` or `classic`.
    #[arg(long, value_name = "PRESET")]
    keymap: Option<String>,

    /// Show hidden files at startup.
    #[arg(long)]
    hidden: bool,

    /// Write the last folder to FILE on exit (for the shell `cd` helper).
    #[arg(long, value_name = "FILE")]
    cwd_file: Option<PathBuf>,

    /// Print the shell helper that changes directory on exit, then quit.
    #[arg(long, value_name = "SHELL", value_parser = ["bash", "zsh", "fish", "nushell", "powershell"])]
    init: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(shell) = &cli.init {
        print!("{}", shell::snippet(shell));
        return Ok(());
    }

    let dirs = Dirs::from_env().context("cannot locate your home and data folders")?;
    let _log_guard = init_logging(&dirs);
    let cfg_file = config::load(&dirs);

    let start = match &cli.path {
        Some(p) => p.clone(),
        None => std::env::current_dir().context("cannot read the current folder")?,
    };
    let start = std::fs::canonicalize(&start)
        .with_context(|| format!("cannot open {}", start.display()))?;
    // Given a file, open its folder with the cursor on it.
    let (start, select) = if start.is_dir() {
        (start, None)
    } else {
        let name = start.file_name().map(|n| n.to_os_string());
        (start.parent().map(PathBuf::from).unwrap_or(start), name)
    };

    let icons_name = cli
        .icons
        .clone()
        .or_else(|| std::env::var("VELA_ICONS").ok())
        .or(cfg_file.icons.clone());
    let icons = match icons_name.as_deref() {
        None | Some("auto") => {
            if fonts::terminal_uses_nerd_font(&dirs.home) {
                vela_tui::IconSet::Nerd
            } else {
                vela_tui::IconSet::Unicode
            }
        }
        Some(n) => vela_tui::IconSet::parse(n)
            .with_context(|| format!("unknown icon set {n:?} (use auto, nerd, unicode or none)"))?,
    };

    let depth = vela_tui::theme::ColorDepth::detect();
    let theme_name = cli
        .theme
        .clone()
        .or_else(|| std::env::var("VELA_THEME").ok())
        .or(cfg_file.theme.clone());
    let theme = match theme_name {
        Some(n) => Theme::named(&n, depth)
            .with_context(|| format!("unknown theme {n:?} (use {})", Theme::NAMES.join(", ")))?,
        None => Theme::named("vela", depth).expect("built-in theme"),
    };
    let bookmarks = cfg_file
        .bookmarks
        .iter()
        .map(|b| vela_tui::palette::expand(b, &dirs.home))
        .collect();

    let images_name = cli
        .images
        .clone()
        .or_else(|| std::env::var("VELA_IMAGES").ok())
        .or(cfg_file.images.clone());
    let image_mode = match images_name {
        Some(n) => vela_tui::ImageMode::parse(&n).with_context(|| {
            format!("unknown image mode {n:?} (use auto, halfblocks, kitty, sixel, iterm2 or off)")
        })?,
        None => vela_tui::ImageMode::Auto,
    };

    let preset_name = cli
        .keymap
        .clone()
        .or_else(|| std::env::var("VELA_KEYMAP").ok())
        .or(cfg_file.keymap.clone());
    let preset = match preset_name {
        Some(n) => vela_tui::keymap::Preset::parse(&n).with_context(|| {
            format!(
                "unknown keymap {n:?} (use {})",
                vela_tui::keymap::Preset::NAMES.join(", ")
            )
        })?,
        None => vela_tui::keymap::Preset::VimClassic,
    };
    let keymap = vela_tui::keymap::Keymap::new(preset, &cfg_file.keys);
    let mouse = cfg_file.mouse && !cli.no_mouse && std::env::var_os("VELA_NO_MOUSE").is_none();

    let platform = platform::current(dirs.clone());
    let journal = match Journal::open(dirs.journal_path()) {
        Ok(j) => Some(j),
        Err(e) => {
            tracing::error!("journal unavailable: {e}");
            None
        }
    };
    let slow = demo::slow_fs_from_env();
    let fs: Arc<dyn vela_core::fs::FsEngine> = match slow {
        Some(s) => Arc::new(s),
        None => Arc::new(LocalFs),
    };
    let services = Services::start(fs, platform, journal);
    let cfg = Config {
        start_dir: start,
        icons,
        show_hidden: cli.hidden || cfg_file.show_hidden,
        sort: cfg_file.sort,
        theme,
        bookmarks,
        demo: cli.demo.clone().map(|scene| vela_tui::app::Demo {
            scene,
            dest: std::env::var_os("VELA_DEMO_DEST").map(PathBuf::from),
        }),
        image_mode,
        keymap,
        mouse,
        select,
        limits: vela_core::preview::Limits {
            image: cfg_file.image_limits.clone(),
            ..Default::default()
        },
    };

    let outcome = vela_tui::run::run(cfg, services).context("terminal error")?;
    if let Some(f) = &cli.cwd_file {
        write_cwd(f, &outcome.last_dir).with_context(|| format!("cannot write {}", f.display()))?;
    }
    Ok(())
}

fn write_cwd(file: &std::path::Path, dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::fs::write(file, dir.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(file, dir.to_string_lossy().as_bytes())
    }
}

/// Logs go to a file, never to the terminal the UI is drawing on.
fn init_logging(dirs: &Dirs) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let dir = dirs.log_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let appender = tracing_appender::rolling::daily(&dir, "vela.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = tracing_subscriber::EnvFilter::try_from_env("VELA_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(filter)
        .init();
    Some(guard)
}
