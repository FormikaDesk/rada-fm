//! rada: a terminal file manager you can trust.

mod cli_text;
mod config;
mod demo;
mod desktop;
mod fonts;
mod fontsetup;
mod icons;
mod macapp;
mod setup;
mod shell;
mod update_cmd;
mod winsetup;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{FromArgMatches, Parser, Subcommand};
use rada_core::fs::LocalFs;
use rada_core::i18n::{Key, tf};
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
    /// Look for a newer rada and install it. Reads GitHub's public list of releases and sends
    /// nothing but `User-Agent: rada/<version>`.
    Update {
        /// Only say whether a newer version exists, and what is in it.
        #[arg(long)]
        check: bool,
        /// Go back to the version that was running before the last update.
        #[arg(long, conflicts_with = "check")]
        rollback: bool,
        /// Do not ask before installing.
        #[arg(short, long)]
        yes: bool,
    },
    /// Say what rada found out about this terminal, and which profile it will use.
    Doctor,
}

#[derive(Subcommand, Debug)]
enum SetupWhat {
    /// Add rada to the application menu (Linux), make Rada.app in ~/Applications (macOS) or a
    /// Start menu shortcut (Windows), for this user only.
    Desktop {
        /// Take away exactly what a previous setup installed.
        #[arg(long)]
        remove: bool,
        /// Also make rada the default program for folders (the previous one is
        /// remembered, and `--remove` restores it).
        #[arg(long)]
        default_file_manager: bool,
        /// Also put a launcher on the Desktop (macOS and Windows).
        #[arg(long)]
        desktop: bool,
    },
    /// Download and install JetBrainsMono Nerd Font Mono for this user (a fixed release,
    /// checked against its checksums; nothing needs administrator rights), so that the icons draw.
    Font {
        /// Take away what a previous setup installed.
        #[arg(long)]
        remove: bool,
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

    /// Icons: `auto` (default: the best set the terminal's font can draw; Nerd Font symbols
    /// if the terminal uses such a font, or is Ghostty), `nerd`, `unicode-icons`,
    /// `unicode-std` or `none`.
    #[arg(long, value_name = "SET")]
    icons: Option<String>,

    /// How much is asked of the terminal: `auto` (default), `modern` (UTF-8, colours,
    /// mouse, pictures) or `basic` (ASCII, 16 colours, keyboard only).
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,

    /// Image previews: `auto` (default), `halfblocks`, `kitty`, `sixel`, `iterm2` or `off`.
    #[arg(long, value_name = "MODE")]
    images: Option<String>,

    /// Colour theme: `auto` (default), rada, catppuccin or tokyo-night (these follow the
    /// terminal's background), or one fixed variant: rada-dark, rada-light,
    /// catppuccin-mocha, catppuccin-latte, tokyo-night-dark, tokyo-night-day, nebbia-light.
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

/// The language the process speaks before anything is parsed, so that even `--help` has it:
/// the one the config file asks for, or the system's.
fn early_language() {
    use rada_core::i18n::{Lang, set_chosen_lang, set_lang};
    let chosen = Dirs::from_env()
        .ok()
        .and_then(|d| config::chosen_language(&d))
        .and_then(|l| Lang::parse(&l));
    match chosen {
        Some(l) => set_chosen_lang(l),
        None => set_lang(Lang::system()),
    }
}

fn main() -> Result<()> {
    // Started by rada itself to call the shell from a process with no console (see
    // `rada_core::platform::launch`): it does that one thing and ends, before anything of the
    // interface or of the command line is touched.
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Some(task) = rada_core::platform::launch::parse_helper_args(&raw) {
        std::process::exit(rada_core::platform::windows::run_launch_helper(&task));
    }
    early_language();
    let matches = cli_text::command::<Cli>().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    if let Some(Command::Setup { what }) = &cli.command {
        let dirs = Dirs::from_env().context(Key::MNoDirs.tr())?;
        let lines = match what {
            SetupWhat::Desktop {
                remove,
                default_file_manager,
                desktop,
            } => setup::desktop(
                &dirs,
                &setup::DesktopOptions {
                    remove: *remove,
                    default_file_manager: *default_file_manager,
                    desktop: *desktop,
                },
            )?,
            SetupWhat::Font { remove } => setup::font(&dirs, *remove)?,
        };
        for l in lines {
            println!("{l}");
        }
        return Ok(());
    }
    if let Some(Command::Update {
        check,
        rollback,
        yes,
    }) = &cli.command
    {
        let dirs = Dirs::from_env().context(Key::MNoDirs.tr())?;
        let cfg_file = config::load(&dirs);
        cfg_file.apply_language();
        let settings = cfg_file.update_settings(&dirs.home)?;
        let ctx = update_context(&dirs, settings)?;
        return update_cmd::run(ctx, *check, *rollback, *yes);
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
                std::io::read_to_string(std::io::stdin()).context(Key::MRequestStdin.tr())?
            } else {
                std::fs::read_to_string(src).with_context(|| tf(Key::MCannotRead, &[&src]))?
            };
            let req: rada_core::ops::OpRequest =
                serde_json::from_str(&text).context(Key::MRequestBad.tr())?;
            req.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
            Some(req)
        }
        None => None,
    };
    let dirs = Dirs::from_env().context(Key::MNoDirs.tr())?;
    let _log_guard = init_logging(&dirs);
    let cfg_file = config::load(&dirs);
    cfg_file.apply_language();

    // What the terminal can do comes first: the icons, the colours and the pictures follow.
    let profile_name = cli
        .profile
        .clone()
        .or_else(|| std::env::var("RADA_PROFILE").ok())
        .or(cfg_file.profile.clone());
    let forced_profile = match profile_name {
        Some(n) => rada_tui::Profile::parse(&n).with_context(|| tf(Key::MBadProfile, &[&n]))?,
        None => None,
    };
    let caps = rada_tui::TermCaps::detect_here(forced_profile);
    let basic = caps.profile == rada_tui::Profile::Basic;
    let windows_terminal = caps.terminal == rada_tui::caps::TerminalKind::WindowsTerminal;

    let start = match cli.path.first() {
        Some(p) => p.clone(),
        None => std::env::current_dir().context(Key::MNoCwd.tr())?,
    };
    let start = rada_core::fs::canonical_path(&start)
        .with_context(|| tf(Key::MCannotOpen, &[&start.display()]))?;
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
        // A terminal that draws only ASCII has no icons to show.
        None | Some("auto") if basic => rada_tui::IconSet::None,
        // The best set the terminal's font can draw: Nerd Font symbols when the terminal is
        // set to use such a font; Unicode symbols when the system has a symbol font for the
        // terminal to fall back on; otherwise only the glyphs every monospace font has. The
        // classic Windows console draws with its one font and falls back on nothing.
        None | Some("auto") => {
            // Ghostty comes with a Nerd Font symbols fallback: its icons draw with no setup.
            if fonts::terminal_uses_nerd_font(&dirs.home, windows_terminal)
                || fontsetup::running_in_ghostty()
            {
                rada_tui::IconSet::Nerd
            } else if caps.terminal != rada_tui::caps::TerminalKind::WindowsConsole
                && fonts::system_has_symbol_font(&dirs.home)
            {
                rada_tui::IconSet::UnicodeIcons
            } else {
                rada_tui::IconSet::UnicodeStd
            }
        }
        Some(n) => rada_tui::IconSet::parse(n)
            .with_context(|| tf(Key::MBadIcons, &[&n, &rada_tui::IconSet::NAMES.join(", ")]))?,
    };
    if let Some(Command::Doctor) = &cli.command {
        for line in caps.describe() {
            println!("{line}");
        }
        // Why this set: the font the terminal is set to draw with, when it could be read.
        let chosen = !matches!(icons_name.as_deref(), None | Some("auto"));
        match fonts::terminal_nerd_font(&dirs.home, windows_terminal) {
            _ if chosen => println!("{}", tf(Key::DocIconsChosen, &[&icons.name()])),
            Some(font) => println!("{}", tf(Key::DocIconsNerd, &[&icons.name(), &font.trim()])),
            None => println!("{}", tf(Key::DocIconsNone, &[&icons.name()])),
        }
        return Ok(());
    }

    // The offer to install a Nerd Font: only where the icons were left to rada to decide, they
    // would not draw (one of the Unicode sets stands in for them), and it has not been made
    // before. Never made without being asked.
    let font_offer = (matches!(icons_name.as_deref(), None | Some("auto"))
        && matches!(
            icons,
            rada_tui::IconSet::UnicodeIcons | rada_tui::IconSet::UnicodeStd
        )
        && request.is_none()
        && cli.demo.is_none()
        && !font_offered(&dirs))
    .then(|| make_font_offer(&dirs));

    let depth = caps.depth;
    let theme_name = cli
        .theme
        .clone()
        .or_else(|| std::env::var("RADA_THEME").ok())
        .or(cfg_file.theme.clone())
        .unwrap_or_else(|| "auto".into());
    let follows = Theme::follows_background(&theme_name)
        .with_context(|| tf(Key::MBadTheme, &[&theme_name, &Theme::NAMES.join(", ")]))?;
    let appearance_name = cli
        .appearance
        .clone()
        .or_else(|| std::env::var("RADA_APPEARANCE").ok())
        .or(cfg_file.appearance.clone());
    let forced = match appearance_name {
        Some(n) => rada_tui::termtheme::Appearance::parse(&n)
            .with_context(|| tf(Key::MBadAppearance, &[&n]))?,
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
                transparent: cfg_file.transparent_background,
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
    // `transparent_background`: the default dark theme leaves the terminal's background.
    let theme = if cfg_file.transparent_background {
        theme.transparent()
    } else {
        theme
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
        Some(n) => rada_tui::ImageMode::parse(&n).with_context(|| tf(Key::MBadImages, &[&n]))?,
        None if caps.images == rada_tui::caps::ImagePlan::Off => rada_tui::ImageMode::Off,
        None => rada_tui::ImageMode::Auto,
    };

    let preset_name = cli
        .keymap
        .clone()
        .or_else(|| std::env::var("RADA_KEYMAP").ok())
        .or(cfg_file.keymap.clone());
    let preset = match preset_name {
        Some(n) => rada_tui::keymap::Preset::parse(&n).with_context(|| {
            tf(
                Key::MBadKeymap,
                &[&n, &rada_tui::keymap::Preset::NAMES.join(", ")],
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
    let services = Services::start_with(fs, platform, journal, cfg_file.archive_limits)
        .with_folder_sizes(cfg_file.folder_size_settings()?);
    // Updates: the daily check starts once the interface is up. Not in a demo scene, whose
    // screens must not depend on what GitHub says today.
    let services = if cli.demo.is_some() {
        services
    } else {
        match update_context(&dirs, cfg_file.update_settings(&dirs.home)?) {
            Ok(ctx) => services.with_updates(ctx),
            Err(e) => {
                tracing::warn!("updates are off: {e}");
                services
            }
        }
    };
    let layout = match cfg_file.layout.as_deref() {
        None => rada_tui::LayoutKind::Explorer,
        Some(n) => rada_tui::LayoutKind::parse(n).with_context(|| tf(Key::MBadLayout, &[&n]))?,
    };
    let view = match cfg_file.view.as_deref() {
        None => rada_tui::ViewMode::Details,
        Some(n) => rada_tui::ViewMode::parse(n).with_context(|| tf(Key::MBadView, &[&n]))?,
    };
    let dates = match cfg_file.dates.as_deref() {
        None => rada_tui::fmt::DateStyle::Relative,
        Some(n) => rada_tui::fmt::DateStyle::parse(n).with_context(|| tf(Key::MBadDates, &[&n]))?,
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
        basic,
        tags_column: cfg_file.tags_column,
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
        cmd_keys: cfg_file.cmd_keys,
        font_offer,
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

    let outcome = rada_tui::run::run(cfg, services, caps).context(Key::MTerminalError.tr())?;
    if let Some(f) = &cli.cwd_file {
        write_cwd(f, &outcome.last_dir).with_context(|| tf(Key::MCannotWrite, &[&f.display()]))?;
    }
    Ok(())
}

fn font_offered_file(dirs: &Dirs) -> PathBuf {
    dirs.rada_state().join("font-offered")
}

/// Has the offer of a Nerd Font been made before?
fn font_offered(dirs: &Dirs) -> bool {
    font_offered_file(dirs).exists()
}

/// The offer: what is shown, what installing does, and the note that it was made.
fn make_font_offer(dirs: &Dirs) -> rada_tui::FontOffer {
    let setup = fontsetup::FontSetup::for_this_system(dirs);
    let total: u64 = setup.faces.iter().map(|f| f.size).sum();
    let (install_dirs, note_dirs) = (dirs.clone(), dirs.clone());
    rada_tui::FontOffer {
        family: fontsetup::FAMILY.to_string(),
        megabytes: total as f32 / 1_000_000.0,
        install: Arc::new(move || {
            let setup = fontsetup::FontSetup::for_this_system(&install_dirs);
            setup.install(&mut |_| {}).map_err(|e| e.to_string())?;
            Ok(tf(Key::MFontInstalled, &[&fontsetup::FAMILY]))
        }),
        shown: Arc::new(move || {
            let file = font_offered_file(&note_dirs);
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(file, "offered\n");
        }),
    }
}

/// What an update needs to know about this program and where its files are.
fn update_context(
    dirs: &Dirs,
    settings: rada_core::update::Settings,
) -> Result<rada_core::update::Ctx> {
    rada_core::update::Ctx::for_this_program(
        env!("CARGO_PKG_VERSION"),
        settings,
        dirs.rada_state(),
        dirs.home.clone(),
    )
    .map_err(anyhow::Error::msg)
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
