//! `rada setup desktop`: the right integration for the system rada runs on.

use anyhow::{Context, Result, bail};
use rada_core::platform::Dirs;

pub struct DesktopOptions {
    pub remove: bool,
    pub default_file_manager: bool,
    /// Also put a launcher on the Desktop (macOS, Windows).
    pub desktop: bool,
}

pub fn desktop(dirs: &Dirs, o: &DesktopOptions) -> Result<Vec<String>> {
    if o.remove && o.default_file_manager {
        bail!("--remove and --default-file-manager cannot be used together");
    }
    let exe = std::env::current_exe().context("cannot find the rada executable")?;
    if cfg!(target_os = "macos") {
        if o.default_file_manager {
            bail!("--default-file-manager is for Linux desktops: macOS has no setting for it that a program may change");
        }
        let setup = crate::macapp::MacSetup {
            apps_dir: dirs.home.join("Applications"),
            desktop_dir: dirs.home.join("Desktop"),
            exe,
            state_dir: dirs.rada_state(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        return if o.remove {
            setup.remove()
        } else {
            setup.install(o.desktop)
        };
    }
    // Linux and the other Unix systems: a launcher entry in the application menu.
    let setup = crate::desktop::Setup::from_env(dirs)?;
    let mut lines = if o.remove {
        setup.remove()?
    } else {
        setup.install(o.default_file_manager)?
    };
    if o.desktop {
        lines.push("--desktop is for macOS and Windows: on Linux the entry is in the application menu".into());
    }
    Ok(lines)
}
