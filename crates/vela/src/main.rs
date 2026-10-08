//! vela: a terminal file manager you can trust.

mod config;
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

    /// Icons: `nerd` (needs a Nerd Font), `unicode` (default) or `none`.
    #[arg(long, value_name = "SET")]
    icons: Option<String>,

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
    let start = if start.is_dir() {
        start
    } else {
        start.parent().map(PathBuf::from).unwrap_or(start)
    };

    let icons_name = cli
        .icons
        .clone()
        .or_else(|| std::env::var("VELA_ICONS").ok())
        .or(cfg_file.icons.clone());
    let icons = match icons_name {
        Some(n) => vela_tui::IconSet::parse(&n)
            .with_context(|| format!("unknown icon set {n:?} (use nerd, unicode or none)"))?,
        None => vela_tui::IconSet::Unicode,
    };

    let platform = platform::current(dirs.clone());
    let journal = match Journal::open(dirs.journal_path()) {
        Ok(j) => Some(j),
        Err(e) => {
            tracing::error!("journal unavailable: {e}");
            None
        }
    };
    let services = Services::start(Arc::new(LocalFs), platform, journal);
    let cfg = Config {
        start_dir: start,
        icons,
        show_hidden: cli.hidden || cfg_file.show_hidden,
        sort: cfg_file.sort,
        theme: Theme::detect(),
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
