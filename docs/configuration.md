# Configuration

The configuration file is optional.

Optional config: `$XDG_CONFIG_HOME/rada/config.toml`

```toml
profile = "auto"      # auto | modern | basic: see "Terminals"
icons = "auto"        # auto | nerd | unicode-icons | unicode-std | none: see "Terminals"
theme = "auto"        # auto | rada | catppuccin | tokyo-night (follow the terminal), or a fixed
                      # rada-dark | rada-light | catppuccin-mocha | catppuccin-latte | tokyo-night-dark | tokyo-night-day
                      # | nebbia-light | carta
appearance = "auto"   # auto | light | dark: which variant a theme name takes
transparent_background = false  # true: the default dark theme leaves your terminal's background
show_hidden = false
sort = "name"         # name | size | date (date: the most recent first, as in the Finder and Explorer)
reverse = false       # true turns the order round (for a date: the oldest first)
bookmarks = ["~/projects", "/mnt/data"]
images = "auto"       # auto | halfblocks | kitty | sixel | iterm2 | off
image_max_megapixels = 50
image_max_file_mb = 128
pdf_max_file_mb = 512     # larger PDFs are not drawn
pdf_timeout_seconds = 8   # a PDF that takes longer to draw is given up on
svg_timeout_seconds = 8   # an SVG that takes longer to draw is "too complex" and is not drawn
syntax_highlight = true  # colour the preview of source files and the like with the colours of the theme (only the first 64 KB; a language that is not known stays plain). false: plain text
mouse = true          # false: rada never captures the mouse
sidebar = true        # the navigation pane; Ctrl+B toggles it and remembers
show_hints = true     # false: no row of key hints at the bottom (`hints` is the older name)
layout = "auto"       # auto (panels with three columns from 140x36, with two from 70x24, the classic layout below) | panels (three columns from 100x24, two below that, down to 70x24) | classic | compact (no bar, command bar or hints: the most room); `explorer` is the older name of classic. View ▾ chooses too, and what it chose replaces this
show_details_pane = true  # false: never show the details pane by itself (Alt+P still does)
view = "details"      # details | icons | columns: how a new tab shows a folder
tags_column = true    # the coloured dots of the tags in their own column of the Details view
language = "auto"     # auto (from the system) | it | en
remember_tabs = true  # reopen the tabs of the last session
dates = "relative"    # relative ("Today 14:03", "3 days ago") | absolute (dd/mm/yyyy hh:mm, in the order of your locale)
sidebar_density = "auto"  # auto (the same as compact) | compact: one line to an entry, one gap between the groups | comfortable: blank lines between the entries as the height of the terminal allows (a line after every entry where all fit, after every second one, or only between the groups), even if the pane then scrolls
nav = "percorso"      # the NAV box of the panels view: percorso (the places of the sidebar, then only the way to the folder shown: its ancestors and its subfolders) | albero (the places and the whole tree of Home, with an arrow on each folder)
folder_sizes = "auto" # auto: count the size of the folders you can see, in the background | items: only how many items each holds | off
folder_size_seconds = 3   # one count gives up after this long and shows "> size"
folder_size_entries = 200000  # ...or after this many files and folders
zoxide = true         # use zoxide when it is installed: the bar (Ctrl+P) offers its folders and every folder rada enters is added to it. false: rada never calls it
zoxide_path = ""      # the zoxide program; empty looks for `zoxide` in the PATH
git = true            # the panels view asks git, when it is installed, for the branch and the changed and new files of the folder shown (`git status`, in the background, read only, with a time limit). false: rada never calls it
git_path = ""         # the git program; empty looks for `git` in the PATH
keymap = "vim+classic"   # vim+classic (default) | vim | classic
natural_sort = true   # file2 before file10, capitals and small letters mixed (as the Finder and Explorer do)
cmd_keys = true       # macOS: ask the terminal to report the Cmd key, for Cmd+C/V/X and friends

[archives]
max_extract_gb = 8        # an extraction that would write more asks for a typed "yes"
max_ratio = 200           # ...and so does one that unpacks this many times its size

[update]
check = true              # look for a newer rada, at most once a day, in the background
channel = "auto"          # auto | stable | prerelease (see Updates)
method = "auto"           # auto | binary | source | package
source_dir = "~/rada-fm"  # the git checkout the `source` method updates

[devices]
hide = ["tmpfs", "fuse.*", "/boot", "/boot/*"]   # replaces the built-in list of mounts kept out of the
                      # disks: a name is a filesystem type (`fuse.*` a prefix), `/path` a mount point,
                      # `/path/*` it and everything below

[keys]                # per action; replaces all of its keys, [] unbinds it
copy = ["y", "ctrl+c"]
trash = ["d", "delete"]
quit = "ctrl+q"
```

**Sorting.** `sort` and `reverse` are the order every tab starts in. A tab keeps its own order only where you chose it (a click on a column title, **Sort ▾**, the keys `s` and `S`): that one is remembered with the tab, and the others follow `config.toml` at every start, so changing the file changes them. `sort = "date"` shows the **most recent first**, with `reverse = false`; `reverse = true` gives the oldest first. The folders stay before the files (`dirs_first`). When rada is used as a [file picker](desktop.md#use-rada-as-the-file-picker) it starts by date, most recent first, whatever `sort` says.


State lives in `$XDG_STATE_HOME/rada/` (`journal.jsonl`, `log/`, and `ui.json` with the tabs that were open, whether you hid the navigation pane or the details pane, and the view of each tab); the PDF page cache is in `$XDG_CACHE_HOME/rada/previews`. Set `RADA_LOG=debug` for more logging (written to a file, never to the screen).
