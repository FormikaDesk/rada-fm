# rada

**A safe harbor for your files.**

rada is a fast file manager for the terminal, written in Rust, driven by keyboard or mouse. It is built around three promises:

1. **Plan first.** Every operation on files shows a *plan* before anything is touched: what will be done, how many files and bytes, and warnings about symlinks, permissions and name conflicts. You confirm, or you don't.
2. **Undo always.** Every operation that finishes is recorded in a persistent journal and can be undone, even after a restart or a crash. If a file was modified after the operation, undo leaves it alone and tells you, instead of overwriting it.
3. **Robust by construction.** Progress is counted in bytes. A failure on one file never stops the rest: you choose to skip, retry or abort, and the message always carries the full path. Symlinks stay symlinks (even broken or circular ones), nothing follows a link out of the tree, and a half-written file never appears under its final name.

![rada: the folder list with an image preview](docs/screenshots/main.png)

> **Status:** version 0.1.0, the first release. Linux is supported. Windows and macOS are **in development**: the code is structured for them and compiles, but they are not supported yet.

## Why it is different

| | |
|---|---|
| **A calm interface** | No boxes around the list: a breadcrumb on top, a size bar and a coloured type dot per file, relative dates ("3 h ago"), one line of hints at the bottom. Windows (plan, errors, prompts, jump palette) float in the middle over a dimmed background; notifications come and go by themselves ("Copied 342 files — press u to undo"). It adapts to the width of the terminal: the preview hides first, then the date, the type and the size bar. Names with CJK, emoji or very long text stay aligned and are cut with `…`. |
| **Keyboard *and* mouse** | Vim keys and the usual desktop shortcuts work together by default (`y` or `Ctrl+C`, `d` or `Del`, `u` or `Ctrl+Z`…). The mouse selects, opens, scrolls, sorts by column, and a right click opens a context menu that shows each shortcut. Every key can be rebound. |
| **Plan window** | Copy, move, rename, bulk rename, new folder, trash, delete: all show a plan with totals and warnings first. Permanent deletion states clearly that it cannot be undone and asks you to type `yes`. |
| **Journal + undo** | `u` undoes the last operation (with its own plan, so you see what undo will do). `U` shows the history. Undo of a copy removes exactly what the copy created; undo of a move moves back; undo of trash restores from the trash; undo of an overwrite brings the old file back. |
| **Per-file errors** | One unreadable file does not abort a 10,000-file copy. Errors are handled per step: skip / skip all / retry / abort. |
| **Byte progress** | A real progress bar with throughput and ETA, not "3 of 4 items". |
| **Safe across filesystems** | Moves between filesystems are copy → verify (checksum of what was written) → remove source, file by file; the source is never deleted for something that failed to copy. Trashing follows the freedesktop.org specification, including per-device trash directories. |
| **Live** | The folder view updates by itself (inotify via `notify`), with debouncing. Sorting is stable and deterministic and applies instantly. |
| **Nothing blocks the UI** | All I/O happens in worker threads; the interface thread only draws and handles keys. Volumes are listed by a worker, never on a keypress, with a timeout per mount so a hung network share cannot freeze anything. |
| **Real image previews** | PNG, JPEG (with EXIF rotation), GIF (first frame), WebP, BMP and SVG are drawn with the best protocol the terminal offers: Kitty graphics (Ghostty, kitty), iTerm2 images (WezTerm, iTerm2), Sixel (foot), or coloured half blocks everywhere else. Decoding and resizing run in worker threads, so browsing a folder of photos never delays a keypress. Below the picture: format, pixel size, weight and date. Images that are too big are refused from the header alone, with a clear message. |
| **Binary files get a card** | Instead of a wall of hex: file type, size, dates, permissions and, for executables, the architecture (ELF, PE and Mach-O are read from the header, never run). The hex dump is one key away (`H`). |
| **Hostile names are harmless** | Newlines, escape sequences, bidi controls and invalid UTF-8 in file names are shown as visible escapes. Paths are `Path`/`OsString` everywhere, never text. |

![The plan window: totals, warnings, how clashes are settled, what will happen to each item](docs/screenshots/plan.png)

![The jump palette](docs/screenshots/palette.png)

![The context menu opened with the right mouse button, with each action's shortcut](docs/screenshots/menu.png)

![The help: every action with its vim and classic keys side by side](docs/screenshots/help.png)

![Progress counted in bytes, with throughput and time left](docs/screenshots/progress.png)

## Install

From source. You need Linux and a Rust toolchain of version 1.90 or newer ([rustup](https://rustup.rs)).

```sh
git clone https://github.com/formikadesk/rada-fm rada
cd rada
cargo install --path crates/rada
```

Or just build it: `cargo build --release` and use `target/release/rada`.

A [Nerd Font](https://www.nerdfonts.com/) in your terminal is recommended: rada then shows file-type icons. It looks for the font in the configuration of Ghostty, kitty, WezTerm, Alacritty and foot; otherwise it uses plain Unicode markers (`--icons nerd|unicode|none` overrides).

Optional: make your shell follow rada's last folder (`v` instead of `rada`):

```sh
eval "$(rada --init bash)"        # also: zsh, fish, nushell, powershell
```

Optional: add rada to your application menu, see [Desktop integration](#desktop-integration).

## First steps

```sh
rada                 # open the current folder
rada ~/Pictures      # open a folder
rada photo.png       # open its folder with the cursor on the file
```

Move with `j` `k` or the arrow keys, open with `Enter`, go up with `Backspace`. Press `?` (or `F1`) for the full list of keys, `Ctrl+P` to jump to any folder, bookmark or disk. Select with `Space`, then copy (`Ctrl+C`), move to another folder and paste (`Ctrl+V`): a **plan** appears first, and nothing happens until you press `Enter`. Changed your mind afterwards? `Ctrl+Z` undoes it.

## Usage

```sh
rada [PATH] [--icons nerd|unicode|none] [--theme NAME] [--keymap PRESET] [--no-mouse] [--hidden]
```

Icons default to plain Unicode markers. With a [Nerd Font](https://www.nerdfonts.com/) installed, use `--icons nerd` (or `icons = "nerd"` in the config file).

Open a file instead of a folder (`rada photo.png`) and rada starts in its folder with the cursor on it.

### Images

`--images auto` (the default) picks the protocol like this: under tmux, half blocks (graphics queries are only answered with `allow-passthrough`, and waiting for them would be slow); in a terminal that identifies itself (Ghostty, kitty, WezTerm, iTerm2, foot), the matching protocol with no query at all; otherwise the terminal is asked, with a bounded wait. Force a protocol with `--images kitty|sixel|iterm2|halfblocks`, or switch image drawing off with `--images off` (then nothing is decoded). Under tmux with passthrough enabled, `--images kitty` works too.

Large images: anything above 50 megapixels or 128 MiB is not decoded (a 12000×12000 PNG is refused instantly, from its header). Both limits are configurable.

Optional config: `$XDG_CONFIG_HOME/rada/config.toml`

```toml
icons = "nerd"        # nerd | unicode | none
theme = "rada"        # rada | catppuccin | tokyo-night
show_hidden = false
sort = "name"         # name | size | date
reverse = false
bookmarks = ["~/projects", "/mnt/data"]
images = "auto"       # auto | halfblocks | kitty | sixel | iterm2 | off
image_max_megapixels = 50
image_max_file_mb = 128
mouse = true          # false: rada never captures the mouse
keymap = "vim+classic"   # vim+classic (default) | vim | classic

[keys]                # per action; replaces all of its keys, [] unbinds it
copy = ["y", "ctrl+c"]
trash = ["d", "delete"]
quit = "ctrl+q"
```

State lives in `$XDG_STATE_HOME/rada/` (`journal.jsonl`, `log/`). Set `RADA_LOG=debug` for more logging (written to a file, never to the screen).

## Themes

Every colour on screen comes from a small set of named tokens (text, dim text, accent, selection, one colour per file type, success/warning/error…). Nothing is coloured by hand elsewhere; a test reads the sources and fails if it finds a hand-written colour outside `theme.rs`, and another renders every theme and checks that each cell uses only that theme's tokens.

| `rada` (default) | `catppuccin` | `tokyo-night` |
|---|---|---|
| calm azure on a cool neutral base | Catppuccin Mocha | Tokyo Night |

![Catppuccin](docs/screenshots/theme-catppuccin.png)
![Tokyo Night](docs/screenshots/theme-tokyo-night.png)

Choose with `theme = "…"` in the config, `--theme NAME` or `RADA_THEME`. Colours are true colour when the terminal supports it (`COLORTERM=truecolor`), fall back to the 256-colour palette, and to the 16 ANSI colours (with reverse video for the selection) on a basic terminal. The terminal's own background is left alone, so transparency and your wallpaper keep working.

## Keys

Both schemes are active together by default. `keymap = "vim"` or `"classic"` keeps only one. The help (`?` or `F1`) lists every action with its keys in both schemes side by side, and always reflects your own bindings.

| Action | Vim | Classic |
|---|---|---|
| move | `j` `k` | `↓` `↑` |
| open · parent folder | `l` · `h` | `Enter` · `Backspace` (also `→` `←`) |
| top · bottom · page | `g` `G` · `Ctrl+U` `Ctrl+D` | `Home` `End` · `PgUp` `PgDn` |
| mark and move down | `Space` | `Space`, `Ctrl+Space`, `Ins` |
| select all · extend selection | | `Ctrl+A` · `Shift+↑` `Shift+↓` (`Ctrl+Shift+Home/End` to the ends) |
| clear selection, cancel the running operation, clear filter | `Esc` | `Esc` |
| copy · cut · paste (a plan comes first) | `y` `x` `p` | `Ctrl+C` `Ctrl+X` `Ctrl+V` |
| move to trash · delete permanently (type `yes`) | `d` `D` | `Del` · `Shift+Del` |
| rename · bulk rename · new folder | `r` · `R` · `n` | `F2` · — · `Ctrl+N` |
| undo · redo | `u` · `Ctrl+R` | `Ctrl+Z` · `Ctrl+Y` (or `Ctrl+Shift+Z`) |
| history of operations | `U` | `F3` |
| filter this folder | `/` | `Ctrl+F` |
| jump palette (folders, recents, bookmarks, disks) | `m` | `Ctrl+P` `Ctrl+L` |
| sort key · reverse · hidden files | `s` `S` `.` | |
| hex dump of a binary · scroll preview | `H` · `J` `K` | |
| bookmark this folder · home | `B` · `~` | |
| help · quit | `?` · `q` | `F1` · `Ctrl+Q` |

**`Ctrl+C` copies and never quits**: the terminal is in raw mode, so it arrives as an ordinary key, and a stray `SIGINT` is ignored. Quit with `q` or `Ctrl+Q`; `SIGTERM` and closing the terminal end rada in an orderly way (the running operation is cancelled, the terminal is restored).

**Redo** (`Ctrl+Y`) plans the operation you just undid again, and shows the usual plan window: it is the same request run through the same engine and recorded in the same journal. Redo is available until another operation is run; permanent deletions are never redoable.

In the plan window: `Enter` runs, `c` changes how name conflicts are resolved (skip existing / keep both / overwrite, where the old file goes to the trash so undo can restore it), `Tab` switches between the summary and every step, arrows scroll, `Esc` cancels. Paths are shown relative to the source and destination folders, which are named once at the top.

### Mouse

Click selects; double-click opens; the wheel scrolls the list (or the preview, when the pointer is over it); `Ctrl+click` adds one item and `Shift+click` selects a range; click a folder in the path to go there, a column title to sort (again to reverse), an item of the hint bar to run it, a row of the palette to jump, and the buttons of any window. A right click opens a context menu with each action's shortcut. `mouse = false` (or `--no-mouse`) turns it all off. While rada owns the mouse, **hold `Shift` and drag to select text** in the terminal as usual. Drag and drop between windows is planned for a later release.

### Keys that terminals take for themselves

Checked in Ghostty by sending each combination to a real window and reading what the program received (the probe is `cargo run --example keyprobe`):

| Combination | What happens | In rada |
|---|---|---|
| `Shift+Home`, `Shift+End` | Ghostty scrolls its own scrollback and the program never sees them | `Ctrl+Shift+Home/End` extend the selection to the ends instead |
| `Shift+PgUp/PgDn`, `Ctrl+Shift+V/A/F/P/N/T/W`, `Ctrl+T`, `Ctrl+Enter`, `Ctrl+,`, `Ctrl+±/0` | Ghostty's own tabs, search, paste, fullscreen, font size | not used |
| `Shift+↑/↓` | passed on (Ghostty only uses them to adjust a text selection that exists) | extend selection; clear a terminal selection first if it seems not to work |
| `Ctrl+H`, `Ctrl+I`, `Ctrl+M`, `Ctrl+[` | the same bytes as Backspace, Tab, Enter, Esc in most terminals | not used (`Ctrl+Backspace` arrives as `Ctrl+H`) |
| `Ctrl+/` | arrives as `Ctrl+7` | not used |
| `Ctrl+S`, `Ctrl+Q`, `Ctrl+Z` | would be flow control and job control in a normal terminal | delivered as keys (raw mode); `Ctrl+Q` quits, `Ctrl+Z` undoes |
| `Ctrl+Space` | passed on | extra mark key |

Everything else rada uses (`Ctrl+C/X/V/Z/Y/A/F/L/P/N/Q`, `Shift+Del`, `F1`–`F3`, `F7`, `Alt+↑`, `Ctrl+Shift+Z`, …) arrived intact. The window manager's own bindings (Hyprland here) all use the Super key, except `Alt+Space`, `Ctrl+Alt+Del` and `Ctrl+Shift+R`, which rada does not use. Terminals that implement the kitty keyboard protocol can tell more combinations apart; rada works with what every terminal sends.

## Requests as data

Every operation can be described as data and handed to the engine, with no interface involved. The request is a JSON document (`schema/request.schema.json`, generated from the Rust types and kept current by a test); the answer is a plan (`schema/plan.schema.json`). A plan produced this way goes through exactly the same confirmation window, the same executor and the same undo journal as one built with the keyboard.

```json
{ "op": "copy", "sources": ["/home/me/photos/2024"], "destination": "/mnt/backup", "conflict": "keep_both" }
```

```sh
rada --schema request            # print the JSON Schema (also: plan)
rada --request job.json          # plan it and open the usual confirmation window (`-` reads stdin)
```

Operations: `copy`, `move`, `rename`, `bulk_rename`, `make_dir`, `trash`, `delete`, `undo`. Paths must be absolute; unknown fields are refused rather than ignored. In Rust: `Engine::plan_request` (and `Jobs::plan_request` for the asynchronous version) turns an `OpRequest` into a `Plan` without touching anything; `plan_to_json` serialises it. Non-UTF-8 file names survive the round trip. Nothing runs without a confirmation: a deletion request still asks you to type `yes`.

## Desktop integration

Linux only. Everything is installed for your user, nothing needs root, and nothing is done unless you ask.

```sh
rada setup desktop                        # launcher entry and icons for this user
rada setup desktop --default-file-manager # also make rada the program that opens folders
rada setup desktop --remove               # take away exactly what was installed
```

`setup desktop` writes `rada.desktop` to `~/.local/share/applications` and the icon to `~/.local/share/icons/hicolor` (SVG, and PNG at 48, 128 and 256 px), updates the desktop database when `update-desktop-database` is available, and prints each file it wrote. With `--default-file-manager` it uses `xdg-mime` to set rada as the handler of `inode/directory`; the previous handler is remembered and `--remove` puts it back. The launcher entry runs `rada --spawn-terminal`, which opens rada in a new terminal window, choosing in this order `xdg-terminal-exec`, `$TERMINAL`, then the first one found of ghostty, kitty, foot, alacritty, wezterm, konsole, gnome-terminal, xterm. The files to install by hand (for packagers) are in `packaging/linux/`.

## How it is built

A Cargo workspace:

- `crates/core` — filesystem model, the plan/execute/undo engine, journal, watcher, workers, platform layer. No dependency on any interface.
- `crates/tui` — the interface (ratatui + crossterm).
- `crates/rada` — the `rada` binary.

Every operation is expressed as a list of atomic **steps** (`MakeDir`, `CopyFile`, `CopySymlink`, `Rename`, `TrashItem`, `RemoveFile`, …). A step knows its own inverse, so the same machinery plans, executes, journals and undoes. Everything that depends on the operating system sits behind one `Platform` trait (trash, volumes, file attributes, opening files, path rules); the engine never branches on the OS. The trait already models what Windows needs (drive letters, NTFS junctions and reparse points, hidden/system attributes, the Recycle Bin), with a complete Linux implementation and compilable stubs for Windows and macOS.

### Tests

```sh
cargo test --workspace
```

The interface is tested headlessly with real workers on a sandboxed filesystem: every action through both key schemes, simulated mouse events (click, double click, wheel, right click, modifiers), and committed text snapshots of the screens (`crates/tui/tests/snapshots/`, pinned clock and home folder; update with `INSTA_UPDATE=always cargo test -p rada-tui --test snapshots` and review the diff).

The suite includes regression tests for real bugs found in other terminal file managers: folders with dots in their names, symlinks (to files, to folders, broken, circular) inside copied trees, an unreadable file in a copied folder, moving and trashing across filesystems (tmpfs ↔ disk), Unicode/emoji/special/non-UTF-8 names, and undo of every kind of operation including "modified after the operation".

Tests never touch your real folders: each one runs in a temporary sandbox with `HOME` and all `XDG_*` variables pointing into it, and a guard fingerprints your real trash, config, state and cache before the first test and fails the test if anything under them changed.

CI (GitHub Actions) builds and runs the core tests on Linux, Windows and macOS. Tests that are not supported yet on Windows/macOS are marked `#[ignore = "<reason>"]`.

## Roadmap

Planned, roughly in this order of interest:

- **Storage insights**: find what takes up space and get cleanup suggestions, with an optional AI assistant that runs locally (e.g. Ollama), is off by default, and only sees file names and sizes — never file contents. Every suggestion goes through the usual plan and undo.

- Dual pane, tabs, layout restore
- Archives (browse and extract safely, all common formats)
- Git integration
- Animated GIF playback, image zoom, EXIF details
- Syntax-highlighted preview
- Recursive search, recursive sizes
- Drag and drop
- Trash browser (list, restore, empty)
- Compare and sync two folders with a dry-run plan
- Remote filesystems (SFTP) through the same engine
- Plugins
- Preserve extended attributes, ACLs, hard links and sparse files when copying
- Full Windows and macOS support (Recycle Bin, Trash, volumes, drive letters, junctions)
- Signed binaries and distribution packages

## Contributing

Bug reports and ideas are welcome as issues; the templates ask for what helps most (version, terminal, steps). If rada ever loses or damages a file, say so first: that is the one kind of bug that matters most, and the journal (`U`) may still be able to undo it.

For code: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` must pass. Changes to the look of the interface come with updated screen snapshots (`INSTA_UPDATE=always cargo test -p rada-tui --test snapshots`) whose diff you have reviewed. New operations must be expressed as plan steps that know their own inverse. Tests never touch your real folders and must keep it that way.

## License

MIT © Alessandro Formica. See [LICENSE](LICENSE).
