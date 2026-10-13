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
            bail!(
                "--default-file-manager is for Linux desktops: macOS has no setting for it that a program may change"
            );
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
    if cfg!(windows) {
        if o.default_file_manager {
            bail!(
                "--default-file-manager is for Linux desktops: Windows keeps Explorer as the folder handler"
            );
        }
        let local = dirs.data.clone();
        let platform = rada_core::platform::current(dirs.clone());
        let desktop_dir = platform
            .user_dirs()
            .desktop
            .unwrap_or_else(|| dirs.home.join("Desktop"));
        let setup = crate::winsetup::WinSetup {
            start_menu: dirs.config.join("Microsoft/Windows/Start Menu/Programs"),
            desktop_dir,
            windows_terminal: crate::winsetup::find_windows_terminal(&local),
            local_app_data: local,
            exe,
            state_dir: dirs.rada_state(),
            font_installed: crate::fontsetup::FontSetup::for_this_system(dirs).is_installed(),
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
        lines.push(
            "--desktop is for macOS and Windows: on Linux the entry is in the application menu"
                .into(),
        );
    }
    Ok(lines)
}

/// `rada setup font`: install (or with `remove`, take away) the Nerd Font.
pub fn font(dirs: &Dirs, remove: bool) -> Result<Vec<String>> {
    let setup = crate::fontsetup::FontSetup::for_this_system(dirs);
    let mut lines = Vec::new();
    if remove {
        setup.remove(&mut |l| lines.push(l))?;
    } else {
        setup.install(&mut |l| {
            println!("{l}");
        })?;
    }
    Ok(lines)
}
