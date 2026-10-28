# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed
- **An operation that is still running is never settled as interrupted, however long it takes.** A copy to a phone or a USB drive that lasted more than 24 hours, seen by a second rada, was closed as "interrupted" while its own process was still writing. Every running operation now holds a lock (a small file in `locks/` next to the journal, held with the system's file lock), and the recovery at start touches an operation only when it can take that lock. The lock goes away by itself when the process ends, `kill -9` included. The 24-hour limit is kept only for operations begun by an older rada, which have no lock.
- **A damaged line in the middle of the history no longer goes unnoticed, and a crash no longer eats the next record.** A line of `journal.jsonl` that cannot be read was skipped in silence, wherever it was. Now the last line without its newline (a write cut by a crash) and a record of a newer rada are still skipped, but any other unreadable line with more lines after it stops the automatic settling and the undo of the operations that had not finished before it, and rada tells you at start which line it is and where the file is. Also, the record written right after a crash used to be glued to the cut line and lost; it is now kept.
- **Removing a file checks it again right where the name is taken off.** Deleting a copy (an undo, a delete) looked at the file and then removed it by its path, so a file or a folder swapped in between could be hit instead. On Linux and macOS the folder is now opened once and the file is looked at and removed through it (`fstatat` and `unlinkat`, never following a link), and on Windows the file is opened so that nobody can write, rename or delete it between the look and the removal. A file that is not what the plan expected is kept and reported as changed.
- **Undo no longer removes the last name of a file with several names whose contents were rewritten.** The check that tells an undo whether a file was changed ignores the status-change time for a file with several hard links (making a link changes it for every name), so contents rewritten with the same size and modification time (`touch -r`, `rsync -t`) went unseen and the undo of a copy could delete them. For these files only, the copy now records a hash of the contents in the history, and an undo that is about to remove the last remaining name compares it: a file that differs is kept and listed as modified after the operation. A name that has another name elsewhere is removed as before, since the contents stay.
- **Two instances of rada can no longer undo the same operation at once.** Both would run the same steps against the same files and each write its own account to the history. An undo now holds a lock for as long as it runs, and a second one is refused with a message that says another rada is undoing it. A plan shown before another rada undid the operation is refused as well ("already undone") instead of running again.

## [0.6.0-rc.17] - 2026-10-25

On a Mac the standard colours renamed in the Finder keep their colour, an Arch Linux package is ready, and the code of the interface is reorganised. Still a **pre-release**.

### Added
- **An Arch Linux package** (`PKGBUILD` in `packaging/aur`), to be published on the AUR.

### Fixed
- **On a Mac a standard tag you renamed in the Finder keeps its colour.** A tag renamed to "🌈" (the orange one) or "👵🏼" (the green one) was drawn gray in the sidebar and on files that carry another number; rada now reads the Finder's list of names (`FavoriteTagNames` in its preferences) and gives a renamed tag the colour of its place in that list. The list is used only when it is the Finder's own (eight entries, the first empty, every standard colour name in its place, at least three of them) and is read again only when the file changes; otherwise it is ignored as a whole. The colour of a tag is now decided in this order: a standard colour name, a name from the list, the number in the file, none. Nothing changes on Linux and Windows.

### Internal
- The functions of `App` live with the groups they work on, and none is longer than 40 lines.

## [0.6.0-rc.16] - 2026-10-24

Tags on a Mac show the colours of the Finder, and the panels view lists your own tags. Still a **pre-release**.

### Fixed
- **On a Mac a tag named after one of the seven colours shows that colour even when the file carries another number**, as in the Finder: a file tagged `Verde` with the number 1 (gray) was drawn gray in the list, the TAG column and the details, and the Finder shows it green. Tags of your own keep the colour of their number. Nothing changes on Linux and Windows.
- **The row of tags in NAV of the panels view lists the tags of your own** after the seven colours (on a Mac "Arancio", "🌈", "👵"), as the classic sidebar does: each is a dot in its colour (gray if it has none), a click or `Enter` opens its list, and `←` `→` walk them. When they do not fit on a line they go on to a second and a third; in a short box the extra lines go first and the others are counted as `+N`. The name of the dot under the pointer or the keyboard is in the status bar. Nothing changes on Windows, which keeps no tags.

## [0.6.0-rc.15] - 2026-10-24

The terminal's background takes the theme's colour while rada runs, so the margin around the window has no frame, the ring and the preview follow a zoom of the font, and the interface's code is split into groups. Still a **pre-release**.

### Changed
- **The terminal's background is the theme's colour while rada runs**, and goes back to what it was when rada ends, so a terminal that keeps a margin around its grid no longer shows a frame of another colour. It also goes back on an error and when the terminal is closed. Nothing is changed with a transparent theme, on 16 colours, with `NO_COLOR`, under tmux or screen, in Terminal.app and in the Windows console. After `kill -9` the terminal keeps the colour until you run `reset`. The window of `Rada.app` shares the leftover room on every side.
- **The blank rows between the bars of the panels view stay from 40 rows**, not 42: a Mac window of 41 rows no longer loses them all.

### Fixed
- **The ring and the picture of the preview are made again when you change the size of the terminal's font** (zoom), so they no longer stay at the old size.

### Internal
- The interface's state is split into groups: the main structure goes from 114 fields to 19, and switching, opening and closing a tab swap one value instead of copying 17 fields by hand. `docs/ARCHITECTURE.md` says what each group holds.
- A test can no longer wait for ever: the chooser tests wait for the folder before moving, a test that lasts more than 120 seconds says what it was doing and stops the run, and the test job may run 45 minutes.
- The tests that draw disks wait for the system's list before setting their own, so a late answer (seen once on macOS) cannot replace them.

## [0.6.0-rc.14] - 2026-10-23

The file chooser gets a preview, a rename onto a name that exists asks what to do, and the panels view keeps the places in sight and cuts long names in the middle. Still a **pre-release**.

### Added
- **The file chooser shows a preview** of what is under the cursor beside the list, from 70 columns; below that `Alt+P` swaps the list and the preview. The chooser never asks for a bigger window.
- **Renaming onto a name that exists asks what to do**: skip, keep both, or overwrite. Overwrite sends the old item to the trash first, so `Ctrl+Z` brings both back.

### Changed
- **Long names are cut in the middle** (`CV_Alessan…Accenture.pdf`), so the start and the end show and similar names can be told apart. This holds in the list, the file chooser, NAV, the path bar, the tabs and the sidebar; a column too narrow for four cells on each side cuts at the end as before.
- **NAV keeps Home and the other places in sight.** When the box is short, PERCORSO gives up rows first ("… N more"), then the blank rows go, and DEVICES shows only the main disk and "… N more".
- **SPAZIO gives a disk's name the room it needs**; the bar shrinks, down to six cells.
- **The blank rows between the bars of the panels view go in steps** with the height of the window.
- The README opens on the panels view, and Windows and macOS are described as a pre-release.

### Fixed
- **`unicode-std` and the panels view.** The marks of the panels use the standard set, and "selected" and "ready to paste" no longer look the same.
- The scroll thumb of NAV no longer covers the end of the highlighted row.
- The list of a file chooser fits its columns to the room it has, and the hint of a binary file's preview is cut to its box.

### Internal
- Code review: `docs/ARCHITECTURE.md`, long functions split, and the tests reorganised by subject.

## [0.6.0-rc.13] - 2026-10-21

The **panels view** — the screen as rounded boxes — with the **Neon** theme, the state of a git repository, folders from **zoxide**, colours in the preview of source files, and the corrections of what was seen in it: the NAV tree opens its folders, follows the folder you are in and scrolls, the favourites no longer repeat the standard folders, and SPAZIO draws the main disk as a ring. Security updates of two libraries. Still a **pre-release**.

### Added
- **The main disk of SPAZIO is a ring** where the terminal draws pictures (kitty, Ghostty, WezTerm… and a theme in true colours): a ring from cyan to violet with the percentage and *usato* in its middle and, beside it, the name, used of total and free. The picture is made at the size of its cells, in four pieces around the hole where the text is, and again only when the percentage changes. Elsewhere the bar stays.
- **The panels view.** On a window of 140 columns by 36 rows or more rada draws the screen as rounded boxes: **NAV** (a tree of folders from Home with the number of items in each, the favourites, a row of the seven tag colours, the trash) and **SPAZIO** (the main disk with its percentage, used of total and free, and the other volumes with a bar of twelve cells) on the left; the folder in the middle (Name, Size as a number alone, Modified, Tag; the selected row filled with the selection colour and rounded; the card of the last operation inside the box); **ANTEPRIMA** (the preview, name and type, rounded labels for type, size and resolution, a short full **Apri** button) and **DETTAGLI** on the right. The tabs, the navigation row and the buttons are rows of their own with a blank row between them, as are the status bar and the key hints; below 42 rows the blank rows go. `Tab` moves the keyboard between NAV, the folder and ANTEPRIMA, and the border of the box it is in takes the accent colour. The tree is listed by a worker (`rada_core::workers::tree_dirs`), never by the drawing. The box under ANTEPRIMA is chosen in one place, ready for an assistant box in a later version. Documented in `docs/interface.md`.
- **`layout = "auto"` (the default), `"panels"`, `"classic"` or `"compact"`.** Auto draws the panels with three columns from 140×36 and with **two from 70×24** (NAV on the left, the folder; SPAZIO under NAV from 36 rows; the command that shows the details — `Alt+P`, `F6`, *Details pane* in View ▾ — puts ANTEPRIMA in the place of NAV and, the same command again, NAV back; `Ctrl+B` still takes the left column away), and the classic layout below that (a tiled window, the file chooser always). With `auto` there is no advice to enlarge the window — rada chooses the view; it is shown only with `layout = "panels"` when the boxes do not fit. `"panels"` chooses between three columns (from 100×24) and two. It changes by itself as the window is resized. In the narrow boxes a label or a button that does not fit whole is left out, NAV drops the numbers when a name does not fit with them, and SPAZIO writes `156/209 GB` and `53 GB liberi` (no decimals from 10 up) where the full figures would be cut, with bars that give up cells so that the names of the disks stay whole. **View ▾** has *Layout: automatic / panels / classic*; the choice is remembered in `ui.json` and replaces the configuration's. `explorer` still means classic.
- **The date column of the list goes below 90 columns** (the classic layout and the middle box of the panels view): the name has the room, and the sort by date stays.
- **The state of a git repository in the panels view.** When the folder shown is in a repository, the top edge of its box says the branch (with an icon and how far it is from its upstream) and the number of changed (`●`) and new (`?`) files; an `M` in yellow or a `?` in green follows a file's name, and the status bar and DETTAGLI say the same. It is read with `git status --porcelain=v2 -b -z` on a thread of its own, only when git is installed, with a time limit; with git missing, refusing or slow nothing is shown and no error is reported. git is told to take no locks, to leave the file system monitor of a repository alone and to ask nothing on the terminal. `git = false` never calls it; `git_path` names the program.
- **`nav = "percorso"` (the default) or `"albero"`: what the NAV box lists.** `percorso` shows on top the places of the classic sidebar (Home, Recenti, the standard folders, the favourites, the devices, the tags, the trash) and, under a blank row and the title PERCORSO, only the way to the folder shown: the nearest place that holds it (or the root of its disk) with its icon, then the folders down to it one to a row and indented, the folder itself highlighted with its number of items, and its subfolders, as many as the box has room for and then "… N more". In a place (Home, Documents, a pinned folder, a disk) there is no PERCORSO, only the highlighted place; the folders have the colour of those of the middle box, and the trash is always the last row of the box. It follows every change of folder; a click or `Enter` on a row shows that folder. `albero` keeps the whole tree of Home as in the earlier builds of this release.
- **The theme `neon`** (`theme = "neon"`): an indigo night (`#0e1022`, boxes `#13152b`) with a violet accent and selection and six bright hues. A light terminal gets Gesso.
- Every theme has two new kinds of entry for the panels view: `inner` (the tone the inside of a box is filled with) and six hues (cyan, pink, yellow, green, orange, blue). Contrast tests cover them; on Gesso and Carta the text on the selection is dark.

- **Jump with zoxide.** In the bar (`Ctrl+P`), when what you type matches no command, no place and no name in the folder, rada also offers the folders `zoxide query --list <text>` gives (at most five, marked *zoxide*); `Enter` opens the one picked, or the first. Every folder rada enters is told to zoxide with `zoxide add`, in the background. If zoxide is not installed nothing happens and no error is shown. `zoxide = false` in `config.toml` makes rada never call it; `zoxide_path` names the program (the tests use a made-up one). Documented in `docs/usage.md` and `docs/configuration.md`.

- **Colours in the preview of source files.** Comments, strings, numbers, keywords, types, functions and attributes are coloured, for most languages (syntect, with its pure Rust regex engine: nothing to build with a C compiler, so Windows, macOS and musl are not harder; TOML, which it does not know, has a small reader of its own). The colours are taken from the active theme (`text_dim`, `success`, `warn`, `accent` and the file-kind colours): none of them is written by hand. Only the first 64 KB and 1000 lines are coloured, by the preview worker and not by the interface; an unknown language or a binary file looks as before. `syntax_highlight = false` in `config.toml` turns it off. `deny.toml` ignores RUSTSEC-2025-0141 (bincode, which syntect uses to read its own bundled definitions) with the reason.

### Changed
- **The colours of Neon are those of its reference screens** (`docs/design/`): cyan `#5fe1e6`, pink `#f472b6`, yellow `#fbbf24`, green `#34d399`, orange `#fb923c`, blue `#60a5fa`, red `#f87171`, the quiet tone `#4b5180` and the track `#262a52`, sampled from the pictures.
- **`rustls-webpki` 0.103.16 and `webpki-roots` 1.0.10** (RUSTSEC-2026-0381, RUSTSEC-2026-0382).
- **`rada update` names the version to a copy installed with cargo while the new one is a pre-release.** Cargo does not pick a pre-release by itself, so `cargo install rada --locked` would find nothing: the advice is now `cargo install rada --locked --version <new version>` (in Italian too). With a stable version it stays `cargo install rada --locked`. `docs/install.md` and `docs/updates.md` say the same.

### Fixed
- **The NAV tree follows the folder you are in and scrolls.** Wherever the folder changes from (the list, the path, `Ctrl+P`, zoxide, back and forward) the tree opens the branches down to it, highlights it and scrolls so that it is in view; a tree longer than its box scrolls with the wheel and with `↑` `↓` when NAV has the keyboard, and a thin bar at its right edge says there is more. `Enter` and a click on a name show the folder in the middle box with the same history as the classic sidebar; a click on an arrow only opens or closes the branch. Tests cover a tree longer than the box, a deep folder, the wheel and the keys.
- **PREFERITI no longer repeats the standard folders** (Documents, Downloads, Pictures…): they are in the tree under Home. Only the folders you pinned are listed, and with none the section is not drawn.
- **The end of a pill or of a row stood on the wrong colour with a theme that leaves the terminal's background alone** (`catppuccin`, `tokyo-night`, `transparent_background`): the round end took the colour the cell had before and blended into the pill. The background behind an end is now always set. A test checks every pill of the panels view.
- **The program's name keeps its lower case at the start of a notice.** The message that closes an update said "Rada 0.6.0-rc.12 è installato…" (seen on Windows): the notice title puts its first letter in capitals, and it did so to the name too. A text that begins with the word `rada` is now left as it is, in Italian and in English.

## [0.6.0-rc.12] - 2026-10-20

rada can now be installed from crates.io (`cargo install rada --locked`, from the moment the crates are published), and `rada update` leaves a copy installed that way to cargo. The pictures of the README and the docs are drawn again, so that they show the centred icons of the narrow sidebar and the Tag column as it is now. Still a **pre-release**.

### Added
- **The crates can be published on crates.io**, so that `cargo install rada --locked` builds rada from there (it works from the moment the crates are published; while only pre-releases exist, add `--version 0.6.0-rc.12`). `rada-core`, `rada-tui` and `rada` carry the data crates.io asks for (description, licence, repository, keywords, categories, a README each, the minimum Rust version) and depend on each other by path and by exact version; each package holds only what compiling needs (no tests, screenshots or media: 380, 285 and 204 KB). `docs/install.md` and the README tell how to install it, `docs/release.md` has the steps to publish a version (`rada-core`, then `rada-tui`, then `rada`), and `scripts/set-version.sh` moves the versions the crates ask of each other along with the workspace.

### Changed
- **The screenshots and the GIFs of the README and the docs are drawn again** (`docs/screenshots`, `docs/media`; the 29 pictures and the two GIFs) with the program of this version, from the tapes and on the made-up home folder of `scripts/demo-home`: they showed the Tag column empty and the icons of the narrow sidebar off centre, which rc.11 fixed. The status bar in them says `v0.6.0-rc.12`.
- **`rada update` leaves a program installed with `cargo install rada` to cargo.** It reads cargo's `.crates.toml` next to the `bin` folder, and when `rada` from crates.io is listed there it replaces nothing and says to run `cargo install rada --locked` (in Italian too). A program put in `~/.cargo/bin` by `cargo install --path`, or only copied there, is updated as before.
- **`scripts/set-version.sh` also moves the install line in the README of the `rada` crate** (`crates/rada/README.md`).
- `contrib/rada-wrapper.sh`, the script `rada setup file-picker` installs, now lives in `crates/rada/assets/linux/` with the other files the program carries inside itself, so that the crate can be built from crates.io.

## [0.6.0-rc.11] - 2026-10-20

`rada setup shell` puts the "cd on exit" in your shell's file for you, rada no longer ends with a terminal error after opening a file with another program (found on a Mac), and a narrow window is better used: the icon of the current place is back in the icon-only sidebar, the Tag column and the file chooser's help line give their room back. **On macOS, make `Rada.app` again with `rada setup desktop`**: the launcher that keeps the window open on an error is in the new one (the old one keeps working). Still a **pre-release**.

### Added
- **`rada setup shell` installs the "cd on exit" for you.** `rada --init <shell>` printed the function (`v`: opens rada and, when you quit, the shell goes to the folder you were in) but you had to put the call in your shell's file yourself. Now `rada setup shell` finds your shell in `$SHELL` (`--shell bash|zsh|fish|nushell|powershell` chooses by hand; PowerShell on Windows), shows what it would write and where, asks, and writes (`--yes` skips the question). **fish** gets a file of its own, `~/.config/fish/conf.d/rada.fish` (`config.fish` is not touched); **bash** (`~/.bashrc`, `~/.bash_profile` on a Mac), **zsh** (`$ZDOTDIR/.zshrc`, else `~/.zshrc`) and **PowerShell** (the profile) get a block between `# >>> rada >>>` and `# <<< rada <<<` that holds only the line calling `rada --init`; **nushell**, which cannot run what a command prints, gets a file of its own in its autoload folder (`vendor/autoload/rada.nu`) with the function in it. If it is already installed (or your file already calls `rada --init`) it says so and writes nothing; `--remove` takes away exactly that block or file, and a file of rada's own that you changed since is left alone. At the end it says to use `v` and to open a new terminal. **Not tried on a real nushell, PowerShell or Mac**: the places are tested in a temporary home. The help screen (`?`) has a line about it.

### Changed
- **The Tag column takes no room where it has nothing to show.** It is left out of a folder in which no item carries a tag, and below 80 columns; the room goes to the names. In a wide window, a folder with tagged items has it as before.
- **The file chooser's help line has a short version.** In a window too narrow for the whole line (for example *Space mark · Enter choose the marked · → open folder · ← up · Esc cancel* at 78 columns) it said only half and ended with `…`; now it gives *Space mark · Enter choose* (in Italian, *Spazio segna · Invio scegli*), and likewise for the folder and save-as bars. The full line is used where it fits.
- **`scripts/set-version.sh <version>`** moves the version in `Cargo.toml`, `Cargo.lock` and the numbers in `docs/` in one go (see `docs/release.md`).

### Fixed
- **The icon of the place you are in was missing in the icon-only sidebar** (a window of less than 100 columns): the highlighted pill showed empty. The pane inside the border is three cells wide, and the icon sat in the third, where the half circle that rounds the right end of the pill was drawn over it. The icon is now in the middle cell, which also centres it in the column.
- **`docs/install.md`: the download example named one version in the address and another in the file name.** Both are now the same, and the script above keeps them so.
- **rada ended with "terminal error: The cursor position could not be read within a normal duration" after opening a file with another program** (reported on a Mac, in Ghostty; the same on Linux in a pseudo-terminal). Starting a program makes rada draw the screen again from nothing (now, and again after 0.3, 1.5 and 4 seconds, because some programs write on the terminal they were started from), and clearing the screen asked the terminal where the cursor is. The thread that reads the keyboard was waiting in `crossterm::event::read()`, which keeps the only reader of the terminal's answer locked for as long as it waits: the question could not be answered, and after two seconds it failed and ended rada. Now the keyboard thread waits for at most 50 ms at a time and is stopped, and waited for, before the terminal is asked anything; what is typed meanwhile waits in the terminal and is read right after, so no key is lost.
- **A terminal that does not answer, or a screen that could not be drawn, no longer ends rada.** The failure goes to the log as a warning and the screen is drawn again from nothing at the next turn. A terminal that did not answer the cursor question is not asked again (the screen is cleared without it). Only a terminal that is gone (closed, hung up) or that fails 50 frames in a row ends rada.
- **Every error that ends rada is written to the log** (`rada.log`, as `ERROR`) with all its causes in one line, before it leaves; before, a terminal error left only the start-up lines there.
- **macOS: a window of `Rada.app` no longer vanishes with the error in it.** When rada ends with an error and was started by `Rada.app`, it shows the message and waits for a key before the window closes. The launchers set `RADA_FROM_APP=1` for this; make `Rada.app` again with `rada setup desktop` to get it (the old one keeps working, but its window still closes at once).

## [0.6.0-rc.10] - 2026-10-16

rada can now be the **file picker of the Linux desktop** (the window a browser opens for "upload a file"), the light theme is a more neutral off-white, tabs respect your sorting settings, and on Windows rada can open folders for other programs. This is also the **first update you can verify with the signature**: if you have rc.9, `rada update` checks `SHA256SUMS.minisig` before it installs this one. Still a **pre-release**.

### Added
- **Windows: `rada setup desktop --default-file-manager` makes rada the program that opens a folder when another program asks** (the Run box, "Open containing folder" in many programs), as Total Commander and Directory Opus can be set up to be. It writes only in your own registry (`HKEY_CURRENT_USER\Software\Classes`, never the machine's): the `open` of the `Directory` class, and takes away a `DelegateExecute` that another replacement left there. It shows each key and value (and what was there) first and writes only after a yes (`--yes` skips the question); what was there is written down, and `setup desktop --remove` puts it back, takes away only the keys it made and leaves alone a value changed since. Explorer is not touched (its own folders, `Win+E`, *Open in new window*, the virtual folders and the drives stay as they are). **Not tried on a real Windows yet**: the logic is tested with a registry in memory.
- **A table of what rada does with each system** in `docs/platforms.md` (*System integration*): opening folders for other programs, "show in folder", the browser's file picker (Linux only: on macOS and Windows it is a window of the system that a program cannot replace), `cd` on exit, trash, tags, launcher. It also says what `Rada.app` can and cannot be on a Mac (the Finder's place and `NSFileViewer` are left alone, and what would need a real Mac to find out is listed).
- **A theme called `carta` (`theme = "carta"`, `--theme carta`)**: the warm parchment that `rada-light` was until now, with its values unchanged. It is a light theme of its own that does not follow the terminal (`carta` or `paper` pin it, like `nebbia-light`).
- **rada as the system's file chooser (Linux).** `rada --chooser-file ANSWER` is the window that other programs get when they ask the desktop for a file ("upload a file" in a browser), through xdg-desktop-portal-termfilechooser, as yazi and ranger do. `Enter` chooses (the paths go to `ANSWER`, one per line, and rada quits), `Esc` cancels and writes nothing. A bar at the top says what is asked: *Choose a file*, *Choose files* (`--chooser-multiple`, `Space` marks), *Choose a folder* (`--chooser-dir`: only folders are listed, `Alt+Enter` takes the one you are in) or *Save as* (`--chooser-save=NAME`: `Ctrl+S` asks for the name with the suggested one filled in, and a name already taken asks twice). **Nothing can be deleted, moved, renamed, copied or changed in this mode**: those actions, the context menu, the command buttons and the key hints are gone, and the confirmation window refuses any plan that is not a new folder, the one thing that can be made (where a folder or a place to save is asked for). A chooser starts **sorted by date of modification, the most recent first** (what is looked for is nearly always what was just made or downloaded), whatever the `sort` setting says; the order can be changed as usual while choosing, and rada leaves its own remembered state, updates and the font offer alone in this mode.
- **`rada setup file-picker`** writes the three files that make it work, for this user only: the script the portal runs (`contrib/rada-wrapper.sh`, which opens rada in `$TERMCMD`, `$TERMINAL` or the first terminal found, with the options that keep the window in front: Ghostty gets `--gtk-single-instance=false`), the portal's `config`, and `<desktop>-portals.conf` (`FileChooser=termfilechooser`, starting from the system's own file so that the other portals keep their backends). It shows what it would write and where, with the new lines marked, and writes only after you say yes (`--yes` skips the question); it keeps a copy of every file it changes, uses no `sudo` and installs no package (it says which one to install). `--remove` goes back. See *Use rada as the file picker* in `docs/desktop.md`.

### Changed
- **The default light theme (`rada-light`) is now "Gesso" (chalk): an almost neutral off-white, `#f4f3f0`, instead of the warm parchment `#ebe4d5`, which was too "paper" (on a Mac most of all).** Text is a near-black neutral (`#2f2f2f`) and the accent a little cooler blue (`#3d5b88`). Every other token (the row under the mouse, fields, borders, selection, key caps, buttons, tabs) is the old one moved to the new background together, with the same distance in lightness from it, so the theme stays as coherent as it was. Contrast on the background: text 12.1:1, secondary text 5.6:1, accent 6.2:1, the border of a menu or window 3.1:1; the thin lines between panels are as visible as they were (1.4:1). Whoever liked the parchment keeps it with `theme = "carta"` (below).
- **`sort = "date"` is the most recent first**, as in the Finder and Explorer, and `reverse = true` is the oldest first. Before it was the other way round (`"date"` showed the oldest at the top and you had to reverse it). The arrow beside the column title says what the list does (↓ from the newest), the Sort ▾ menu's *Ascending* and *Descending* follow it, and a new key keeps the direction of the arrow. If you had `sort = "date"` with `reverse = true` to see the newest first, remove the `reverse` line. Tabs saved by an older version look as they did.

### Fixed
- **CI: every job ends after 30 minutes at most.** A Windows test job once ran for over 30 minutes until it was cancelled. The one test that read the real Windows registry is gone (the logic is tested with a registry in memory), and `reg.exe`, when it is run for real, is stopped after 20 seconds.
- **A tab remembered its order even when you had never changed it, and that order won over `config.toml`** (with `sort = "date"` the Downloads folder was still alphabetical). Every saved tab used to carry `sort` and `reverse`, so a configuration changed later was ignored by the tabs that were open at the time. Now a tab's order is written to `ui.json` only where it was chosen (a click on a column title, the Sort ▾ menu, the keys), at the moment it is chosen; every other tab follows `config.toml` at each start. A `ui.json` from an older version cannot tell the two apart: a tab sorted by name and not reversed is taken for one that was never touched, and any other order for a choice.
- **Two writes of the remembered state close together could leave the older one in the file** (each is made by a thread of its own). The older one is now given up when a newer one has been asked for.
- **`scripts/sign-release.sh` without `RADA_REPO`** signed the release but then failed to upload the signature and publish (`fatal: not a git repository`): it had moved to a temporary folder and `gh` could not tell which repository it was in. The repository is now asked of `gh` once, before leaving the folder the script was started from, printed (`Repository: owner/name`) and passed with `-R` to every call; if it cannot be told, the script stops at once and says to set `RADA_REPO`. `scripts/test-sign-release` has three new cases for it.

## [0.6.0-rc.9] - 2026-10-15

The first **signed** release: `rada update` now checks a signature before it installs anything. Also phones, cameras and USB sticks in the sidebar on Linux, a tidier sidebar, and fixes found in a review of the code. Still a **pre-release**.

**Updating from rc.8:** rc.8 does not check signatures, so the step from rc.8 to rc.9 is checked only by the hash, as before. From rc.9 on every update is checked with the signature. To be sure about this first one, check the download by hand: [verify.md](https://github.com/formikadesk/rada-fm/blob/main/docs/verify.md).

### Documentation
- **Documentation reorganized.** The README is now a short page (what rada is, why to trust it, install, a quick start, the public keys for verifying downloads, links) and everything else it used to hold moved, unchanged in substance, to `docs/`: `features.md`, `usage.md` (first steps, command line, all keys, mouse, requests as data), `interface.md`, `configuration.md`, `themes.md`, `terminals.md`, `archives.md`, `tags.md`, `devices.md`, `updates.md`, `install.md`, `desktop.md`, `platforms.md`, `development.md` and `roadmap.md`. `docs/verify.md` says that releases up to `0.6.0-rc.8` carry no signature. Added `.github/FUNDING.yml` (Ko-fi).

### Added
- **Phones, cameras and sticks in the sidebar (Linux).** DEVICES now lists the phones and cameras (MTP and PTP) that the desktop has opened, under their own names (from `gio mount -li`, or from the folder name without it), and the ones that are **plugged in and not mounted**: a phone or camera that `gio` lists, a USB stick that `lsblk` shows as a removable drive with a filesystem and no mount point. An entry that is not mounted says so after its name, and `Enter`, a click or its menu **mounts** it (`gio mount`, `udisksctl mount`); a phone or a stick can be **ejected** (`gio mount -u`, `udisksctl unmount` and `power-off`). A missing tool is a sentence saying what to install (*install gvfs-mtp*), not an error; a phone that is locked or held by another program says so. The programs run with the C locale and a time limit, in a thread of their own. Before, a phone was invisible: `fuse.gvfsd-fuse` and `/run/user/*` were both in the hidden list, and it is not a mount of its own anyway.
- **The plan says what is different on a phone or camera**, before it runs: the data reaches the device when the file is closed; permissions, owner and extended attributes are not kept; links are skipped and hard links become separate files; renaming and moving are not atomic; there is no trash (a delete copies to the trash of this computer first); undo is not guaranteed; an archive cannot be written onto one. Eight new kinds of warning (`DeviceWrite`, `DeviceAttrs`, `DeviceLink`, `DeviceHardLink`, `DeviceUndo`, `DeviceNoTrash`, `DeviceRename`, `DeviceArchive`), in English and Italian, and the plan schema.
- A stick mounted by hand somewhere unusual (`/mnt/stick`) is **removable** (it has the icon and the Eject entry): the drive is flagged removable, or sits on the USB bus, in `/sys`. Before, only `/run/media` and `/media` said so.
- A test-only variable, `RADA_TEST_GVFS_DIR`, points the folder of the devices at a folder of a test; it exists only in test builds, like `RADA_TEST_SIGNING_KEY`.

- **`rada debug devices`** (hidden): prints what the device code sees, read only: the tools found, every mount and why it is listed or left out, the entries of the virtual filesystem's folder, what `gio mount -li` and `lsblk` printed and what became of each volume and drive in it, and the final lists. A `gio` or `lsblk` that fails, times out or prints something nobody expected is written to the log and shown there, so a device is never left out in silence.

### Changed
- **The sidebar's entries stand one column further in than the titles of the groups** (PREFERITI, DISPOSITIVI, TAG), which are what sets the groups apart, and there is a space after the icon of every entry (it touched the name, a wide Nerd Font symbol most of all). A disk's bar follows the indent of its disk. Names are cut sooner by it (*Canon E… not mounted*). New setting `sidebar_density = "auto" | "compact" | "comfortable"`: `auto` is `compact`, one line to an entry and one gap between groups, as before; `comfortable` adds blank lines between the entries as the height of the terminal allows (between Home and Recent and two between groups, then after every second entry, then after every entry; never fewer than a gap between groups, the list scrolling if it must) and the blank lines are not entries (no click, no arrow lands on them). The column of icons is unchanged.
- **Inside a phone or camera the path bar starts with the device's name** (*motorola edge 40 › Memoria interna condivisa*) instead of `run › user › 1000 › gvfs › mtp:host=…`; the first crumb is the device and a click goes to it. The real folder stays in the details and in the properties.
- **A date of zero is shown as "—"** in the list, the details, the properties and the previews, on any filesystem (a phone gives 1970-01-01 when it has no date).
- **Copying to or from a phone is written for what a phone is.** The file is written under its final name (no temporary file, no rename); permissions, dates and `fsync` are tried and never fail the copy; the file is closed and the answer to the close is read (a plain drop threw it away); what the device kept is measured against what was sent; a move or a verified copy is read back and compared before the source is touched, and what the device half-kept is removed. Sparse files are not looked for on a FUSE filesystem. A move between two phones, or two storages of one, is a copy and not a rename.
- Nothing is counted, tag-read or read for a preview on a phone just because the cursor is on it: folder sizes skip the devices and the folder that holds them, entries there skip the per-entry attribute read, and the preview of a file there says it is not read.
- The test sandbox now puts do-nothing `gio`, `lsblk` and `udisksctl` at the front of `PATH` (a test never lists or mounts the real disks and phones of whoever runs it); `gio` is no longer one of the openers that fail a test when run.

### Security
- **Updates are signed.** Every release now carries `SHA256SUMS.minisig`, a [minisign](https://jedisct1.github.io/minisign/) signature of `SHA256SUMS`, and `rada update` refuses a release that has none or whose signature is not valid for one of the two public keys built into the program (the main one and a reserve). The signed comment must be exactly `rada <tag>` of the release being installed, so an old signed release cannot be shown again as a new one. The order is: signature, then the hash of the archive, then unpacking, and only then is the new program started (`--version`) and, on a Mac, the quarantine flag removed; before, the program was run before any signature existed. With an invalid or missing signature nothing is downloaded or changed, and the way back stays as it was. The two public keys are in `crates/core/src/update/sign.rs`, the README and `docs/verify.md`, and a test checks that they are the same and that no placeholder is left. Source and package updates are unchanged (they trust the repository you chose and the package manager). How to check a download by hand: `docs/verify.md`.
- **Releases are published by hand.** The release workflow creates a **draft** with the binaries and `SHA256SUMS` and publishes nothing; `scripts/sign-release.sh <tag>` downloads the draft, checks the hashes again, signs with the offline key (the passphrase is asked by `rsign`) and publishes. The workflow runs only for tags `v<digit>…`, has a read-only token except in the job that makes the draft, and every action is pinned to a commit hash. The flow is in `docs/release.md`.
- **`rustls` 0.23.46** (RUSTSEC-2026-0346, -0347, -0348, -0349).
- **The state folder is private.** `~/.local/state/rada` (and the journal, the interface state, the update state, the logs) is `0700` and its files `0600` on Unix; an existing install is corrected at start. Before, the folder was `755` and the journal readable by other users of the machine when the home folder allowed it.
- **A `.7z` that asks for more than 1 GiB of memory for one block** (a dictionary of gigabytes) is refused before it is unpacked, as an `.xz` already was.
- **Extracting an archive keeps the links of the archive in view whatever the case** where the destination ignores case (macOS, Windows): a file under `foo/` is not written through a link called `Foo`.
- **Extracted files are written to the disk before they get their name** (`fsync` of the file, then of the folder when the run ends), so a power cut cannot leave a name on an empty file with a journal that says "done".
- **An empty entry in `PATH`** (`/usr/bin::/bin`) is no longer the current folder when rada looks for a program (`7z`, `unrar`, `xdg-open`, the terminal, `pdftoppm`…): one function, `platform::which`, replaces seven copies.
- **An SVG over 20 MB is not drawn** (it was read whole, with no limit).
- **Removing a tree uses `remove_dir_all`**, which cannot be sent through a link swapped in while it walks (it is used by the trash across volumes and by putting back).

### Under the hood
- One `write_atomic` (a temporary name of its own, `fsync`, rename, `fsync` of the folder) for the four state files that had a copy each.
- The crates of the workspace depend on each other by path through `[workspace.dependencies]` (no version to keep up to date), and are marked `publish = false`.
- CI: a job `cargo audit` and `cargo deny` (`deny.toml`: permissive licences, advisories as errors, duplicates as warnings, crates.io only); the token is read-only; the actions are pinned to commit hashes; `--locked` for the build and the tests.

### Fixed
- A flaky test (`spotlight_is_asked_for_the_tag_by_the_name_the_finder_stores`, one failure in 360): starting a program that was written a moment before could fail with "Text file busy" when another test forked at that instant, and the search tool turned the failure into "no result". The retry that the updater already had is now one function (`platform::spawn::spawn_retrying_busy`) used by the updater, the tag search and the PDF tools.
- Removed `verb()`, which nothing called.

## [0.6.0-rc.8] - 2026-10-15

Carta on a less warm paper, the names of the Finder on macOS, a "Recent" entry in the sidebar, and an option to leave the terminal's background alone. Still a **pre-release**.

### Added
- **Recent** in the sidebar, right under Home: a read-only list of the files used lately, the last used first (at most 200), with a *Last used* column instead of *Modified*. Enter opens the file; **Show in folder** (`Alt+F`, and the menu of the right button, also in the list of a tag) opens its folder with the cursor on it. Where the list comes from: macOS, Spotlight (`mdfind`, the last-used date of the last 30 days, asked on a worker with a timeout); Linux, `recently-used.xbel` in the data folder, only files that still exist; Windows, the user's Recent folder, whose shortcuts (`.lnk`) are read for the file they point at (the format is read directly, without the shell). If the source is missing or fails, the entry is not there and nothing is said. Hidden folders and the Mac's `Library` are left out.
- **`transparent_background = true`** in the configuration: the default dark theme leaves the terminal's own background instead of painting its night blue, as `catppuccin` and `tokyo-night` do. Off by default; the other themes ignore it.
- A demo scene `recenti` and the tapes `recenti` and `recenti-carta`; the demo home has a `recently-used.xbel`.

### Changed
- **Carta is less warm**: the background is `#ebe4d5` (it was `#ecdfc6`), fields and buttons `#e1d9c8`, keys `#d9d0bc`, lines `#cbbfa6`, text `#3a3229` (dim text `#5d5446`), the selected row `#ddd6c7`, taken from the reference picture; the contrast tests still pass (the text was darkened, not the background lightened). The whole interface is still one background.
- **macOS: the folders of the Finder have the Finder's names, in the language of the interface.** With the interface in Italian, `Desktop` is shown as *Scrivania*, `Documents` as *Documenti*, `Downloads` as *Download*, `Pictures` as *Immagini*, `Music` as *Musica*, `Movies` as *Filmati*, `Public` as *Pubblica*, `Applications` as *Applicazioni* and `Library` as *Libreria*, in the sidebar, the list, the bar of the path and the tabs. The names come from the system's own table (`SystemFolderLocalizations/<lang>.lproj/SystemFolderLocalizations.strings`, binary, XML or text), with a short fixed table for Italian as a reserve; only folders in the home folder (or `/Applications` and `/Library`) that carry the `.localized` marker are renamed, as in the Finder. Only the shown name changes: the path, the search, copy, move, rename and the command line use the real name (the filter finds a folder by either). In English the names stay as they are, whatever the Finder's own language is. Linux and Windows are unchanged (on Windows the sidebar and the bar of the path already show Explorer's names; the tab keeps the name on disk for now).

## [0.6.0-rc.7] - 2026-10-14

A correction to how popups are drawn, seen on a real terminal (half a cell of the popup's colour stuck out of the rounded line), and with it a new look for the colours: one background for the whole interface. Icons, layout and texts are as they were. Still a **pre-release**.

### Fixed
- **The border of a popup is never a colour of its own.** The cells of `╭─╮│╰─╯` take the colour of the cell just outside the line, so no band of the popup's colour sticks out and none is left between the line and the inside. A menu, a submenu and a notification are filled with the theme's background, no lighter surface; a window dims the screen behind it and its border and inside are one tone (the dimmed background a little lighter; on a light theme, the paper itself). Where a row of another colour (a selected row) goes on beyond the line, the line keeps that colour on its side. The coloured left side and the bottom edge of a notification are drawn with the same rule.
- **No shadows.** Menus, submenus, windows and notifications have none, in any theme; the `shadow` helper of the theme is gone.
- **`popup_border` can be seen on the background in all seven themes** (non-text contrast of at least 3:1, against the menu and against the window), with a test. The `popup` token is now what a menu is filled with: the theme's background.
- **Menus opened near the bottom stay above the status bar and the key hints** (they move up, leaving a row of air over the bar). A submenu that opens on the left no longer leaves dark strips or an empty row at the bottom.
- **A submenu and the list of programs sit right against their menu**, on either side, with the plain background between the two lines: the cursor row that passes behind no longer shows through a column of space.
- **`.docx`, `.doc`, `.odt` and the other office files showed the PDF icon** (every kind of document did, in the list and in the menu header). Word files and text documents have the Word mark; spreadsheets, presentations and e-books their own; only `.pdf` has the PDF one.
- **`catppuccin-latte` and `tokyo-night-day`: the text of the cursor row was dark on a dark gradient.** It is white now, and the solid colour used without shades is where the gradient starts.
- **macOS, Rada.app: one window, one tab.** Ghostty's own question *"Allow Ghostty to execute …?"* is asked for every file it is asked to open as a document (a double click in the Finder, a drop on the Dock, or a path among the arguments, which is how the first Rada.app, started with `-e`, reached it; that also opened a second tab). The maintainers keep it on purpose and it cannot be switched off, so rada does not try to get round it: the launcher gives Ghostty options only and the program is the `initial-command` of the window's settings file. Those settings now also say `window-save-state = never`, so that the windows of the last run are not given back beside the one that runs rada. An older Rada.app has to be made again with `rada setup desktop` (it replaces the whole bundle); the README says how to check it.
- A flaky test of the order by size (it waited a fixed time for the list to be sorted again) now waits for the order.

### Changed
- **One background for every zone.** The tab bar, the bar of the path, the command bar, the sidebar, the list, the details, the menus, the windows, the status bar and the key hints are the same colour; panels are told apart by thin rounded lines (the `track` token); fields, pills and key caps are a step away from the background. Every theme follows this (`cmdbar`, `panel` and the strips of the bars are the canvas), and a test checks it.
- **Carta, the paper theme, on warm parchment** (`#ecdfc6`, no white anywhere): lines `#cfbb96`, brown text `#3b2f22` and `#625340`, accent `#405c84`, the cursor row shaded from `#6d87ad` to `#8a78a8` with the darkest brown text, the selected row `#e0d2b8`, the border of popups `#8e7b5a`, quieter dots for the tags and status colours deepened a little where the contrast tests asked for it. `nebbia-light`, `catppuccin-latte` and `tokyo-night-day` keep their colours on the same scheme.
- **The default dark theme is one night blue** (`#1b1d27`, painted, so it looks the same on any terminal and no longer shows through transparency; `catppuccin` and `tokyo-night` still leave the terminal's own background alone): lines `#2c3040`, accent `#86a0d4`, text `#c8cde0`, the cursor row shaded from `#3e5487` to `#5f4f88`, pills and keys a step lighter than the background.
- **A light theme dims what is behind a window by darkening it a little**, not by making it paler (which turned parchment white), and the window is the paper itself.
- The README starts with *The cross-platform terminal file manager where every destructive action is planned and undoable.*

### Added
- Cell-by-cell tests of the border of a menu, a submenu (on both sides of its menu), the delete window and the stacked notifications, on the real buffer, in a dark theme and in Carta (`crates/tui/tests/popup_edges.rs`); tests of the contrast of the new colours, of the one background and of the options Rada.app gives Ghostty.
- A demo scene `menubottom` and the tapes `menu-fondo` (the menu of a file among the last rows of the list) and `apri-con-carta`; the tapes' terminals have the colours of the new themes.

## [0.6.0-rc.6] - 2026-10-14

The windows that sit in front of the interface — menus, "Open with…", the plan, the undo, the notifications — are redone in one style, and the command line speaks the language too. Still a **pre-release**.

### Added
- **One look for every popup.** A surface a little lighter than the window behind it, a thin rounded border one step lighter again, a shadow of a cell on the right and below (true colour only: on a light theme it is at most about 6% darker than the paper), and corners that take the colour behind them. Two new theme tokens, `popup` and `popup_border`, in all seven themes, with contrast tests. In the basic profile the border is `+-|`. Windows dim the whole screen behind them; menus do not.
- **Menus** with the name and the type of what they are about on top, an icon on each entry, the key at the right (a `›` for what opens another menu, a tick for a choice that is on), groups set apart by a short dashed line, the entry under the cursor a pill with the shade of the list's cursor row, and what takes files away (*Sposta nel cestino*, *Elimina definitivamente*) in red and last. A submenu opens beside its parent, which stays in view with its entry marked; `→` opens it, `←` and `Esc` give the parent back; entries that are greyed out are skipped by the cursor. The menu moves to stay in the window.
- **"Open with…"** titled *Apri photo.jpg con*, each program with a mark and a colour of its own (Firefox, Chromium, LibreOffice Writer, Calc and Impress, GIMP, Krita, Inkscape, VLC, mpv, Okular, Evince, Zathura, VS Code, Kate, gedit, Nautilus, Dolphin, Thunar, Ghostty, kitty, Loupe, Preview, TextEdit, Notepad, Paint, Photos and more; the kind of file when the program is not known), the system's choice with a *predefinito* label, *Altro programma…* and the box *Usa sempre per i file .jpg*. It is a glyph, not the program's own icon: drawing that needs the icon theme and a picture protocol.
- **Plan and confirmation windows** with a round mark, a title that asks (*Eliminare definitivamente 3 elementi?*, *Copia 3 elementi in Documenti*), a quiet line with the size and where from and to, a list of the items with their icon, name and size (six, then *… e altri N*), a label on each (*nuovo*, *esiste già*), warnings as strips in their own colour, the choice for name clashes as a group of pills changed with `←` `→`, and buttons with their keys inside (*Chiudi  Esc*, *Elimina  Invio*). Permanent deletion still asks for the typed word.
- **Undo and redo windows** that say what they take back: *Annullare la copia di 3 elementi?*, *Ripetere lo spostamento…?*, a line per item with *verrà tolto* or *torna al suo posto*, a strip for what was changed since and will be left alone, and buttons *Chiudi* and *Annulla la copia* (*Ripeti la copia*).
- **Notifications** piled at the bottom right of the list, up to three, each with its own six seconds and a bottom edge that shortens; a coloured left side for the state (green, red for an error, yellow, the accent for what is running), the title in bold, quiet details at the right (*42% · 12 MB/s · 3 s*) and the actions as links. A running operation is a card with a shaded progress bar; the bar at the bottom of the screen is gone. The list gets shorter while the cards are there instead of being covered.
- **The key bar with a menu open** names only the keys that work there: `↑↓` choose, `→` open, `←` back, `Invio` confirm, `Esc` close.
- **The command line speaks the language.** `rada --help` and the help of each command, `rada doctor`, `rada update`, `rada setup` and the errors of a wrong choice come from the same table as the interface, in the language of the system or of `language` in the configuration (read before the arguments are). What clap says on its own about a wrongly typed line stays in English.
- A picture the terminal draws itself (Kitty, Sixel, iTerm2) is left out where a popup is drawn.
- `scripts/docs-media` takes several tape prefixes and `DOCS_OUT`, a folder to draw into instead of the repository.

### Fixed on Windows
- A program opened from rada (an Electron editor such as VS Code is the case that showed it) no longer writes its logs on rada's console, which scrolled the screen and spoiled the panels. Such programs call `AttachConsole(ATTACH_PARENT_PROCESS)` and so attach to the console of whoever started them, which was rada. Now rada starts a helper of its own (its executable, with a hidden first argument) with no console and no window (`DETACHED_PROCESS | CREATE_NO_WINDOW`) and with standard input and error closed; the helper makes the call to the shell (`ShellExecute`, or the handler chosen in *Open with…*) and ends, so what it started has no console to attach to. The reason of a failure comes back through the helper's standard output. Explorer, Windows Terminal and the program typed in *Other program…* are started with the same flags and no standard streams.

### Changed
- `Ctrl+L` draws the whole screen again from nothing (clear and a full redraw), wherever it is pressed, besides starting the editing of the address; the same is done right after a program is started and a few times in the seconds after, on every system, so that anything that spoils the terminal is put right. The help says so.
- In Italian *Annulla* only takes an operation back; closing a window is *Chiudi*, and stopping one that runs is *Interrompi*.
- The thin shaded line under the bar of the path and search is gone, in every theme.
- The sentences that put a count before a word that agrees with it have a form for one (*1 selezionato*, *1 elemento copiato*), and a test keeps it so.
- Letters with Alt are shown in capitals (`Alt+Z`); times have a space before their units (`7 s`, `1 min 20 s`); notification titles start with a capital.
- The GitHub actions of the workflows move to the versions that run on Node 24.

### Fixed
- The archive card of the details pane no longer breaks a size over two lines or cuts its keys.
- The logo of the demo folder is a recognisable one, not a purple square.
- In the basic profile the half-row of air under the tabs, which came out as `#`, is gone.


## [0.6.0-rc.5] - 2026-10-14

A restyling release: a softer look, the interface in Italian, one bar at the top, a Tag column, notifications and a third view. Still a **pre-release**.

### Added
- **The interface speaks Italian.** Every text of the interface — the plan window and its warnings, the journal, the Type column ("Immagine JPEG", "Archivio (tar.gz)"), dates ("3 giorni fa", "Ieri", "Oggi 15:21"), sizes (`70,7 KB`), the help, the menus, the hints, the key names (`Invio`, `Spazio`, `Canc`) and the notifications — comes from one table of English and Italian texts. The language is the system's (`LC_ALL`, `LC_MESSAGES` and `LANG` on Linux, the language preferences on macOS, the interface language on Windows) or the one the configuration asks for: `language = "auto" | "it" | "en"`. A key without both languages does not compile, and tests check the placeholders and that the layout holds the longest Italian words at 170, 140, 120 and 80 columns. The configuration accepts the Italian names of keys (`invio`, `spazio`, `canc`).
- **One bar at the top.** The path as crumbs and *Search files or type a command…* are one field, a pill with the key that starts it (`Ctrl+P`) and a thin shaded line under it. `Ctrl+P`, `Ctrl+L` and `Ctrl+F` all start it: a path is completed and gone to, words filter the files as you type and offer the commands (and the places) that match, `Enter` runs the one picked with the arrows (or keeps the filter), `Esc` goes back to the path. `Ctrl+L` shows the path selected: the first key typed replaces it. The buttons *back*, *forward*, *up* and *refresh* are little pills on the left, the view in use is a pill on the right.
- **Soft shapes.** The three panels are cards with rounded corners; the buttons, the tab in front, the key chips and the column titles are pills; the row under the cursor, the selected rows and the current place of the sidebar have rounded ends. Where the font has no half-circle glyphs, a pill is a flat rectangle of the same size.
- **A shade behind the cursor.** The row under the cursor is shaded cell by cell from `grad_row_start` to `grad_row_end` with bold white text, and the line under the bar from `grad_line_start` to `grad_line_end`: four new theme tokens in every theme (dark `#3d59a1 → #6a4c9c` and `#7aa2f7 → #bb9af7`, Carta `#476fa8 → #7d58a6` and `#3d68a3 → #8a57a8`). The white text reads at 4.5:1 or better at both ends and everywhere between, in every theme (a test). On a 256-colour terminal the shades are solid colours; in the basic profile and without Nerd Font glyphs there are neither shades nor special glyphs.
- **A Tag column** in the list: a dot in the colour of each tag, three at most, then `+N`. *View ▾* shows or hides it (`F8`, remembered), and so does `tags_column = true | false` in the configuration. It comes last and goes first when the name would get too little room.
- **Disks as ten segments**, `■■■■■■□□□□`, in the sidebar and the status bar (`[######----]` in the basic profile).
- **Notifications** at the bottom right, a soft card: after a copy, move, trash, rename, compression, extraction or tag change, *3 elementi copiati in Documenti* with **Annulla** (`Ctrl+Z`) and **Mostra**, both clickable. They go by themselves after about six seconds or at the first key (which still does its job), never take the keyboard, and queue one at a time; an error stays until it is closed (a click, or `Esc`). Tests run them on a fake clock.
- **The Columns view**, as in the Finder: the folder above (the one you are in picked out), the folder you are in, and what the item under the cursor holds or its preview. `←` goes to the folder above, `→` into the folder under the cursor; `Ctrl+3`, *View ▾* and `view = "columns"` choose it.
- **Pictures with rounded corners**, a mask applied to the pixels before they are sent to the terminal and blended into the background colour, so every graphics protocol (and the half blocks) shows it.
- `scripts/demo-home` makes a home folder of made-up files, and `docs/tapes/` has the [VHS](https://github.com/charmbracelet/vhs) tapes that draw the screenshots and the GIFs of the README (`scripts/docs-media`).

### Changed
- The Palette is no longer a window: it is the bar at the top.
- Notifications are no longer a one-line pill that replaces the last one.
- The command bar has *Tag ▾* in place of *View ▾* (the view is the pill on the right), and gives up its icons before its words on a narrower terminal.
- The key hints close up their gaps before dropping one.
- The list leaves a blank row under the column titles.
- `v` goes through all three views (Details, Icons, Columns).
- The help's column of names is as wide as the longest name, so the Italian ones no longer touch the keys.

### Fixed
- What a picture does not fill of its last cells is the background colour, not black.
- In the basic profile the half-block rows that pad the bars are plain colour, not rows of `#`.

## [0.6.0-rc.4] - 2026-10-13

A release in which tags have a place of their own, updates can be seen, "Open with…" knows what is installed, and the terminal profiles and icon sets of @chrias (pull request #1) join the line: thank you. Still a **pre-release**.

### Added
- **Tags in the navigation pane.** A **TAGS** section lists the seven colours always, in the Finder's order and called what your system calls them, then the tags of your own that rada has seen in the folders you opened. A click opens **Tag: Blu**, the items that carry it wherever they are: Spotlight (`mdfind`) on a Mac, Baloo (`baloosearch`) on Linux, and a walk of the home folder in the background (hidden folders skipped, 30 seconds or half a million items at most) where the index has nothing to say; none on Windows. The list has the operations of any list of items and refuses what would create something in it; each row names its folder. A colour is found under any of its names, a tag of your own that is blue is another tag.
- **Updates can be seen.** A pill in the accent colour in the tab bar, **↑ Update to X**, clickable like `Alt+U`. The status bar always shows `v<version> ⟳`; a click looks for a newer version at once and says *rada is up to date*, or opens the release notes. **Help ▾** is now a menu: the keys, *Check for updates* and *About rada* (version, channel, mode of updating).
- **Open with… knows what is installed.** The window lists the programs that can open the file, the system's choice first and those used lately after it, filtered as you type, with **Other program…** (the free field of before) at the end. Linux reads the `.desktop` files with a compatible `MimeType`, `mimeinfo.cache` and `mimeapps.list`, and starts a program with its `Exec` line, in a terminal for `Terminal=true`; macOS asks Launch Services and opens with `open -a`; Windows lists the programs the shell associates with the extension. *Always use this for this kind of file* is kept in rada's own state and never touches the system's defaults.

### Changed
- The dots after a name are three at most, then **+N** for the N more (it was a bare `+`).
- The Finder's colour number is always written, `0` for a tag with no colour, so what the Finder reads is what rada meant.
- The Help button opens a menu; `F1` and `?` still open the keys directly.

### Fixed
- Rename on an item of a list of tagged items started from its whole path instead of its name.

### Terminal profiles and icon sets (by @chrias, pull request #1)

Thank you, @chrias.

#### Added
- **Two terminal profiles, chosen before anything is drawn** (`crates/tui/src/caps.rs`). *Modern* is the same interface on Windows Terminal, Terminal.app, GNOME Terminal and Konsole, with what only some of them offer (24-bit colour, Sixel, the clipboard sequence) used where it exists. *Basic* is one fixed fallback for plain terminals (`TERM` empty, `dumb`, `linux`, `vt100`…, or a non-UTF-8 locale): ASCII glyphs, 16 colours, no mouse, no pictures, no terminal queries. `profile = "auto" | "modern" | "basic"`, `--profile`, `RADA_PROFILE`. The README has the table of what each terminal supports.
- `rada doctor` prints the profile and every capability, with the reason.
- The basic profile is a backend in front of the real one (`compat.rs`) that replaces the interface's glyphs with ASCII ones of the same width: the interface itself is drawn once, the same way. A test reads the sources and fails when a glyph has no stand-in.
- `NO_COLOR` is honoured: no colour is written, bold and reverse video carry the meaning.
- Copy path uses the system's clipboard program (`pbcopy`, `wl-copy` / `xclip` / `xsel`, `clip.exe`) on the terminals that ignore OSC 52 (Terminal.app, GNOME Terminal, Konsole).
- `Ctrl+O` / `Ctrl+G` go back and forward and `F6` shows or hides the details pane: every action now has a key without `Alt` or `Ctrl+Shift`, and a test keeps it so.
- Nerd Font detection reads Windows Terminal's `settings.json` and Konsole's profiles.
- **Three icon sets with a fallback between them.** `nerd` when the terminal is set to use a Nerd Font; `unicode-icons` (the former `unicode`, still accepted under that name) when the system has a symbol font for the terminal to draw the rarer symbols from (Segoe UI Symbol, Apple Symbols, DejaVu Sans, Noto Sans Symbols…); `unicode-std` otherwise, which draws the same marks with only the 46 glyphs that every default monospace font has. `icons = "auto"` walks that chain; `rada doctor` prints the result.

#### Changed
- Windows: colours are 24-bit (the console sets neither `TERM` nor `COLORTERM`, so they were the 16 ANSI ones); pictures are Sixel in Windows Terminal, with the cell size asked to the terminal because the system does not report it; a theme that follows the background now asks the console for its colour, as on Unix.
- Pictures drawn with half blocks are brought down to 256 or 16 colours where the terminal has no more (Terminal.app before macOS 26).
- GNOME Terminal and Terminal.app are no longer asked about graphics at start: they have none.

## [0.6.0-rc.3] - 2026-10-13

A release of interface polish, a softer light theme, folder sizes and the first fixes from using rada on a Mac. Still a **pre-release**.

### Added
- **The size of folders.** The Size column now shows how big a folder is, counted in the background and only for the folders you can see: it shows `···` while it counts, and a figure when it is done (`> 3.2 GB` when the count had to stop early). The details pane says `12 items · 4.8 MB` for a folder, and sorting by size uses the figure once it is known. rada does not follow links, does not leave the disk the folder is on, and never counts `/proc`, `/sys`, `/dev` or a network share; it gives up after a time or a number of entries you can set. Opening another folder stops the counting. In the configuration: `folder_sizes = "auto"` (the default), `"items"` (only how many items each folder holds, which is quick) or `"off"`, with `folder_size_seconds` and `folder_size_entries` for the limits.
- **Two new light themes, easier on the eyes.** `rada-light` (also what `theme = "auto"` takes on a light terminal) is now "paper": a warm off-white instead of pure white. `nebbia-light` ("mist") is the same with a cool grey-blue paper. Both paint their own background (on a 16-colour terminal the terminal's own background is kept). Automatic tests check the contrast of the main text (7:1), of the secondary text and the accent (4.5:1) on the background, the strip of hints, the fields and the selected rows, for every theme, light and dark.
- **Half a row of air** between the tabs and the address row, and **rounded address and search fields** with Nerd Font icons (plain rectangles without them). When the window is short, this row is the first thing to go.
- `RADA_TAG_LANGUAGE` chooses by hand the language the names of the seven tag colours are written in.

### Changed
- The buttons of the details pane are separate pills: *Open* across the pane, a blank row, then *Open with…* and *Preview* side by side (and *Quick Look* below them on a Mac, or in place of *Preview* when the pane is short). A blank row is kept between the last property and the status bar.
- The preview of a folder in the details pane leaves out what the list leaves out (`.DS_Store`, `.localized`, anything the system hides) and follows the *Hidden files* setting.
- A folder that is both a standard place and a bookmark is listed once in the sidebar (and a standard folder that two settings name is one entry).
- `tag:red` in the search field also finds a colour by the name your system gives it (`tag:rosso`).

### Fixed
- **Tags on a Mac in another language.** A colour chosen in rada showed right for a moment and then became a grey dot, because the Finder in Italian calls blue "Blu" and gives a tag called "Blue" no colour. The seven colours are now written with the names your system uses, with the colour number the Finder writes after them (`Blu` and `4`), and a tag's colour is read from that number.
- **Rada.app on a Mac opened two windows**, each with rada. It started Ghostty with `-e`, for which Ghostty asks "Allow Ghostty to execute …?" on every start (its author keeps that question on purpose, for security) and, started through `open`, runs the command twice. The command is now the `initial-command` of the settings file of rada's window: one window, one rada. The README says what is left of the question.
- macOS: a folder called `Chrome Apps.localized` is shown as `Chrome Apps`, as the Finder shows it (Properties keeps the real name).
- **Opening a folder or an archive could be cancelled by chance.** If the watcher reported a change of the folder you were leaving just after you asked to go elsewhere, rada read the old folder again instead and you stayed where you were (it showed on a Mac, whose notifications arrive late). The folder you asked for is now always the one that opens.
- The update check that starts the downloaded program (`--version`) tries again when the program is still "busy" for writing, which rarely failed an update for no reason (and, in tests, a build now and then).
- Tests that depended on how fast the machine was, or on how long a path was, now wait for the state they need (a double click, the watcher being ready, an extraction's answer). The same commit used to fail different tests on different runs of the continuous integration.

## [0.6.0-rc.2] - 2026-10-13

The release of the first real try-out on a Mac. Still a **pre-release**: Windows and macOS are tested by CI on GitHub's machines and, for macOS, by one real Mac; please report what looks odd.

### Added
- **rada updates itself.** Once a day at most, in the background, rada reads GitHub's public list of releases (it sends only `User-Agent: rada/<version>`; a failure is silent). A stable version sees stable releases, a pre-release also sees later pre-releases (`channel` in `[update]` decides by hand). The status bar says *New version X available — press Alt+U to update* (clickable; `U` already means the history in the vim keys), a window shows the release notes, and `rada update` does the same from the command line (`--check` only looks, `--rollback` goes back, `--yes` skips the question). How it updates follows how rada got there (`method = "auto"`, or `binary`, `source`, `package`): a **downloaded** program is replaced after its SHA-256 matches the release's `SHA256SUMS` and the new program has answered `--version` (atomic everywhere, with the running-file rename on Windows; the old one is kept for `--rollback`; on a Mac the new file is checked for the quarantine flag); a program **built from a git checkout** (`source_dir`) is updated with `git pull --ff-only`, `cargo build --release --locked`, a copy in place and `cargo clean` — only from a clean `main`, in the background, with the old program untouched if a step fails and the log one key away; a program installed by a **package manager** is never touched and the window says which tool to use. New `[update]` section: `check`, `channel`, `method`, `source_dir`.
- **macOS, as a Finder user expects it.**
  - When macOS refuses a folder until you allow it (the Desktop, Documents, Downloads, iCloud Drive, external disks), rada says *macOS has not given Ghostty access to Documents* with a button that opens *System Settings → Privacy & Security → Files & Folders* (and the explanation of Full Disk Access); the sidebar marks the folder with the same words.
  - **Packages** (`.app`, `.bundle`, `.framework`, `.photoslibrary`, `.pages`, `.numbers`, `.key`, `.rtfd`, and what the system says is a package) open like files, sort with the files, and have *Show package contents* in the menu.
  - Hidden as in the Finder: `.DS_Store`, `._*`, `.localized` and folders flagged hidden (`~/Library`).
  - The sidebar and breadcrumbs show the **names the Finder shows** (localized folders, "Macintosh HD"); Properties and *Copy path* keep the real path. **iCloud Drive** appears in the sidebar.
  - **Eject** for external disks and disk images (menu, and `Del` on the disk in the sidebar), with a confirmation and a plain message when the disk is busy ("Finder (process 412) is still using it").
  - **Quick Look** in the menu, under *Preview* in the details pane, on `Alt+Y` (and `Cmd+Y`) and `Q` in the full-screen preview. `Space` is still *select*.
  - **Cmd keys** in the classic scheme (`Cmd+C/V/X`, `Cmd+Backspace` to the trash, `Cmd+I`, `Cmd+O`, `Cmd+↑`, `Cmd+[` / `Cmd+]`, `Cmd+Shift+.`, `Cmd+Z`, `Cmd+Y`, `Cmd+A`, `Cmd+Shift+N`) for terminals that report them (the kitty keyboard protocol, asked for on a Mac only; `cmd_keys = false` stops that). Which keys Ghostty keeps for itself was read from its source, and the Ghostty settings file made for rada's own window frees them with `keybind = …=unbind`; the README says plainly what works and what does not.
- **Natural order everywhere**: `file2` before `file10`, capitals and small letters mixed, as the Finder and Explorer do (this was already how rada sorted; `natural_sort = false` now turns it off).
- **Windows names as Explorer shows them**: the sidebar and breadcrumbs use the shell's display names for the Known Folders and the drives ("Documenti", "Disco locale (C:)"); the real path stays in Properties and *Copy path*. **Eject** for removable drives without administrator rights (lock, dismount, release the media).
- **Previews with nothing to install.** PDF first pages on macOS (`sips`) and Windows (`Windows.Data.Pdf`); page count, title and author read from the file when `pdfinfo` is absent; Quick Look thumbnails on macOS for Office documents, Pages, Numbers, Keynote, RTF and videos; the shell's thumbnails on Windows. On Linux poppler is still used, and when it is missing the message gives the install command for *your* distribution. For Windows the choice was between the system renderer, the pure-Rust `hayro` and pdfium: `hayro` needs Rust 1.92 (the minimum here is 1.90) and bundles fonts, pdfium needs a DLL the person would have to provide, while `Windows.Data.Pdf` is part of Windows 10 and later, needs no dependency of ours beyond the Windows bindings, and draws what Edge draws.
- **rada has its own identity.** On Linux the window is started as `com.formikadesk.rada` (`ghostty --class=`, `kitty --class`, `alacritty --class`, `wezterm start --class`, `foot --app-id`) and the launcher entry says `StartupWMClass`, so bars and workspaces show rada's icon. On macOS `rada setup desktop` makes `~/Applications/Rada.app` with rada's icon at every size (`.icns` generated from the logo): a click opens rada in Ghostty (a 180×45 window with its own settings), else iTerm2, else Terminal (`--desktop` adds a link on the Desktop; the Dock still shows the terminal's icon). On Windows it makes a Start menu shortcut (and `--desktop`: Desktop) and, with Windows Terminal, a profile "rada" as a fragment; the icon is built into `rada.exe`.
- **`rada setup font`** installs JetBrainsMono Nerd Font Mono for the user (Nerd Fonts v3.4.0; every file checked against a SHA-256 in the code; the OFL licence goes with it; no administrator rights; `--remove` undoes it). At the first start where the icons would not draw, rada **offers** it — once, and only if you say yes. Ghostty draws the icons with no font to install, and rada now detects it (it used to fall back to plain markers there).
- **A line that asks for a bigger window** when the terminal is smaller than the full layout (100×24): *Enlarge the window for the full layout (now 80×24)*, closed with `Esc` for the session.
- **Tags** (macOS: the Finder's; Linux: `user.xdg.tags`; none on Windows): coloured dots in the Details and Icons views, a *Tags* row in the details pane, a *Tags ▸* menu (the seven colours, the tags the items already have, *New tag…*, *Remove all tags*) that works on a selection, and `tag:red` in the search field. A change of tags is an **operation**: a plan, a journal entry and an undo (which leaves alone tags that were changed since); in the JSON requests it is `set_tags`. Copies and moves keep tags (on macOS, tags only: the other extended attributes are still not carried over, and the plan says so).
- CI runs clippy with `-D warnings` on Windows and macOS too, and cancels a run when a newer push to the same branch arrives. `scripts/xcheck` type-checks and lints the other systems from Linux.

### Changed
- The classic scheme gains `Alt+U` (update), `Alt+Y` (Quick Look) and `Alt+↓` (show package contents).
- Icons are chosen as before, but Ghostty counts as a terminal that draws them.
- The plan's note about attributes on macOS and Windows now says that tags are kept where the system has them.

### Fixed
- macOS: a function only the Linux sparse-file copy uses was reported as unused by clippy.
- A change in the folder that arrived while it was being read again (F5, or a tag change) could leave it showing *loading* for good; such a change now asks for another full read.
- A test of the plan window showed the date of the day it was written; it is pinned now.

## [0.6.0-rc.1] - 2026-10-10

The first release with ready-made binaries, and a **pre-release**: Windows and macOS have been tested by CI on GitHub's machines but not yet by people using them every day. Please report anything odd in an issue.

### Added
- **Windows and macOS (pre-release).** The platform layer is real on both. Windows: the Recycle Bin (move, put back, check that an item is still in it), drive letters with type, label and free space, the Known Folders (Desktop, Documents, Downloads…, wherever they were moved), opening with the default program, `Open with…`, reveal in Explorer and a terminal in the folder, hidden and system attributes. macOS: `~/.Trash` and `/Volumes/<name>/.Trashes/<uid>` where the Finder looks, volumes from `/Volumes` with `statfs`, opening with `open(1)` (`-a` for `Open with…`, `-R` to reveal) and the terminal you are in, case- and normalisation-insensitive names. Attributes (ACLs, alternate data streams, extended attributes, resource forks) are not carried over by a copy yet, and the plan says so.
- A trash that is a folder (`DirTrash`), used by macOS and by the tests of every system, so that no test ever puts anything in the real Recycle Bin or Trash of the machine running it.
- **Binaries.** A tag `v*` builds Linux (musl, static; x86_64 and aarch64), Windows (x86_64, zip) and macOS (x86_64 and aarch64) binaries with `.github/workflows/release.yml`, checks that each one starts with `--version`, and publishes them with `SHA256SUMS` and these notes as a GitHub Release (a pre-release when the tag has a hyphen, as in `-rc.1`).
- CI runs the whole workspace — core, interface and program — on Linux, Windows and macOS. Tests that cannot run on a system say why with `#[ignore = "<reason>"]`.

### Fixed
- Windows: a move inside a volume was planned as copy-and-delete because the volume of a file was not known; it is a rename again. The volume, the file index and the number of links are now read from the file.
- Windows: moving to another volume was not recognised as such (`ERROR_NOT_SAME_DEVICE`), and removing a folder that is not empty was not told apart from other errors, so undo of a folder that gained files reported a failure instead of keeping it.
- Windows: the folder rada starts in, and the one `Open a terminal here` opens, were written `\\?\C:\…` in the address bar; they are plain paths now.
- Windows: a link that leads back to itself is reported as circular (it was reported as broken).
- Bulk rename compared names as text, so `d/2` and `d\2` were two different places on Windows; names are compared by component.
- Paths under the home folder are shown as `~/a/b` on every system (they were `~/a\b` on Windows).
- The schema files and the snapshots are checked out with `\n` line ends on every system (`.gitattributes`).

## [0.5.0] - 2026-10-10

### Added
- **Explorer look.** Tabs, an address bar, a command bar, a navigation pane, a details pane, a status bar and a row of key hints, arranged like the file managers people already know, with everything rada stands for kept: a plan before every operation, undo, vim and classic keys, light and dark themes. `layout = "compact"` hides the address bar, the command bar and the hints.
- **Tabs.** `Ctrl+T`, `Ctrl+W`, `Ctrl+Tab` / `Ctrl+Shift+Tab` / `Ctrl+PgDn` / `Ctrl+PgUp`, `Alt+1…9`, click, middle click. Each tab has its own folder, history, cursor, selection, sort order and view; clipboard, journal and undo are shared. Remembered between sessions (`remember_tabs`).
- **Address bar** with clickable segments and, with `Ctrl+L` / `Alt+D` / `F4` or a click, a text field with completion from the subfolders (listed by a worker); **search** field that filters the folder (`Ctrl+F`).
- **Command bar**: New ▾ (folder, empty file), Cut, Copy, Paste, Rename, Compress, Delete, Sort ▾, View ▾ and *Undo · copy of 3 items*, the description coming from the journal. Unusable buttons are faint; every button runs the same action as its key, through the same plan.
- **Details view** with checkboxes and sortable Name / Date modified / Type / Size, types in plain English from a table in `rada-core` ("JPEG image", "Archive (tar.gz)", "Rust source"…), relative dates ("Today 14:03", "Yesterday", "3 days ago", "2 weeks ago", then the full date in the order of the locale; `dates = "absolute"`), and an **Icons view** (a grid with big icons, two-dimensional navigation; `Ctrl+1` / `Ctrl+2`, `v`, View ▾ or the status-bar buttons).
- **Details pane** (`Alt+P`): a big preview, Open, Open with…, a full-screen Preview, Dimensions, exact Size, Modified, Created, Location and Permissions. A column from 140 columns, an overlay below that.
- **Navigation pane**: Home, PINNED (your standard folders and the folders you pin, with a pin), DEVICES (a bar of how full each disk is and what is free) and Trash. Pin from the menu or with `B`.
- **Context menus** for an item (Open, Open with…, Cut, Copy, Paste, Rename, Compress…, Extract here / to…, Pin to navigation, Copy path, Delete, Properties) and for empty space (New, Paste, Sort by, View, Refresh, Open a terminal here); `Shift+F10` or the Menu key.
- **New empty file** (`N`, `Alt+N`, New ▾): planned, journaled and undoable like everything else; in the JSON request as `make_file`.
- Windows-style keys in the classic scheme, nothing taken from vim: `Alt+←` / `Alt+→` / `Alt+↑`, `F5` refresh, `Ctrl+L` / `Alt+D` address, `Ctrl+F` search, `Ctrl+Shift+N` new folder, `F2`, `Del`, `Shift+Del` (always a plan, said to be permanent), `Ctrl+A`, `Ctrl+Z`, `Alt+Enter` properties. `Ctrl+L` now edits the address (the jump palette is `Ctrl+P` and `m`); `Backspace` is *back* with the classic keys alone and still the parent folder where the vim keys are on.
- Configuration: `layout`, `show_details_pane`, `show_hints` (the old `hints` still works), `remember_tabs`, `dates`, `view`. Sort by type.
- Selection and cursor have a clear hierarchy: selected rows are quiet with a ticked box and a bold name, the row under the cursor is solid with white text and a bar at its left, and a ticked box stays visible under the cursor. New theme tokens for the selection, the cursor text, the command bar, the hints strip and its keys, fields, buttons, ticks and disk bars, in every theme.
- The "copy path" command puts the path on the system clipboard through the terminal (OSC 52).
- Tests: every screen at 170×35, 140×35, 120×30, 100×26, 80×24, 60×20 and 40×16 in both themes (text and a map of theme tokens), the Icons view, both context menus and two tabs; tabs, the address field, the command bar, checkboxes, the Icons grid, the details pane, menus, dates and the file types; the keymap for every new key.

### Changed
- Sizes are written `KB`, `MB`, `GB` (still powers of 1024), as Windows does.
- The old preview pane is now the details pane; the preview facts of a picture or a PDF are properties, and the full-screen preview keeps them under the picture.
- The state of a tab is kept apart from where and how it is drawn (the list and the grid are drawn into any area they are given), which leaves room for a split view in the future. Nothing of that is visible yet.

## [0.4.1] - 2026-10-10

### Changed
- zstd (`.tar.zst`, `.zst`) now uses the `zstd` crate, bindings to the official library (its source is bundled and compiled with the build, no system library is needed), instead of the young pure-Rust `structured-zstd 0.0.59`. Reading and writing are both supported, with the same behaviour as before; archives written by rada carry a checksum and pass `zstd -t`. The other formats are still pure Rust.
- The minimum Rust version is now 1.90 (the real minimum of the dependencies, checked with `cargo +1.90 build --workspace --locked`); 1.92 was needed only by the removed encoder.

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
