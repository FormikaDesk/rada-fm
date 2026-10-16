//! `rada setup desktop`: the right integration for the system rada runs on.

use anyhow::{Context, Result, bail};
use rada_core::i18n::Key;
use rada_core::platform::Dirs;

pub struct DesktopOptions {
    pub remove: bool,
    pub default_file_manager: bool,
    /// Also put a launcher on the Desktop (macOS, Windows).
    pub desktop: bool,
    /// Windows: write the registry without asking.
    pub yes: bool,
}

pub fn desktop(
    dirs: &Dirs,
    o: &DesktopOptions,
    input: &mut dyn std::io::BufRead,
) -> Result<Vec<String>> {
    if o.remove && o.default_file_manager {
        bail!("{}", Key::MRemoveAndDefault.tr());
    }
    let exe = std::env::current_exe().context(Key::MNoExe.tr())?;
    if cfg!(target_os = "macos") {
        if o.default_file_manager {
            bail!("{}", Key::MDefaultFmMac.tr());
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
        // Windows' own folders for a person's application data, not the XDG variables a
        // developer may have set: that is where Windows Terminal and the Start menu look.
        let known = |var: &str, fallback: &str| -> std::path::PathBuf {
            std::env::var_os(var)
                .map(std::path::PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| dirs.home.join(fallback))
        };
        let local = known("LOCALAPPDATA", "AppData/Local");
        let roaming = known("APPDATA", "AppData/Roaming");
        let platform = rada_core::platform::current(dirs.clone());
        let desktop_dir = platform
            .user_dirs()
            .desktop
            .unwrap_or_else(|| dirs.home.join("Desktop"));
        let exe_for_registry = exe.clone();
        let setup = crate::winsetup::WinSetup {
            start_menu: roaming.join("Microsoft/Windows/Start Menu/Programs"),
            desktop_dir,
            windows_terminal: crate::winsetup::find_windows_terminal(&local),
            local_app_data: local,
            exe,
            state_dir: dirs.rada_state(),
            font_installed: crate::fontsetup::FontSetup::for_this_system(dirs).is_installed(),
        };
        let state = dirs.rada_state();
        let hive = crate::winfolder::RegExe {
            scratch: state.clone(),
        };
        return if o.remove {
            // What `--default-file-manager` wrote, if it did, is put back with the rest.
            let mut lines = crate::winfolder::undo(&hive, &state)?;
            lines.extend(setup.remove()?);
            Ok(lines)
        } else {
            let lines = setup.install(o.desktop)?;
            if o.default_file_manager {
                // What was made is told before the question about the registry.
                for l in &lines {
                    println!("{l}");
                }
                println!();
                crate::winfolder::install(&hive, &exe_for_registry, &state, o.yes, input)
            } else {
                Ok(lines)
            }
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
        lines.push(Key::SayDesktopFlag.tr().to_string());
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
