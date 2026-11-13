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

`rada --chooser-file ANSWER [FOLDER]` makes rada the window that another program uses to ask for a file: it is the same explorer, with a bar at the top that says what is asked (*Choose a file*, *Choose files*, *Choose a folder*, *Save as*). `Enter` chooses (the paths are written to `ANSWER`, one per line, and rada quits); `Esc` cancels and writes nothing. Nothing can be deleted, moved, renamed, copied or changed in this mode; the only thing that can be made is a new folder, where a folder or a place to save is asked for. The list starts sorted by date of modification, the most recent first, whatever `sort` says in the configuration; the usual keys change the order.

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
| the bar: commands, files, folders, recents, pinned, disks | `:` (opens it as the command line, see below) | `Ctrl+P` |
| edit the address · refresh | `Ctrl+L` `Alt+D` `F4` · `F5` (both schemes) | |
| properties · open with… · copy path · terminal here | `i` · `O` · `Y` · `t` | `Alt+Enter` · `Alt+O` · `Ctrl+Shift+C` · `Ctrl+Alt+T` |
| tabs: new · close · next · previous · jump | `Ctrl+T` `Ctrl+W` `Ctrl+Tab` `Ctrl+Shift+Tab` `Alt+1…9` (both schemes) | |
| details pane · Details / Icons / Columns · Tag column | `Alt+P` or `F6` · `Ctrl+1` `Ctrl+2` `Ctrl+3` or `v` · `F8` | |
| context menu (right click) | `m` (`Shift+F10` and `Menu` too) | `Shift+F10` · `Menu` |
| sort key · reverse · hidden files | `s` `S` `.` | |
| hex dump of a binary · scroll preview | `H` · `J` `K` | |
| pin this folder (or the item in the navigation pane) · home | `B` · `~` | |
| move between list and sidebar · show / hide the sidebar | `Tab` · `Ctrl+B` (both schemes) | |
| update rada · Quick Look (macOS) · show package contents (macOS) · show in folder (a list of tagged or recent items) | `Alt+U` · `Alt+Y` · `Alt+↓` · `Alt+F` (both schemes) | |
| help · quit | `?` · `q` | `F1` · `Ctrl+Q` |

**`Ctrl+C` copies and never quits**: the terminal is in raw mode, so it arrives as an ordinary key, and a stray `SIGINT` is ignored. Quit with `q` or `Ctrl+Q`; `SIGTERM` and closing the terminal end rada in an orderly way (the running operation is cancelled, the terminal is restored).

**Redo** (`Ctrl+Y`) plans the operation you just undid again, and shows the usual plan window: it is the same request run through the same engine and recorded in the same journal. Redo is available until another operation is run; permanent deletions are never redoable.

In the plan window: `Enter` runs, `c` changes how name conflicts are resolved (skip existing / keep both / overwrite, where the old file goes to the trash so undo can restore it), `Tab` switches between the summary and every step, arrows scroll, `Esc` cancels. Paths are shown relative to the source and destination folders, which are named once at the top.

Renaming to a name that already exists opens the same plan. *Skip* is the default and changes nothing; *keep both* uses the next free `name (1).ext`; *overwrite* sends the old item to the trash first, so `Ctrl+Z` brings everything back. Only a file can replace a file, and a folder an empty folder. Bulk rename still refuses name conflicts.

### Bulk rename

`R` with the vim keys (in the classic ones, *Rename many* in the menu or in the bar) renames what is selected with a **pattern**. Under the field, the new name of each item as you type, made in the background; the plan comes with `Enter`.

| in the pattern | becomes |
|---|---|
| `{name}`, `{name:lower}`, `{name:upper}` | the name without its extension |
| `{ext}` | the extension, without the dot |
| `{n}`, `{n:3}` | a counter, from 1, here padded to 3 digits |
| `{parent}` | the name of the folder the item is in |
| `{mtime}`, `{mtime:%Y-%m-%d}` | when the item was last changed |
| `{exif.date:%Y-%m-%d}`, `{exif.time:%H%M%S}` | when a photo was taken, as the camera's clock said |
| `{exif.camera}`, `{exif.place}` | the camera; the place written in the photo |
| `{audio.artist}`, `{audio.title}`, `{audio.album}`, `{audio.year}`, `{audio.track:02}` | the tags of a song |
| `{pdf.title}`, `{pdf.author}` | what a PDF says of itself |
| `{video.duration}` | the length of a video (`45s`, `4m05s`, `1h02m03s`), only when `ffprobe` is installed |
| `{{`, `}}` | a brace |
| `s/find/replace/` (the whole pattern) | the name with every *find* replaced |

A date takes `%Y` `%y` `%m` `%d` `%H` `%M` `%S` `%j` and `%%` (all digits, which every system accepts in a name); without a format a date is `2024-05-01` and a time `143005`. A number takes the width it is padded to. A text from the file has what no system accepts in a name (`/ \ : * ? " < > |`) put as `_`: an artist *AC/DC* gives `AC_DC`, never a folder.

After `|` comes what to use when the file does not say: `{exif.date|mtime:%Y-%m-%d}` is the day the photo was taken, or the day the file was last changed. The fields after `|` must be of the same kind (dates with dates, texts with texts).

When the pattern needs what files say about themselves, they are read first, with the same limits as the details pane, and the window says *Reading metadata… 12/40*; `Esc` closes it and stops the reading. An item that does not have the field, and has nothing after `|` that it has, is marked with the reason (*✖ no value for {exif.date}*), and `Enter` starts no plan while there is one: change the pattern, or press `Ctrl+X` to take the marked items out of the rename. The plan checks everything again, as for any rename: names the system accepts, two items that would get the same name, a name already taken, a change of case only, and each item compared at the last moment with what the plan saw.

**`{exif.place}` is often empty.** It is the place the photo names (XMP or IPTC, written by some cameras and by photo programs); a phone usually writes only coordinates, and rada does not turn coordinates into a place. Give it a fallback (`{exif.place|exif.camera}`) or take those items out.

The **history of operations** (`F3`, `U` with the vim keys) lists every operation of the journal, newest first; `Enter` undoes the one chosen. Beside each one that can still be undone there is a mark, worked out in the background when the window opens (the rows in view first):

- **● can undo**: every step was recorded, and what it left is as it left it;
- **◐ to check (N files changed)**: some of what it left changed or is gone since; the undo leaves those alone and says which. Past 2000 items nothing is looked at (*to check: too many files*);
- **○ not guaranteed**: it ran another program (rada can remove the file it wrote, if you have not changed it; it does not see anything else the program did), it worked on a phone or a camera, or some of its steps ran without their way back being written.

Under the row chosen a sentence says what its mark means for it. The mark is an opinion from one look at each item (no contents are read); the plan of the undo checks every item again and decides. Marks are kept while the folders they looked into do not change, and forgotten when an operation ends.

### Mouse

Click a name to put the cursor there; the box on its left (or `Space`) selects; double-click opens; the wheel scrolls (a turn is three rows, or one row of tiles); `Ctrl+click` adds one item and `Shift+click` selects a range. Everything drawn as a button or a field is clickable: the tabs (middle click closes), **← → ↑ ⟳**, the path segments, the bar, every button of the command bar, a column title to sort, an item of the navigation pane (right-click for *Open* and *Pin / Unpin*), the buttons of the details pane, the Details / Icons switch, every key of the hints, a command or place offered by the bar, and the buttons of any window. What the pointer is over is lit with a quiet tone (the theme's *row under the mouse*): a row of the list, a button, a tab, a path segment, a menu entry. The row the cursor is on, a selected row and the tab in front are left as they are; where a field or the main button already has a fill the text is underlined instead, and with 16 colours everything is. `mouse = false` (or `--no-mouse`) turns it all off. While rada owns the mouse, **hold `Shift` and drag to select text** in the terminal as usual. The mouse's own back and forward buttons are not reported by the terminal library (use `Alt+←` / `Alt+→`). Drag and drop is planned for a later release.

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

### Colours in the preview

The preview of a source file, a script or a markup file shows comments, strings, numbers, keywords, types and functions in colours. They are not their own: each kind takes a colour of the theme in use (dark, Gesso, Carta and the others), so they change with it. Only the first 64 KB and 1000 lines are coloured, and the work is done in the background, so a big file does not slow the interface; a file of a language that is not known, and a binary one, are shown as before. `syntax_highlight = false` in `config.toml` turns it off.

### Jumping with zoxide

If [zoxide](https://github.com/ajeetdsouza/zoxide) is installed, the bar (`Ctrl+P`) uses it. When what you type is no command, no place and no name in the folder you are in, the bar offers the folders `zoxide query --list` gives for it (at most five, marked *zoxide*): pick one with `↓` or `Tab` and press `Enter` (or click it), or press `Enter` at once for the first. Besides that, every folder rada enters is told to zoxide (`zoxide add`, in the background), so that the two share what you visit. If zoxide is not installed nothing happens and no error is shown; `zoxide = false` in `config.toml` makes rada never call it (see [configuration](configuration.md)).

### Commands typed after `:`

With the vim keys, `:` opens the bar with a colon already written: the **command line**. A command is a name followed by its arguments, separated by spaces; a name with spaces goes between quotes (`"my notes"`) or with a backslash before each space (`my\ notes`), `~` is your home folder, an option is written `--reverse`, and an argument can also be given by its name (`by=size`). Every action of the keys has a command (its name is the one used in `[keys]` of `config.toml`, such as `copy`, `new_tab`, `toggle_hidden`); without arguments it does what its key does. A few take arguments:

| command | what it does |
|---|---|
| `:mkdir name` | a new folder here (also `:new_folder`) |
| `:rename pattern` | renames what is selected, or the item under the cursor, with a pattern of the [bulk rename](#bulk-rename) (`{name}_{n:3}.{ext}`, `{exif.date\|mtime:%Y-%m-%d}_{n:2}.{ext}`) |
| `:sort name\|size\|date\|type [--reverse]` | the order of the list |
| `:cd folder` | goes to a folder; relative to the one shown, `~` is home |
| `:find text` | searches by name in this folder and the ones below, as `Ctrl+Enter` does in the bar; a text between slashes is a regular expression (`:find /^img_\d+/`), which the bar alone would take for a path |
| `:undo`, `:redo`, `:history`, `:help` | as `Ctrl+Z`, `Ctrl+Y`, `F3` and `?` |

A command that changes files never does it at once: it opens the **plan** window like its key, and `Ctrl+Z` undoes it. When a line cannot be read, or the command cannot be used where you are (`:mkdir` in an archive or in a search), the bar stays open and says why under it, with the column where the problem is (*column 7: colour is not one of: name, size, date, type*). A line whose first word is not a command is read as words, as in the bar: the commands and places that match are offered, and `Enter` runs the first. `Tab` completes the name of a command, the value of an argument that has a list of them, an option, or a path (folders and files); pressing it again goes to the next. `↑` and `↓` go through the lines run before, which are kept, each once, for the next start (in `ui.json` in the state folder). The classic keys have no `:` (`Ctrl+P` opens the bar), and on an Italian keyboard `:` is `Shift+.`: the full stop alone still shows and hides hidden files.

## Requests as data

Every operation can be described as data and handed to the engine, with no interface involved. The request is a JSON document (`schema/request.schema.json`, generated from the Rust types and kept current by a test); the answer is a plan (`schema/plan.schema.json`). A plan produced this way goes through exactly the same confirmation window, the same executor and the same undo journal as one built with the keyboard.

```json
{ "op": "copy", "sources": ["/home/me/photos/2024"], "destination": "/mnt/backup", "conflict": "keep_both" }
```

```sh
rada --schema request            # print the JSON Schema (also: plan)
rada --request job.json          # plan it and open the usual confirmation window (`-` reads stdin)
```

Operations: `copy`, `move`, `rename`, `bulk_rename`, `make_dir`, `make_file`, `trash`, `delete`, `extract`, `compress`, `set_tags`, `undo`, `recipe`. Paths must be absolute; unknown fields are refused rather than ignored. In Rust: `Engine::plan_request` (and `Jobs::plan_request` for the asynchronous version) turns an `OpRequest` into a `Plan` without touching anything; `plan_to_json` serialises it. Non-UTF-8 file names survive the round trip. Nothing runs without a confirmation: a deletion request still asks you to type `yes`.

A `recipe` is several requests as one operation (one plan, one undo), and the way to ask for a new file made by another program or for a checksum:

```json
{"op": "recipe", "recipe": "smaller-pdf", "requests": [
  {"produce": {
    "inputs": ["/home/me/report.pdf"],
    "output": "/home/me/report (small).pdf",
    "producer": {"by": "program", "chain": [
      {"program": "gs", "args": [{"lit": "-sDEVICE=pdfwrite"}, {"lit": "-dPDFSETTINGS=/ebook"},
                                 {"lit": "-dNOPAUSE"}, {"lit": "-dBATCH"}, {"lit": "-o"}, "out", {"input": 0}]}
    ]}
  }},
  {"produce": {
    "inputs": ["/home/me/report.pdf"],
    "output": "/home/me/report.pdf.sha256",
    "producer": {"by": "checksum", "algo": "sha256"}
  }}
]}
```

A program is named, never given as a path: `qpdf`, `gs`, `magick`, `sips`, `ffmpeg`, `ffprobe`, `soffice`, or a name the configuration gives a file to (`[recipes] programs`). Its arguments are a list: `{"lit": "text"}`, `{"input": n}`, `"inputs"`, `"prev"` (what the program before it in the chain wrote), `"out"` (the file it must write), `"out_dir"` with `"collect": {"single": {"ext": "pdf"}}` for a program that names its output itself, and `"profile_url"` for LibreOffice. `timeout_secs` (600) and `ok_exit` (`[0]`) can be set. The output is always a new file: see [SAFETY.md](SAFETY.md#actions-that-run-another-program) for what is and is not guaranteed.
