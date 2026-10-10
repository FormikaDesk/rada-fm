//! rada: a terminal file manager you can trust.

mod config;
mod demo;
mod desktop;
mod fonts;
mod shell;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rada_core::fs::LocalFs;
use rada_core::journal::Journal;
use rada_core::platform::{self, Dirs};
use rada_tui::{Config, Services, Theme};

#[derive(Subcommand, Debug)]
enum Command {
    /// Install or remove things rada needs outside its own folders.
    Setup {
        #[command(subcommand)]
        what: SetupWhat,
    },
}

#[derive(Subcommand, Debug)]
enum SetupWhat {
    /// Add rada to the application menu (launcher entry and icons), for this user only.
    Desktop {
        /// Take away exactly what a previous setup installed.
        #[arg(long)]
        remove: bool,
        /// Also make rada the default program for folders (the previous one is
        /// remembered, and `--remove` restores it).
        #[arg(long)]
        default_file_manager: bool,
    },
}

#[derive(Parser, Debug)]
#[command(
    name = "rada",
    version,
    about = "A safe harbor for your files: a terminal file manager where every operation shows a plan first, can be undone, and keeps going when one file fails.",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Folder to open (default: the current folder). A folder named like a command
    /// (`setup`) needs a leading `./`.
    path: Vec<PathBuf>,

    /// Open rada in a new terminal window (used by the desktop launcher entry).
    #[arg(long)]
    spawn_terminal: bool,

    /// Icons: `auto` (default: Nerd Font if the terminal uses one), `nerd`, `unicode` or `none`.
    #[arg(long, value_name = "SET")]
    icons: Option<String>,

    /// Image previews: `auto` (default), `halfblocks`, `kitty`, `sixel`, `iterm2` or `off`.
    #[arg(long, value_name = "MODE")]
    images: Option<String>,

    /// Colour theme: `auto` (default), rada, catppuccin or tokyo-night (these follow the
    /// terminal's background), or one fixed variant: rada-dark, rada-light,
    /// catppuccin-mocha, catppuccin-latte, tokyo-night-dark, tokyo-night-day.
    #[arg(long, value_name = "NAME")]
    theme: Option<String>,

    /// Which variant a theme name takes: `auto` (ask the terminal), `light` or `dark`.
    #[arg(long, value_name = "LOOK")]
    appearance: Option<String>,

    /// Developer: start in a ready-made scene (palette[:query], plan, progress).
    #[arg(long, value_name = "SCENE", hide = true)]
    demo: Option<String>,

    /// Do not capture the mouse (the terminal keeps selecting text).
    #[arg(long)]
    no_mouse: bool,

    /// Key bindings: `vim+classic` (default), `vim` or `classic`.
    #[arg(long, value_name = "PRESET")]
    keymap: Option<String>,

    /// Plan an operation given as JSON (a file, or `-` for stdin) and show it in the usual
    /// confirmation window. See `rada --schema request`.
    #[arg(long, value_name = "FILE")]
    request: Option<String>,

    /// Print the JSON Schema of `request` (what `--request` accepts) or `plan`, then quit.
    #[arg(long, value_name = "WHAT", value_parser = ["request", "plan"])]
    schema: Option<String>,

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
    if let Some(Command::Setup {
        what: SetupWhat::Desktop {
            remove,
            default_file_manager,
        },
    }) = &cli.command
    {
        let dirs = Dirs::from_env().context("cannot locate your home and data folders")?;
        let setup = desktop::Setup::from_env(&dirs)?;
        if *remove && *default_file_manager {
            anyhow::bail!("--remove and --default-file-manager cannot be used together");
        }
        let lines = if *remove {
            setup.remove()?
        } else {
            setup.install(*default_file_manager)?
        };
        for l in lines {
            println!("{l}");
        }
        return Ok(());
    }
    if cli.spawn_terminal {
        return desktop::spawn_terminal(cli.path.first().map(PathBuf::as_path));
    }
    if let Some(shell) = &cli.init {
        print!("{}", shell::snippet(shell));
        return Ok(());
    }

    if let Some(what) = &cli.schema {
        print!(
            "{}",
            if what == "plan" {
                rada_core::schema::plan()
            } else {
                rada_core::schema::request()
            }
        );
        return Ok(());
    }
    let request = match &cli.request {
        Some(src) => {
            let text = if src == "-" {
                std::io::read_to_string(std::io::stdin())
                    .context("cannot read the request from stdin")?
            } else {
                std::fs::read_to_string(src).with_context(|| format!("cannot read {src}"))?
            };
            let req: rada_core::ops::OpRequest = serde_json::from_str(&text)
                .context("the request is not valid (see `rada --schema request`)")?;
            req.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
            Some(req)
        }
        None => None,
    };
    let dirs = Dirs::from_env().context("cannot locate your home and data folders")?;
    let _log_guard = init_logging(&dirs);
    let cfg_file = config::load(&dirs);

    let start = match cli.path.first() {
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
        .or_else(|| std::env::var("RADA_ICONS").ok())
        .or(cfg_file.icons.clone());
    let icons = match icons_name.as_deref() {
        None | Some("auto") => {
            if fonts::terminal_uses_nerd_font(&dirs.home) {
                rada_tui::IconSet::Nerd
            } else {
                rada_tui::IconSet::Unicode
            }
        }
        Some(n) => rada_tui::IconSet::parse(n)
            .with_context(|| format!("unknown icon set {n:?} (use auto, nerd, unicode or none)"))?,
    };

    let depth = rada_tui::theme::ColorDepth::detect();
    let theme_name = cli
        .theme
        .clone()
        .or_else(|| std::env::var("RADA_THEME").ok())
        .or(cfg_file.theme.clone())
        .unwrap_or_else(|| "auto".into());
    let follows = Theme::follows_background(&theme_name).with_context(|| {
        format!(
            "unknown theme {theme_name:?} (use auto, rada, catppuccin, tokyo-night, or one of {})",
            Theme::NAMES.join(", ")
        )
    })?;
    let appearance_name = cli
        .appearance
        .clone()
        .or_else(|| std::env::var("RADA_APPEARANCE").ok())
        .or(cfg_file.appearance.clone());
    let forced = match appearance_name {
        Some(n) => rada_tui::termtheme::Appearance::parse(&n)
            .with_context(|| format!("unknown appearance {n:?} (use auto, light or dark)"))?,
        None => None,
    };
    // A name that follows the background is resolved once the terminal is up (it may have
    // to be asked); a fixed variant, or a forced appearance, needs no asking.
    let (theme, adaptive) = match (follows, forced) {
        (true, None) => (
            Theme::variant(&theme_name, false, depth).expect("checked above"),
            Some(rada_tui::app::Adaptive {
                name: theme_name.clone(),
                depth,
            }),
        ),
        (true, Some(look)) => (
            Theme::variant(&theme_name, look.is_light(), depth).expect("checked above"),
            None,
        ),
        (false, _) => (
            Theme::named(&theme_name, depth).expect("checked above"),
            None,
        ),
    };
    let bookmarks = cfg_file
        .bookmarks
        .iter()
        .map(|b| rada_tui::palette::expand(b, &dirs.home))
        .collect();

    let images_name = cli
        .images
        .clone()
        .or_else(|| std::env::var("RADA_IMAGES").ok())
        .or(cfg_file.images.clone());
    let image_mode = match images_name {
        Some(n) => rada_tui::ImageMode::parse(&n).with_context(|| {
            format!("unknown image mode {n:?} (use auto, halfblocks, kitty, sixel, iterm2 or off)")
        })?,
        None => rada_tui::ImageMode::Auto,
    };

    let preset_name = cli
        .keymap
        .clone()
        .or_else(|| std::env::var("RADA_KEYMAP").ok())
        .or(cfg_file.keymap.clone());
    let preset = match preset_name {
        Some(n) => rada_tui::keymap::Preset::parse(&n).with_context(|| {
            format!(
                "unknown keymap {n:?} (use {})",
                rada_tui::keymap::Preset::NAMES.join(", ")
            )
        })?,
        None => rada_tui::keymap::Preset::VimClassic,
    };
    let keymap = rada_tui::keymap::Keymap::new(preset, &cfg_file.keys);
    let mouse = cfg_file.mouse && !cli.no_mouse && std::env::var_os("RADA_NO_MOUSE").is_none();

    let platform = platform::current_with(
        dirs.clone(),
        platform::PlatformOptions {
            hide_devices: cfg_file.hide_devices.clone(),
        },
    );
    let journal = match Journal::open(dirs.journal_path()) {
        Ok(j) => Some(j),
        Err(e) => {
            tracing::error!("journal unavailable: {e}");
            None
        }
    };
    let slow = demo::slow_fs_from_env();
    let fs: Arc<dyn rada_core::fs::FsEngine> = match slow {
        Some(s) => Arc::new(s),
        None => Arc::new(LocalFs),
    };
    let services = Services::start_with(fs, platform, journal, cfg_file.archive_limits);
    let layout = match cfg_file.layout.as_deref() {
        None => rada_tui::LayoutKind::Explorer,
        Some(n) => rada_tui::LayoutKind::parse(n)
            .with_context(|| format!("unknown layout {n:?} (use explorer or compact)"))?,
    };
    let view = match cfg_file.view.as_deref() {
        None => rada_tui::ViewMode::Details,
        Some(n) => rada_tui::ViewMode::parse(n)
            .with_context(|| format!("unknown view {n:?} (use details or icons)"))?,
    };
    let dates = match cfg_file.dates.as_deref() {
        None => rada_tui::fmt::DateStyle::Relative,
        Some(n) => rada_tui::fmt::DateStyle::parse(n)
            .with_context(|| format!("unknown dates {n:?} (use relative or absolute)"))?,
    };
    let saved_ui = rada_core::uistate::load(&dirs.rada_state());
    // Only folders that are still there come back.
    let mut saved_ui = saved_ui;
    if saved_ui.tabs.iter().any(|t| !t.path.is_dir()) {
        let active_path = saved_ui
            .tabs
            .get(saved_ui.active_tab)
            .map(|t| t.path.clone());
        saved_ui.tabs.retain(|t| t.path.is_dir());
        saved_ui.active_tab = active_path
            .and_then(|p| saved_ui.tabs.iter().position(|t| t.path == p))
            .unwrap_or(0);
    }
    let cfg = Config {
        start_explicit: !cli.path.is_empty(),
        layout,
        view,
        dates,
        date_format: rada_tui::fmt::DateFormat::from_env(),
        remember_tabs: cfg_file.remember_tabs,
        // What the user chose with Alt+P wins over the file's setting.
        details_pane: saved_ui
            .details
            .or_else(|| (!cfg_file.show_details_pane).then_some(false)),
        saved_ui: saved_ui.clone(),
        start_dir: start,
        icons,
        show_hidden: cli.hidden || cfg_file.show_hidden,
        sort: cfg_file.sort,
        theme,
        adaptive,
        bookmarks,
        demo: cli.demo.clone().map(|scene| rada_tui::app::Demo {
            scene,
            dest: std::env::var_os("RADA_DEMO_DEST").map(PathBuf::from),
        }),
        image_mode,
        keymap,
        mouse,
        show_hints: cfg_file.hints,
        sidebar: saved_ui.sidebar.unwrap_or(cfg_file.sidebar),
        request,
        select,
        limits: rada_core::preview::Limits {
            image: cfg_file.image_limits.clone(),
            archive_entries: cfg_file.archive_preview_entries,
            archive_seconds: cfg_file.archive_preview_seconds,
            ..Default::default()
        },
    };

    let outcome = rada_tui::run::run(cfg, services).context("terminal error")?;
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
    let appender = tracing_appender::rolling::daily(&dir, "rada.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = tracing_subscriber::EnvFilter::try_from_env("RADA_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(filter)
        .init();
    Some(guard)
}
