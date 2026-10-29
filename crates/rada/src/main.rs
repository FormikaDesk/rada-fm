//! rada: a terminal file manager you can trust.

mod cli_text;
mod config;
mod demo;
mod desktop;
mod filepicker;
mod fonts;
mod fontsetup;
mod icons;
mod macapp;
mod setup;
mod shell;
mod shellsetup;
mod update_cmd;
mod winfolder;
mod winsetup;

use std::path::PathBuf;
use std::process::ExitCode;
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
    /// What rada sees of the machine, for finding out why something is not there.
    #[command(hide = true)]
    Debug {
        #[command(subcommand)]
        what: DebugWhat,
    },
}

#[derive(Subcommand, Debug)]
enum DebugWhat {
    /// Every disk, phone, camera and stick the sidebar could list, with where it came from and
    /// why each one that is left out is left out. Reads only; nothing is mounted.
    Devices,
}

#[derive(Subcommand, Debug)]
enum SetupWhat {
    /// Add rada to the application menu (Linux), make Rada.app in ~/Applications (macOS) or a
    /// Start menu shortcut (Windows), for this user only.
    Desktop {
        /// Take away exactly what a previous setup installed.
        #[arg(long)]
        remove: bool,
        /// Also make rada the default program for folders (Linux: the previous one is
        /// remembered and `--remove` restores it; Windows: for your account only, shows what
        /// it writes in the registry and asks first, and `--remove` puts it back).
        #[arg(long)]
        default_file_manager: bool,
        /// Also put a launcher on the Desktop (macOS and Windows).
        #[arg(long)]
        desktop: bool,
        /// With `--default-file-manager` on Windows: do not ask before writing the registry.
        #[arg(short, long)]
        yes: bool,
    },
    /// Download and install JetBrainsMono Nerd Font Mono for this user (a fixed release,
    /// checked against its checksums; nothing needs administrator rights), so that the icons draw.
    Font {
        /// Take away what a previous setup installed.
        #[arg(long)]
        remove: bool,
    },
    /// Make rada the system's file chooser (Linux, through xdg-desktop-portal-termfilechooser):
    /// shows what it would write and where, and writes only after you say yes. Copies of the
    /// files it changes are kept; nothing needs administrator rights and no package is installed.
    FilePicker {
        /// Go back: restore the files that were changed and take away the ones that were made.
        #[arg(long)]
        remove: bool,
        /// Do not ask before writing.
        #[arg(short, long, conflicts_with = "remove")]
        yes: bool,
    },
    /// Make the shell follow the folder rada was left in (`v` instead of `rada`): writes the
    /// call of `rada --init` in the right place for your shell, after showing it and asking.
    Shell {
        /// The shell: bash, zsh, fish, nushell or powershell (default: `$SHELL`).
        #[arg(long, value_name = "NAME")]
        shell: Option<String>,
        /// Take away exactly what a previous setup wrote.
        #[arg(long)]
        remove: bool,
        /// Do not ask before writing.
        #[arg(short, long, conflicts_with = "remove")]
        yes: bool,
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
    /// catppuccin-mocha, catppuccin-latte, tokyo-night-dark, tokyo-night-day, nebbia-light, carta.
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

    /// Act as a file chooser: write the paths chosen to FILE, one per line, and quit. Enter
    /// chooses, Esc cancels and writes nothing. Nothing can be deleted, moved or renamed in this
    /// mode.
    #[arg(long, value_name = "FILE")]
    chooser_file: Option<PathBuf>,

    /// With --chooser-file: more than one file (or folder) can be chosen.
    #[arg(long, requires = "chooser_file", conflicts_with = "chooser_save")]
    chooser_multiple: bool,

    /// With --chooser-file: choose a folder instead of a file.
    #[arg(long, requires = "chooser_file", conflicts_with = "chooser_save")]
    chooser_dir: bool,

    /// With --chooser-file: choose where to save, with the name to suggest
    /// (`--chooser-save=NAME`; the folder is the path given).
    #[arg(
        long,
        value_name = "NAME",
        requires = "chooser_file",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = ""
    )]
    chooser_save: Option<String>,

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

/// Set by the launchers of Rada.app: the window belongs to rada and closes with it.
const FROM_APP: &str = "RADA_FROM_APP";

fn main() -> ExitCode {
    let mut log_guard = None;
    let Err(e) = run(&mut log_guard) else {
        return ExitCode::SUCCESS;
    };
    // The log is the only place left to look at when the window is gone.
    log_fatal(&e);
    drop(log_guard);
    let said = fatal_text(&e);
    if std::env::var_os(FROM_APP).is_some_and(|v| v == "1") {
        // Started from Rada.app: the window closes with rada, and with it the message.
        eprintln!("\nError: {said}\n\n{}", Key::MPressAnyKey.tr());
        rada_tui::run::wait_for_key();
        std::process::exit(1);
    }
    eprintln!("Error: {said}");
    ExitCode::FAILURE
}

/// An error that ends rada, as it is shown: the message and its causes, as `{:?}` writes them.
/// What the causes carry may come from outside (an answer from the network, the output of a
/// program, a name on the disk), so nothing in it can act on the terminal.
fn fatal_text(e: &anyhow::Error) -> String {
    rada_core::display::block(&format!("{e:?}"))
}

/// An error that ends rada, in the log with every cause in the chain (`a: b: c`).
fn log_fatal(e: &anyhow::Error) {
    tracing::error!("rada ends with an error: {e:#}");
}

/// Windows: started from the old file of an update that stopped before the new program took
/// its place, so that the program's own name is empty. The name goes back to this file, and
/// that is all this run does.
fn put_name_back_after_cut_update() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(name) = rada_core::update::install::restore_missing(&exe) else {
        return false;
    };
    println!("{}", tf(Key::UpRestored, &[&name.display()]));
    true
}

fn run(log_guard: &mut Option<tracing_appender::non_blocking::WorkerGuard>) -> Result<()> {
    run_launch_helper_if_asked();
    early_language();
    if cfg!(windows) && put_name_back_after_cut_update() {
        return Ok(());
    }
    let matches = cli_text::command::<Cli>().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    if run_exiting_command(&cli)? {
        return Ok(());
    }
    let request = read_request(&cli)?;
    let (dirs, cfg_file) = open_state(log_guard)?;
    let caps = detect_terminal(&cli, &cfg_file)?;
    let start = start_place(&cli)?;
    let icons_name = icons_setting(&cli, &cfg_file);
    let icons = pick_icons(icons_name.as_deref(), &dirs, &caps)?;
    if let Some(Command::Doctor) = &cli.command {
        print_doctor(&caps, icons_name.as_deref(), icons, &dirs);
        return Ok(());
    }
    let plain_start = request.is_none() && cli.demo.is_none() && start.chooser.is_none();
    let font_offer = font_offer_if_due(icons_name.as_deref(), icons, &dirs, plain_start);
    let (theme, adaptive) = pick_theme(&cli, &cfg_file, &caps)?;
    let image_mode = pick_image_mode(&cli, &cfg_file, &caps)?;
    let keymap = pick_keymap(&cli, &cfg_file)?;
    let services = start_services(&cli, &cfg_file, &dirs, start.chooser.is_some())?;
    let prefs = pick_view_prefs(&cfg_file, &dirs)?;

    let cfg = build_config(
        &cli,
        &cfg_file,
        &dirs,
        Picks {
            request,
            start,
            icons,
            theme,
            adaptive,
            image_mode,
            keymap,
            font_offer,
            prefs,
            basic: caps.profile == rada_tui::Profile::Basic,
        },
    );
    let outcome = rada_tui::run::run(cfg, services, caps).context(Key::MTerminalError.tr())?;
    write_outputs(&cli, &outcome)
}

fn run_launch_helper_if_asked() {
    // Started by rada itself to call the shell from a process with no console (see
    // `rada_core::platform::launch`): it does that one thing and ends, before anything of the
    // interface or of the command line is touched.
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Some(task) = rada_core::platform::launch::parse_helper_args(&raw) {
        std::process::exit(rada_core::platform::windows::run_launch_helper(&task));
    }
}

/// Runs the command that ends rada before the interface starts, if there is one: `true` then.
fn run_exiting_command(cli: &Cli) -> Result<bool> {
    if let Some(Command::Setup { what }) = &cli.command {
        run_setup(what)?;
        return Ok(true);
    }
    if let Some(Command::Debug {
        what: DebugWhat::Devices,
    }) = &cli.command
    {
        let dirs = Dirs::from_env().context(Key::MNoDirs.tr())?;
        let platform = platform::current(dirs);
        for line in platform.volumes().diagnose() {
            println!("{}", rada_core::display::text(&line));
        }
        return Ok(true);
    }
    if let Some(Command::Update {
        check,
        rollback,
        yes,
    }) = &cli.command
    {
        let dirs = Dirs::from_env().context(Key::MNoDirs.tr())?;
        rada_core::statefile::tighten(&dirs.rada_state());
        let cfg_file = config::load(&dirs);
        cfg_file.apply_language();
        let settings = cfg_file.update_settings(&dirs.home)?;
        let ctx = update_context(&dirs, settings)?;
        update_cmd::run(ctx, *check, *rollback, *yes)?;
        return Ok(true);
    }
    if cli.spawn_terminal {
        desktop::spawn_terminal(cli.path.first().map(PathBuf::as_path))?;
        return Ok(true);
    }
    if let Some(shell) = &cli.init {
        print!("{}", shell::snippet(shell));
        return Ok(true);
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
        return Ok(true);
    }
    Ok(false)
}

fn run_setup(what: &SetupWhat) -> Result<()> {
    let dirs = Dirs::from_env().context(Key::MNoDirs.tr())?;
    let lines = match what {
        SetupWhat::Desktop {
            remove,
            default_file_manager,
            desktop,
            yes,
        } => setup::desktop(
            &dirs,
            &setup::DesktopOptions {
                remove: *remove,
                default_file_manager: *default_file_manager,
                desktop: *desktop,
                yes: *yes,
            },
            &mut std::io::BufReader::new(std::io::stdin()),
        )?,
        SetupWhat::Font { remove } => setup::font(&dirs, *remove)?,
        SetupWhat::Shell { shell, remove, yes } => shellsetup::run(
            &dirs,
            shell.as_deref(),
            *remove,
            *yes,
            &mut std::io::BufReader::new(std::io::stdin()),
        )?,
        SetupWhat::FilePicker { remove, yes } => filepicker::run(
            &dirs,
            *remove,
            *yes,
            &mut std::io::BufReader::new(std::io::stdin()),
        )?,
    };
    for l in lines {
        println!("{}", rada_core::display::block(&l));
    }
    Ok(())
}

fn read_request(cli: &Cli) -> Result<Option<rada_core::ops::OpRequest>> {
    if cli.chooser_file.is_some() && (cli.request.is_some() || cli.demo.is_some()) {
        anyhow::bail!("{}", Key::MChooserWithRequest.tr());
    }
    Ok(match &cli.request {
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
    })
}

fn open_state(
    log_guard: &mut Option<tracing_appender::non_blocking::WorkerGuard>,
) -> Result<(Dirs, config::FileConfig)> {
    let dirs = Dirs::from_env().context(Key::MNoDirs.tr())?;
    // A state folder from an earlier version may be open to other users: close it first.
    rada_core::statefile::tighten(&dirs.rada_state());
    *log_guard = init_logging(&dirs);
    let cfg_file = config::load(&dirs);
    cfg_file.apply_language();
    Ok((dirs, cfg_file))
}

fn detect_terminal(cli: &Cli, cfg_file: &config::FileConfig) -> Result<rada_tui::TermCaps> {
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
    Ok(rada_tui::TermCaps::detect_here(forced_profile))
}

struct StartPlace {
    dir: PathBuf,
    select: Option<std::ffi::OsString>,
    chooser: Option<rada_tui::chooser::Chooser>,
}

fn start_place(cli: &Cli) -> Result<StartPlace> {
    let start = match cli.path.first() {
        Some(p) => p.clone(),
        None => std::env::current_dir().context(Key::MNoCwd.tr())?,
    };
    // A file chooser may be given a path that is not there yet (where to save, and as what):
    // it starts in the nearest folder that is, and the name is the one to suggest.
    let saving = cli.chooser_save.is_some();
    let chosen_start = cli
        .chooser_file
        .is_some()
        .then(|| rada_tui::chooser::start_from(&start, saving));
    let start = match &chosen_start {
        Some(s) => s.dir.clone(),
        None => start,
    };
    let start = rada_core::fs::canonical_path(&start)
        .with_context(|| tf(Key::MCannotOpen, &[&start.display()]))?;
    // Given a file, open its folder with the cursor on it.
    let (start, select) = match &chosen_start {
        Some(s) => (start, s.select.clone()),
        None if start.is_dir() => (start, None),
        None => {
            let name = start.file_name().map(|n| n.to_os_string());
            (start.parent().map(PathBuf::from).unwrap_or(start), name)
        }
    };
    let chooser = cli.chooser_file.as_ref().map(|_| {
        use rada_tui::chooser::Chooser;
        match &cli.chooser_save {
            Some(name) => {
                let suggested = if name.is_empty() {
                    chosen_start
                        .as_ref()
                        .and_then(|s| s.name.clone())
                        .unwrap_or_default()
                } else {
                    name.clone()
                };
                Chooser::save(suggested)
            }
            None if cli.chooser_dir => Chooser::folder(cli.chooser_multiple),
            None => Chooser::file(cli.chooser_multiple),
        }
    });
    Ok(StartPlace {
        dir: start,
        select,
        chooser,
    })
}

fn icons_setting(cli: &Cli, cfg_file: &config::FileConfig) -> Option<String> {
    cli.icons
        .clone()
        .or_else(|| std::env::var("RADA_ICONS").ok())
        .or(cfg_file.icons.clone())
}

fn pick_icons(
    icons_name: Option<&str>,
    dirs: &Dirs,
    caps: &rada_tui::TermCaps,
) -> Result<rada_tui::IconSet> {
    let basic = caps.profile == rada_tui::Profile::Basic;
    let windows_terminal = caps.terminal == rada_tui::caps::TerminalKind::WindowsTerminal;
    Ok(match icons_name {
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
    })
}

fn print_doctor(
    caps: &rada_tui::TermCaps,
    icons_name: Option<&str>,
    icons: rada_tui::IconSet,
    dirs: &Dirs,
) {
    let windows_terminal = caps.terminal == rada_tui::caps::TerminalKind::WindowsTerminal;
    for line in caps.describe() {
        println!("{}", rada_core::display::text(&line));
    }
    // Why this set: the font the terminal is set to draw with, when it could be read.
    let chosen = !matches!(icons_name, None | Some("auto"));
    match fonts::terminal_nerd_font(&dirs.home, windows_terminal) {
        _ if chosen => println!("{}", tf(Key::DocIconsChosen, &[&icons.name()])),
        Some(font) => println!(
            "{}",
            tf(
                Key::DocIconsNerd,
                &[&icons.name(), &rada_core::display::text(font.trim())]
            )
        ),
        None => println!("{}", tf(Key::DocIconsNone, &[&icons.name()])),
    }
}

fn font_offer_if_due(
    icons_name: Option<&str>,
    icons: rada_tui::IconSet,
    dirs: &Dirs,
    plain_start: bool,
) -> Option<rada_tui::FontOffer> {
    // The offer to install a Nerd Font: only where the icons were left to rada to decide, they
    // would not draw (one of the Unicode sets stands in for them), and it has not been made
    // before. Never made without being asked.
    (matches!(icons_name, None | Some("auto"))
        && matches!(
            icons,
            rada_tui::IconSet::UnicodeIcons | rada_tui::IconSet::UnicodeStd
        )
        && plain_start
        && !font_offered(dirs))
    .then(|| make_font_offer(dirs))
}

fn pick_theme(
    cli: &Cli,
    cfg_file: &config::FileConfig,
    caps: &rada_tui::TermCaps,
) -> Result<(Theme, Option<rada_tui::app::Adaptive>)> {
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
    Ok((theme, adaptive))
}

fn pick_image_mode(
    cli: &Cli,
    cfg_file: &config::FileConfig,
    caps: &rada_tui::TermCaps,
) -> Result<rada_tui::ImageMode> {
    let images_name = cli
        .images
        .clone()
        .or_else(|| std::env::var("RADA_IMAGES").ok())
        .or(cfg_file.images.clone());
    Ok(match images_name {
        Some(n) => rada_tui::ImageMode::parse(&n).with_context(|| tf(Key::MBadImages, &[&n]))?,
        None if caps.images == rada_tui::caps::ImagePlan::Off => rada_tui::ImageMode::Off,
        None => rada_tui::ImageMode::Auto,
    })
}

fn pick_keymap(cli: &Cli, cfg_file: &config::FileConfig) -> Result<rada_tui::keymap::Keymap> {
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
    Ok(rada_tui::keymap::Keymap::new(preset, &cfg_file.keys))
}

fn start_services(
    cli: &Cli,
    cfg_file: &config::FileConfig,
    dirs: &Dirs,
    chooser: bool,
) -> Result<Services> {
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
        .with_folder_sizes(cfg_file.folder_size_settings()?)
        // A demo scene must not depend on, or change, the folders the person goes to.
        .with_zoxide(rada_core::zoxide::Zoxide::new(
            cfg_file.zoxide && cli.demo.is_none(),
            cfg_file
                .zoxide_path
                .as_deref()
                .map(std::path::PathBuf::from),
        ))
        .with_git(rada_core::git::Git::new(
            cfg_file.git,
            cfg_file.git_path.as_deref().map(std::path::PathBuf::from),
        ));
    // Updates: the daily check starts once the interface is up. Not in a demo scene, whose
    // screens must not depend on what GitHub says today.
    Ok(if cli.demo.is_some() || chooser {
        services
    } else {
        match update_context(dirs, cfg_file.update_settings(&dirs.home)?) {
            Ok(ctx) => services.with_updates(ctx),
            Err(e) => {
                tracing::warn!("updates are off: {e}");
                services
            }
        }
    })
}

struct ViewPrefs {
    view: rada_tui::ViewMode,
    dates: rada_tui::fmt::DateStyle,
    sidebar_density: rada_tui::sidebar::Density,
    nav_mode: rada_tui::panels::NavMode,
    saved_ui: rada_core::uistate::UiState,
    layout: rada_tui::LayoutKind,
    layout_pref: rada_tui::LayoutPref,
}

fn pick_view_prefs(cfg_file: &config::FileConfig, dirs: &Dirs) -> Result<ViewPrefs> {
    let view = match cfg_file.view.as_deref() {
        None => rada_tui::ViewMode::Details,
        Some(n) => rada_tui::ViewMode::parse(n).with_context(|| tf(Key::MBadView, &[&n]))?,
    };
    let dates = match cfg_file.dates.as_deref() {
        None => rada_tui::fmt::DateStyle::Relative,
        Some(n) => rada_tui::fmt::DateStyle::parse(n).with_context(|| tf(Key::MBadDates, &[&n]))?,
    };
    let sidebar_density = match cfg_file.sidebar_density.as_deref() {
        None => rada_tui::sidebar::Density::Auto,
        Some(n) => rada_tui::sidebar::Density::parse(n).with_context(|| {
            tf(
                Key::MBadDensity,
                &[&n, &rada_tui::sidebar::Density::NAMES.join(", ")],
            )
        })?,
    };
    let nav_mode = match cfg_file.nav.as_deref() {
        None => rada_tui::panels::NavMode::default(),
        Some(n) => rada_tui::panels::NavMode::parse(n).with_context(|| tf(Key::MBadNav, &[&n]))?,
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
    // What was chosen in the View menu is used in place of the configuration's layout, as for
    // the panes.
    let (layout, layout_pref) =
        rada_tui::panels::start_layout(cfg_file.layout.as_deref(), saved_ui.layout.as_deref())
            .with_context(|| {
                tf(
                    Key::MBadLayout,
                    &[&cfg_file.layout.as_deref().unwrap_or_default()],
                )
            })?;
    Ok(ViewPrefs {
        view,
        dates,
        sidebar_density,
        nav_mode,
        saved_ui,
        layout,
        layout_pref,
    })
}

struct Picks {
    request: Option<rada_core::ops::OpRequest>,
    start: StartPlace,
    icons: rada_tui::IconSet,
    theme: Theme,
    adaptive: Option<rada_tui::app::Adaptive>,
    image_mode: rada_tui::ImageMode,
    keymap: rada_tui::keymap::Keymap,
    font_offer: Option<rada_tui::FontOffer>,
    prefs: ViewPrefs,
    basic: bool,
}

fn build_config(cli: &Cli, cfg_file: &config::FileConfig, dirs: &Dirs, picks: Picks) -> Config {
    let Picks {
        request,
        start,
        icons,
        theme,
        adaptive,
        image_mode,
        keymap,
        font_offer,
        prefs,
        basic,
    } = picks;
    let ViewPrefs {
        view,
        dates,
        sidebar_density,
        nav_mode,
        saved_ui,
        layout,
        layout_pref,
    } = prefs;
    let chooser = start.chooser;
    let bookmarks = cfg_file
        .bookmarks
        .iter()
        .map(|b| rada_tui::palette::expand(b, &dirs.home))
        .collect();
    let mouse = cfg_file.mouse && !cli.no_mouse && std::env::var_os("RADA_NO_MOUSE").is_none();
    Config {
        start_explicit: !cli.path.is_empty(),
        layout,
        layout_pref,
        view,
        dates,
        sidebar_density,
        nav_mode,
        basic,
        tags_column: cfg_file.tags_column,
        chooser: chooser.clone(),
        date_format: rada_tui::fmt::DateFormat::from_env(),
        // A chooser does not bring back the tabs of the last session, and does not write them.
        remember_tabs: cfg_file.remember_tabs && chooser.is_none(),
        // What the user chose with Alt+P wins over the file's setting.
        details_pane: saved_ui
            .details
            .or_else(|| (!cfg_file.show_details_pane).then_some(false)),
        saved_ui: saved_ui.clone(),
        start_dir: start.dir,
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
        select: start.select,
        limits: rada_core::preview::Limits {
            image: cfg_file.image_limits.clone(),
            archive_entries: cfg_file.archive_preview_entries,
            archive_seconds: cfg_file.archive_preview_seconds,
            syntax_highlight: cfg_file.syntax_highlight,
            ..Default::default()
        },
    }
}

fn write_outputs(cli: &Cli, outcome: &rada_tui::run::Outcome) -> Result<()> {
    if let Some(f) = &cli.cwd_file {
        write_cwd(f, &outcome.last_dir).with_context(|| tf(Key::MCannotWrite, &[&f.display()]))?;
    }
    // The answer of a file chooser: written once the terminal is given back, and not at all
    // when the person said no.
    if let (Some(f), Some(paths)) = (&cli.chooser_file, &outcome.chosen) {
        std::fs::write(f, rada_tui::chooser::render(paths))
            .with_context(|| tf(Key::MCannotWrite, &[&f.display()]))?;
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
            let _ =
                rada_core::statefile::write_atomic(&font_offered_file(&note_dirs), b"offered\n");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Buffer {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_fatal_error_is_logged_with_every_cause_in_its_chain() {
        let buf = Buffer::default();
        let b = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || b.clone())
            .with_ansi(false)
            .finish();
        let err = Err::<(), _>(std::io::Error::other(
            "The cursor position could not be read within a normal duration",
        ))
        .context("terminal error")
        .context("rada cannot go on")
        .unwrap_err();
        tracing::subscriber::with_default(subscriber, || log_fatal(&err));
        let text = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(text.contains("ERROR"), "{text}");
        assert!(
            text.contains(
                "rada cannot go on: terminal error: The cursor position could not be read within a normal duration"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_fatal_error_is_shown_with_its_causes_and_without_what_acts_on_the_terminal() {
        let err = Err::<(), _>(std::io::Error::other(
            "answer: \x1b]0;pwned\x07\x1b[2J and \u{202e}more\nsecond line",
        ))
        .context("cannot reach \x1b[31mhost")
        .unwrap_err();
        let said = fatal_text(&err);
        assert!(
            !said
                .chars()
                .any(|c| c != '\n' && rada_core::display::is_dangerous(c))
        );
        assert!(said.contains("cannot reach \\e[31mhost"), "{said}");
        assert!(said.contains("Caused by"), "the chain stays: {said}");
        assert!(
            said.contains("\\e]0;pwned\\u{7}\\e[2J and \\u{202e}more\n    second line"),
            "{said}"
        );
        // An ordinary message is what `{:?}` writes.
        let plain = anyhow::anyhow!("plain").context("outer");
        assert_eq!(fatal_text(&plain), format!("{plain:?}"));
    }
}
