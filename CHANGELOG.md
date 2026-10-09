# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.4.0] - 2026-10-09

### Added
- **Archives.** `Enter` on an archive opens it as a read-only folder (the path reads `photos.zip › 2024`); the format is recognised from the content, not the name. Read and extract: zip, tar, `.tar.gz`/`.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`, single `.gz` `.bz2` `.xz` `.zst`, and 7z, all with pure-Rust libraries so they work the same on every platform; RAR through `7z`, `7zz` or `unrar` when installed, with a clear message and the install command when not.
- **Archive previews.** An archive that is not opened shows its format, number of files and folders, packed and unpacked size and its first items (counting stops after about 1.5 s on a huge stream and says "at least"); text and pictures inside an archive are previewed, taken out into the preview cache with size limits; the preview is abandoned as soon as you move on.
- **Extract.** *Extract here* (`e`, `Ctrl+E`) and *Extract to a folder…* (`E`, `Alt+E`), also from the right-click menu and the bottom bar. The folder is named from the archive's file name only; an archive with a single folder at its top gets no second one. Copying members out (`Ctrl+C`, `Ctrl+V`) is a partial extraction. Always with a plan (files, folders, links, size, free space, conflicts with skip / keep both / overwrite-through-the-trash), byte progress, cancel, journal and undo, and the same crash recovery as a copy.
- **Compress.** *Compress…* (`z`, `Alt+Z`) on the selection: name and format (zip, `.tar.gz`, `.tar.zst`, `.tar.xz`; `Tab` changes the format), a plan with an estimated size, progress in bytes, cancel (removes the half archive) and undo. Symlinks are kept as links; the plan warns that some programs do not understand them in a zip.
- **Safety.** Members with `..`, absolute or drive paths are not extracted and the plan says why; nothing is written through a link that the archive made; links are created but never followed, with a warning when they lead out; setuid/setgid bits are dropped; extractions above a size or compression-ratio limit (`[archives]`, 8 GiB and 200×) need a typed `yes`; a member can never write more than the archive declared for it; password-protected zip and 7z are recognised and said so; truncated or damaged archives are reported, never silently half-extracted; old code-page-437 zip names are decoded; archives refuse every write.
- New `[archives]` section in `config.toml`; requests `extract` and `compress` (and copy from an archive) in the JSON schema.
- Tests: every format and every safety rule, a fault-injected full disk, an archive changed or cut after the plan, `SIGKILL` in the middle of an extraction, random trees compressed and extracted in all four formats (contents, permissions, symlinks, times), archives with 60,000 members and a 96 MiB member, and unchanged navigation latency with a huge archive under the cursor.

### Changed
- The minimum Rust version is now 1.92 (the pure-Rust zstd encoder needs it).
- `FsEngine` gained `create_file`. The plan JSON gained the steps `extract_file`, `extract_symlink`, `compress` and `estimated_bytes`.

### Fixed
- Found by the new tests: a cancelled read was retried by the copy loops and silently dropped data; tar link names over 100 bytes were refused; an empty folder inside an archive could not be opened.

## [0.3.0] - 2026-10-09

### Added
- **Faithful copies** (Linux): permissions with setuid/setgid/sticky, times, extended attributes, POSIX ACLs, owner and group where the system allows, hard links between the files copied together, and the holes of sparse files (progress and totals count real bytes). The plan warns before you confirm about what the destination cannot hold; the result window lists what a copy could not keep. Undo of a copy or a move restores hard links and holes too. Windows and macOS have documented stubs (`Platform::fidelity`).
- **Survives a killed process.** Journal records reach the operating system as they are written; a *pending* record precedes every step that creates something. On the next start rada removes the temporary file of the step in doubt, adopts a copy that had finished but was not recorded, keeps both sides of a move cut between copy and removal, marks the operation *interrupted* and tells you. An undo cut short resumes where it stopped; a half-written last journal line is ignored.
- **PDF preview**: the first page (poppler's `pdftoppm`) drawn like an image, with page count, title and author (`pdfinfo`). A time limit, a size limit (`pdf_timeout_seconds`, `pdf_max_file_mb`), stopped as soon as you move on; cached in `$XDG_CACHE_HOME/rada/previews` and pruned by age and size. Password-protected and damaged files say so; without `pdftoppm` the first page's text is shown with a hint to install poppler.
- **Disk list**: the sidebar shows only real disks, partitions, removable drives and network shares — no `tmpfs`, `proc`, `binderfs`, `overlay`, `squashfs`, `fuse.portal`, `efivarfs`, bind-mount duplicates, `/boot` or `/efi`. Configurable with `[devices] hide`. The root disk is called *System*.
- `docs/SAFETY.md` (what is guaranteed and what is not), `CONTRIBUTING.md`, an honest *Status* section and a reorganised roadmap.
- Tests: fault injection (full disk in the middle of a file, revoked rights, source changed or deleted mid-copy, destination vanishing), `SIGKILL` of a real child process at six points, and property tests over random trees (symlinks, hard links, sparse files, odd names, read-only folders) for copy → undo and move → undo, also across filesystems.

### Changed
- The plan's warning about hard links now says only that links to files *outside the selection* become independent copies.
- A move of a read-only folder undoes into a read-only folder.

### Fixed
- Undoing a copy of a read-only folder failed to delete what the copy had put in it.
- Undoing a move across filesystems could leave folders behind when it brought back hard-linked files.
- A move whose source vanished during the copy no longer fails after the copy is made: the copy is kept.

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
