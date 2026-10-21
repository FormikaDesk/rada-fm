# Themes

Every colour on screen comes from a small set of named tokens (text, dim text, accent, selection, cursor row, one colour per file type, success/warning/error…). Nothing is coloured by hand elsewhere; a test reads the sources and fails if it finds a hand-written colour outside `theme.rs`, and another renders every theme and checks that each cell uses only that theme's tokens.

Three looks, each with a dark and a light variant, and two more light themes of their own:

| | dark | light |
|---|---|---|
| `rada` (default): calm azure on a cool neutral base | `rada-dark` | `rada-light` |
| `catppuccin` | `catppuccin-mocha` | `catppuccin-latte` |
| `tokyo-night` | `tokyo-night-dark` | `tokyo-night-day` |
| (only light) | | `nebbia-light`, `carta` |
| `neon` (only dark): an indigo night with a violet selection | `neon` | (a light terminal gets Gesso) |

`rada-light` is **Gesso** (chalk): an almost neutral off-white (`#f4f3f0`), near-black neutral text (`#2f2f2f`) and a blue accent a little cooler than before (`#3d5b88`). Its other tokens (the row under the mouse, fields, borders, selection, key caps, buttons, tabs) are those of Carta moved to the new background together: each keeps its distance in lightness from it, and the warmth of the paper is taken away in the same proportion, so the theme is as coherent as it was, only neutral. `carta` ("paper") is the warm parchment (`#ebe4d5`, brown text) that was `rada-light` until this version: **choose it with `theme = "carta"`** to keep it. `rada` is a night blue (`#1b1d27`). `nebbia-light` ("mist") is the paper idea with a cool grey-blue paper. `carta` and `nebbia-light` are light themes that do not follow the terminal: naming them pins them. These paint their own background; on a 16-colour terminal the terminal's background is kept.

The whole interface has **one background**: the tab bar, the bar of the path, the command bar, the sidebar, the list, the details, the menus, the windows, the status bar and the key hints are all the same colour. Panels are told apart by thin rounded lines; fields, pills and key caps are a step darker (lighter, in the dark theme) than the background. The popups follow the same rule: no shadow, a border in the colour of what lies behind it, and a window is one tone from its line to its inside.

**A transparent terminal.** The default dark theme paints its night blue, so it looks the same everywhere. If you would rather see your own terminal's background (a translucent window, a wallpaper), set `transparent_background = true` in the configuration: the default dark theme then leaves the background alone, as `catppuccin` and `tokyo-night` already do. It is off by default and only the default dark theme is affected; the light themes keep their paper.

![Catppuccin](screenshots/tema-catppuccin.png)
![Tokyo Night](screenshots/tema-tokyo-night.png)
![The Gesso theme](screenshots/principale-gesso.png)
![The Carta theme](screenshots/principale-carta.png)

The light variants use the same tokens, chosen so they read on a pale background: automatic tests check the contrast of every token against the colour it is meant to sit on (7:1 for the main text, 4.5:1 for secondary text, accents, file types and statuses). On Gesso the main text is 12.1:1, the secondary text 5.6:1, the accent 6.2:1, and the border of a menu or window 3.1:1. Catppuccin Latte and Tokyo Night Day are deepened where the original colours are too pale to read as text.

**`theme = "auto"` is the default.** The names `auto`, `rada`, `catppuccin` and `tokyo-night` follow your terminal: rada finds out whether its background is light or dark and takes the matching variant. Name a variant (`theme = "rada-light"`) to pin it, or keep the name and set only the brightness: `appearance = "light"` / `"dark"` (`--appearance`, `RADA_APPEARANCE`). Choose with `theme = "…"` in the config, `--theme NAME` or `RADA_THEME`.

How rada finds out, cheapest and safest first, without ever taking a key from you:

1. what you set (`appearance`, `--appearance`, `RADA_APPEARANCE`);
2. the environment: `COLORFGBG`, the Linux console (always dark), Terminal.app on macOS (it follows the system appearance);
3. asking the terminal for its background colour (the standard OSC 11 query) with a 150 ms limit, before rada starts reading the keyboard. The query is followed by a request that every terminal answers, so once that answer is in, any answer to the colour question is in too and nothing can arrive late into your input. Keys you typed in that moment are replayed, not lost. rada does not ask under tmux unless `allow-passthrough` is on (then it asks through it), never under GNU screen or on a dumb terminal, and not when its input or output is not a terminal; `RADA_NO_TERM_QUERY=1` switches the question off;
4. otherwise: the dark theme.

The result will be shown by `rada doctor` when that command exists; until then it is written to the log (`RADA_LOG=info`). The Windows console is not asked in this version (it falls back to dark unless you set `appearance`).

Colours are true colour when the terminal supports it (`COLORTERM=truecolor`), fall back to the 256-colour palette, and to the 16 ANSI colours (with reverse video for the selection) on a basic terminal. `catppuccin` and `tokyo-night` leave the terminal's own background alone, so transparency and your wallpaper keep working there; they too use one tone for every zone.

**Neon** (`theme = "neon"`) is a dark theme made for the [panels view](interface.md#the-panels-view): canvas `#0e1022`, the inside of the boxes `#13152b`, text `#d9ddf2`, dim text `#7b82a9`, a violet accent `#8b7cf6` and a violet selection `#7058f0` that white reads on, and bright cyan, pink, yellow, green, orange and blue. It meets the same contrast minimums as every other theme.

![The panels view on Neon](screenshots/principale-pannelli.png)

**Entries for the panels view.** Every theme has two more kinds of entry, so that the panels view works with each of them: `inner`, the tone the inside of a box is filled with (a little away from the canvas: lighter on a dark theme, paler on a light one), and six hues (cyan, pink, yellow, green, orange, blue) for the icons and the rounded labels. A test checks that the text, the accent and each hue read on both the canvas and the inside of a box (4.5:1, the main text 7:1), and that the text on a pale selection is dark: on Gesso and Carta the text on the selected row is the darkest tone of the theme, never white. A theme that leaves the terminal's own background alone (`catppuccin`, `tokyo-night`) still fills the inside of the boxes.

