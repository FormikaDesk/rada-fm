# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-10-09

### Added
- **Sidebar.** Your standard places under the names the system uses (read from `user-dirs.dirs` on Linux: *Scaricati* and *Documenti* on an Italian desktop), bookmarks you can add and remove from the bar (`B`, `Del`, context menu), and disks with a small usage bar. The place you are in is highlighted. `Tab` moves the keyboard in (focus is visible), `Ctrl+B` shows or hides it and the choice is remembered (`ui.json` in the state folder; `sidebar = false` in `config.toml` sets the default). 26 columns wide from a terminal width of 124; a column of icons with the places only from 100, showing the name of the item under the keyboard or the mouse in the bottom bar; hidden below that, so it always gives way before the preview.
- **Top bar.** Back, forward and parent buttons (faint when unusable), the path with a house for home, clickable segments and a clickable `…` that lists the folders a long path hides, a labelled `Filter…` field, a `Go to…` button and the item count. On a narrow terminal the count goes first, then the path folds; the words *Filter* and *Go to* stay.
- **Bottom bar.** Selection and clipboard on the left, the keys that make sense right now drawn as little clickable keys in the middle (for a selection, the filter, a window; extras when there is room; `hints = false` hides them), the free space of the current disk with a bar on the right.
- **Navigation history.** Back and forward through the folders visited (`Alt+←`, `Alt+→`, the buttons), `Alt+↑` for the parent folder in every key preset; going back puts the cursor on the folder you came out of.
- **Light themes.** `rada-light`, `catppuccin-latte` and `tokyo-night-day`, with automatic checks of the contrast of every token (7:1 for the main text, 4.5:1 for secondary text, accents, file types and statuses); the dark ones can be named explicitly (`rada-dark`, `catppuccin-mocha`, `tokyo-night-dark`).
- **Automatic theme.** `theme = "auto"` (the default) picks the variant that matches the terminal's background: from `COLORFGBG` and known terminals, then by asking the terminal for its background colour (OSC 11) with a short limit, before the keyboard reader starts and without ever taking a key (keys typed meanwhile are replayed). It does not ask under tmux without passthrough, under GNU screen, on dumb terminals or when input is not a terminal. `appearance = "light" | "dark"` (`--appearance`, `RADA_APPEARANCE`) forces only the brightness; naming a variant pins it.
- A stronger **cursor row** (new `cursor` colour token), with the cursor bar and the mark dot side by side, so the cursor stands out from marked rows in every theme.
- The platform layer reports the user's standard folders (`Platform::user_dirs`); Windows (Known Folders) and macOS use the conventional names until their phase.
- Configuration: `appearance`, `sidebar`, `hints`; command line: `--appearance`.

### Changed
- The theme names `rada`, `catppuccin` and `tokyo-night` now follow the terminal's background (use `appearance = "dark"` or a `-dark` / `-mocha` name to keep the dark one on a light terminal).
- The top bar no longer shows the cursor position ("3/12 items"), only how many items the folder has.
- Image facts (format, pixels, weight, date) sit directly under the picture and wrap instead of being cut; the word *Folder* in the Type column is dimmed and files of an unrecognised type get the same grey dot and *file* label as the others.
- The help lists the new actions, with `Alt+←/→`, `Tab` and `Ctrl+B` in both columns.

### Fixed
- Image previews: under load the decoded picture could arrive before the header-only answer, which then undid it.

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

[Unreleased]: https://github.com/formikadesk/rada-fm/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/formikadesk/rada-fm/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/formikadesk/rada-fm/releases/tag/v0.1.0
