# Terminals

rada draws one interface, and decides before drawing how much to ask of the terminal. There are two profiles and nothing in between; `rada doctor` says which one was chosen and why.

**Modern** is the default wherever the terminal speaks UTF-8 and the xterm extensions every current emulator has. The four terminals that come with a system get the same interface; what only some of them offer is used where it exists and replaced where it does not:

| | Windows Terminal | Terminal.app | GNOME Terminal (VTE) | Konsole |
|---|---|---|---|---|
| Alternate screen, mouse (SGR), bracketed paste | yes | yes | yes | yes |
| UTF-8, box drawing, block elements | yes | yes | yes | yes |
| 256 colours | yes | yes | yes | yes |
| 24-bit colour | yes | from macOS 26 (*) | yes | yes |
| Sixel pictures | yes (from 1.22) | no | no (switched off in the builds) (*) | yes |
| kitty / iTerm2 pictures | no | no | no | partly |
| kitty keyboard protocol | (*) | no | no | no |
| Clipboard sequence (OSC 52) | yes | no (*) | no (*) | (*) |
| Background colour (OSC 11) | yes | yes | yes | yes |
| A Nerd Font out of the box | no | no | no | no |
| `Alt+key` delivered out of the box | yes | no (Option is not Meta unless set so) | yes | yes |

(*) from documentation and memory, not yet measured on the terminal itself.

So, in the modern profile:

- **Colours** are 24-bit where the terminal announces them (`COLORTERM`, or a terminal known to draw them), the 256-colour palette otherwise. The pixels of a picture drawn with half blocks follow the same rule.
- **Pictures** are Sixel on Windows Terminal and Konsole (and kitty graphics or iTerm2 images on the terminals that have those), coloured half blocks on Terminal.app and GNOME Terminal. `--images` overrides.
- **Icons** are the best set the terminal's font can draw, tried in this order:
  1. `nerd`: Nerd Font symbols, when the terminal's own configuration names a Nerd Font (Ghostty, kitty, WezTerm, Alacritty, foot, Windows Terminal and Konsole are looked at);
  2. `unicode-icons`: Unicode symbols (dingbats, the rarer arrows, technical marks). No default monospace font has them all, so the terminal draws them from the system's symbol font; this set is chosen when there is one: Segoe UI Symbol on Windows, Apple Symbols on macOS, DejaVu Sans, Noto Sans Symbols, Symbola, FreeFont or Unifont elsewhere;
  3. `unicode-std`: the same marks with only the 46 glyphs every monospace font has (measured: each is in both Consolas and Cascadia Mono), for a system without a symbol font and for the classic Windows console, which falls back on no other font. `✔` becomes `√`, `✖` `×`, `★` `*`, `⇅` `↕`, the spinner `| / - \`.

  `icons = "…"` (`--icons`, `RADA_ICONS`) picks one by hand; `unicode` is still accepted as the old name of `unicode-icons`.
- **Copy path** goes through the terminal where it takes the clipboard sequence, and through the system's own clipboard (`pbcopy`, `wl-copy` / `xclip` / `xsel`, `clip.exe`) where it does not.
- **Keys**: every action has a key without `Alt` and without `Ctrl+Shift`, because not every terminal delivers those. On Terminal.app, *Settings → Profiles → Keyboard → Use Option as Meta key* makes the `Alt` shortcuts work too.

**Basic** is the one fallback for everything else: `TERM` empty, `dumb`, `linux` (the console), `ansi` or a hardware terminal's name (`vt100`, `vt220`…), or a locale that names a character set other than UTF-8. It is the same interface with ASCII glyphs (`+-|` for frames, `>` `v` `~` for arrows and ellipses), the 16 ANSI colours with reverse video for the selection, no icons, no pictures, no mouse, and no question asked to the terminal. Nothing in it is negotiated, so it looks the same wherever it runs.

`profile = "modern"` or `"basic"` (`--profile`, `RADA_PROFILE`) chooses by hand; `icons`, `images`, `theme` and `mouse` still override one thing at a time. `NO_COLOR` removes colour in either profile: bold and reverse video carry the meaning.
