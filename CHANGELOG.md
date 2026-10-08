# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-08

First release.

### Added
- **Plan first.** Copy, move, rename, bulk rename (patterns such as `{name}_{n:3}.{ext}` or `s/find/replace/`), new folder, trash and permanent delete all show a plan before anything is touched: totals, warnings about symlinks, permissions, hard links and name conflicts, a per-item summary, and a choice of how name clashes are settled (skip, keep both, overwrite — the old file goes to the trash first). Permanent deletion asks for a typed `yes`.
- **Undo always, and redo.** A persistent journal records every operation; undo works after a restart or a crash, never overwrites a file changed since, and has its own plan. Redo plans the undone operation again. A history window lists the journal.
- **Robust operations.** Progress counted in bytes with throughput and time left; per-file errors with skip, skip all, retry or abort, always with the full path; symlinks stay symlinks (broken and circular ones included); moves across filesystems are copy, verify, then remove; half-written files never appear under their final name; trash follows the freedesktop.org specification including per-device trash folders.
- **Operations as data.** Every operation can be described as a JSON request (`rada --request FILE`, schema from `rada --schema request`) and planned by the engine without any interface; the plan has its own schema. Plans from requests go through the same confirmation window and journal. Both schemas are committed under `schema/`.
- **Interface.** A calm list with size bars, coloured type dots and relative dates; breadcrumb header; hint bar; floating windows over a dimmed background; notifications such as "Copied 342 files — press u to undo"; jump palette (folders, recents, bookmarks, disks); folder filter; layout that adapts to the terminal width; correct alignment for CJK, emoji and very long names.
- **Previews.** Text with line numbers and encoding detection; images (PNG, JPEG with EXIF rotation, GIF, WebP, BMP, SVG) drawn with Kitty graphics, iTerm2 images, Sixel or half blocks, decoded in workers with size limits; binary files get a summary card (ELF, PE and Mach-O architecture) with a hex dump on demand.
- **Keyboard and mouse.** Vim keys and desktop shortcuts active together (presets `vim+classic`, `vim`, `classic`), every action rebindable in `[keys]`; `Ctrl+C` copies and never quits. Mouse: click, double click, wheel, `Ctrl`/`Shift` selection, breadcrumb, column titles, buttons and a right-click menu showing each shortcut; `mouse = false` turns it off. Key delivery was checked in Ghostty and the conflicts are documented in the README.
- **Themes.** `rada` (default), `catppuccin` and `tokyo-night`, all colours from named tokens, with 256-colour and 16-colour fallbacks.
- **Desktop integration (Linux).** `rada setup desktop` installs a launcher entry and icons for the current user; `--default-file-manager` also sets the folder handler (the previous one is restored by `--remove`); `rada --spawn-terminal [PATH]` opens rada in a new terminal window (`xdg-terminal-exec`, `$TERMINAL`, then the first of ghostty, kitty, foot, alacritty, wezterm, konsole, gnome-terminal, xterm). An original, provisional icon is included.
- Shell helper to follow rada's last folder (`rada --init bash|zsh|fish|nushell|powershell`).
- Configuration file (`$XDG_CONFIG_HOME/rada/config.toml`): icons, theme, sorting, hidden files, bookmarks, image limits, mouse, key bindings.

### Platforms
- Linux is supported. Windows and macOS are in development: the code compiles for them, but they are not supported yet.

[Unreleased]: https://github.com/formikadesk/rada-fm/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/formikadesk/rada-fm/releases/tag/v0.1.0
