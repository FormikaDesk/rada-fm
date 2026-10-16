# Using rada

## First steps

```sh
rada                 # open the current folder
rada ~/Pictures      # open a folder
rada photo.png       # open its folder with the cursor on the file
```

Move with `j` `k` or the arrow keys, open with `Enter`, go up with `Backspace` (`Alt+↑` anywhere). Press `?` (or `F1`) for the full list of keys, `Ctrl+P` to jump to any folder, bookmark or disk. `Alt+←` and `Alt+→` go back and forward like in a browser; `Tab` moves into the sidebar of places. Select with `Space`, then copy (`Ctrl+C`), move to another folder and paste (`Ctrl+V`): a **plan** appears first, and nothing happens until you press `Enter`. Changed your mind afterwards? `Ctrl+Z` undoes it.

## Command line

```sh
rada [PATH] [--icons nerd|unicode|none] [--theme NAME] [--appearance auto|light|dark] [--keymap PRESET] [--no-mouse] [--hidden]
```

Icons default to plain Unicode markers. With a [Nerd Font](https://www.nerdfonts.com/) installed, use `--icons nerd` (or `icons = "nerd"` in the config file).

Open a file instead of a folder (`rada photo.png`) and rada starts in its folder with the cursor on it.

### As a file chooser

`rada --chooser-file ANSWER [FOLDER]` makes rada the window that another program uses to ask for a file: it is the same explorer, with a bar at the top that says what is asked (*Choose a file*, *Choose files*, *Choose a folder*, *Save as*). `Enter` chooses (the paths are written to `ANSWER`, one per line, and rada quits); `Esc` cancels and writes nothing. Nothing can be deleted, moved, renamed, copied or changed in this mode; the only thing that can be made is a new folder, where a folder or a place to save is asked for.

| Option | Asks for |
|---|---|
| `--chooser-file ANSWER` | one existing file |
| `--chooser-file ANSWER --chooser-multiple` | one or more files (`Space` marks, `Enter` chooses the marked) |
| `--chooser-file ANSWER --chooser-dir [--chooser-multiple]` | a folder (only folders are listed; `Enter` chooses the one under the cursor, `Alt+Enter` the one you are in) |
| `--chooser-file ANSWER --chooser-save=NAME FOLDER` | a place to save: `Ctrl+S` or `Alt+Enter` asks for the name (`NAME` is filled in), and a name that is already taken asks twice |

[Using rada as the desktop's file picker](desktop.md#use-rada-as-the-file-picker) is what this is for.

### Images

`--images auto` (the default) picks the protocol like this: under tmux, half blocks (graphics queries are only answered with `allow-passthrough`, and waiting for them would be slow); in a terminal that identifies itself (Ghostty, kitty, WezTerm, iTerm2, foot), the matching protocol with no query at all; otherwise the terminal is asked, with a bounded wait. Force a protocol with `--images kitty|sixel|iterm2|halfblocks`, or switch image drawing off with `--images off` (then nothing is decoded). Under tmux with passthrough enabled, `--images kitty` works too.

Large images: anything above 50 megapixels or 128 MiB is not decoded (a 12000×12000 PNG is refused instantly, from its header). Both limits are configurable.

## Keys

Both schemes are active together by default. `keymap = "vim"` or `"classic"` keeps only one. The help (`?` or `F1`) lists every action with its keys in both schemes side by side, and always reflects your own bindings.

| Action | Vim | Classic |
|---|---|---|
| move | `j` `k` | `↓` `↑` |
| open · parent folder | `l` · `h` (and `Backspace`) | `Enter` · `←` (also `→`, and `Alt+↑` in every preset; `Backspace` is *back* here) |
| top · bottom · page | `g` `G` · `Ctrl+U` `Ctrl+D` | `Home` `End` · `PgUp` `PgDn` |
| mark and move down | `Space` | `Space`, `Ctrl+Space`, `Ins` |
| select all · extend selection | | `Ctrl+A` · `Shift+↑` `Shift+↓` (`Ctrl+Shift+Home/End` to the ends) |
| clear selection, cancel the running operation, clear filter | `Esc` | `Esc` |
| copy · cut · paste (a plan comes first) | `y` `x` `p` | `Ctrl+C` `Ctrl+X` `Ctrl+V` |
| move to trash · delete permanently (type `yes`) | `d` `D` | `Del` · `Shift+Del` |
| rename · bulk rename · new folder · new file | `r` · `R` · `n` · `N` | `F2` · — · `Ctrl+Shift+N` (also `Ctrl+N`, `F7`) · `Alt+N` |
| extract here · extract into a folder · compress | `e` · `E` · `z` | `Ctrl+E` · `Alt+E` · `Alt+Z` |
| undo · redo | `u` · `Ctrl+R` | `Ctrl+Z` · `Ctrl+Y` (or `Ctrl+Shift+Z`) |
| back · forward in the folder history | `Alt+←` · `Alt+→`, or `Ctrl+O` · `Ctrl+G` (both schemes) | |
| history of operations | `U` | `F3` |
| filter this folder (in the bar) | `/` | `Ctrl+F` |
| the bar: commands, files, folders, recents, pinned, disks | `m` | `Ctrl+P` |
| edit the address · refresh | `Ctrl+L` `Alt+D` `F4` · `F5` (both schemes) | |
| properties · open with… · copy path · terminal here | `i` · `O` · `Y` · `t` | `Alt+Enter` · `Alt+O` · `Ctrl+Shift+C` · `Ctrl+Alt+T` |
| tabs: new · close · next · previous · jump | `Ctrl+T` `Ctrl+W` `Ctrl+Tab` `Ctrl+Shift+Tab` `Alt+1…9` (both schemes) | |
| details pane · Details / Icons / Columns · context menu · Tag column | `Alt+P` or `F6` · `Ctrl+1` `Ctrl+2` `Ctrl+3` or `v` · `Shift+F10` or `Menu` · `F8` | |
| sort key · reverse · hidden files | `s` `S` `.` | |
| hex dump of a binary · scroll preview | `H` · `J` `K` | |
| pin this folder (or the item in the navigation pane) · home | `B` · `~` | |
| move between list and sidebar · show / hide the sidebar | `Tab` · `Ctrl+B` (both schemes) | |
| update rada · Quick Look (macOS) · show package contents (macOS) · show in folder (a list of tagged or recent items) | `Alt+U` · `Alt+Y` · `Alt+↓` · `Alt+F` (both schemes) | |
| help · quit | `?` · `q` | `F1` · `Ctrl+Q` |

**`Ctrl+C` copies and never quits**: the terminal is in raw mode, so it arrives as an ordinary key, and a stray `SIGINT` is ignored. Quit with `q` or `Ctrl+Q`; `SIGTERM` and closing the terminal end rada in an orderly way (the running operation is cancelled, the terminal is restored).

**Redo** (`Ctrl+Y`) plans the operation you just undid again, and shows the usual plan window: it is the same request run through the same engine and recorded in the same journal. Redo is available until another operation is run; permanent deletions are never redoable.

In the plan window: `Enter` runs, `c` changes how name conflicts are resolved (skip existing / keep both / overwrite, where the old file goes to the trash so undo can restore it), `Tab` switches between the summary and every step, arrows scroll, `Esc` cancels. Paths are shown relative to the source and destination folders, which are named once at the top.

### Mouse

Click a name to put the cursor there; the box on its left (or `Space`) selects; double-click opens; the wheel scrolls (a turn is three rows, or one row of tiles); `Ctrl+click` adds one item and `Shift+click` selects a range. Everything drawn as a button or a field is clickable: the tabs (middle click closes), **← → ↑ ⟳**, the path segments, the bar, every button of the command bar, a column title to sort, an item of the navigation pane (right-click for *Open* and *Pin / Unpin*), the buttons of the details pane, the Details / Icons switch, every key of the hints, a command or place offered by the bar, and the buttons of any window. `mouse = false` (or `--no-mouse`) turns it all off. While rada owns the mouse, **hold `Shift` and drag to select text** in the terminal as usual. The mouse's own back and forward buttons are not reported by the terminal library (use `Alt+←` / `Alt+→`). Drag and drop is planned for a later release.

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
| `Alt+←`, `Alt+→`, `Alt+↑`, `Tab`, `Shift+Tab`, `Ctrl+B` | passed on, intact | back, forward, parent, switch between list and sidebar (both Tab keys), show / hide the sidebar |

Everything else rada uses (`Ctrl+C/X/V/Z/Y/A/F/L/P/N/Q/B`, `Shift+Del`, `F1`–`F3`, `F7`, `Alt+←/→/↑`, `Tab`, `Ctrl+Shift+Z`, …) arrived intact. The window manager's own bindings (Hyprland here) all use the Super key, except `Alt+Space`, `Ctrl+Alt+Del` and `Ctrl+Shift+R`, which rada does not use. Terminals that implement the kitty keyboard protocol can tell more combinations apart; rada works with what every terminal sends.

## Requests as data

Every operation can be described as data and handed to the engine, with no interface involved. The request is a JSON document (`schema/request.schema.json`, generated from the Rust types and kept current by a test); the answer is a plan (`schema/plan.schema.json`). A plan produced this way goes through exactly the same confirmation window, the same executor and the same undo journal as one built with the keyboard.

```json
{ "op": "copy", "sources": ["/home/me/photos/2024"], "destination": "/mnt/backup", "conflict": "keep_both" }
```

```sh
rada --schema request            # print the JSON Schema (also: plan)
rada --request job.json          # plan it and open the usual confirmation window (`-` reads stdin)
```

Operations: `copy`, `move`, `rename`, `bulk_rename`, `make_dir`, `make_file`, `trash`, `delete`, `extract`, `compress`, `set_tags`, `undo`. Paths must be absolute; unknown fields are refused rather than ignored. In Rust: `Engine::plan_request` (and `Jobs::plan_request` for the asynchronous version) turns an `OpRequest` into a `Plan` without touching anything; `plan_to_json` serialises it. Non-UTF-8 file names survive the round trip. Nothing runs without a confirmation: a deletion request still asks you to type `yes`.
