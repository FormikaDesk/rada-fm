# rada

**A terminal file manager for Linux, macOS and Windows where every file operation shows a plan first and can be undone.**

[![CI](https://github.com/formikadesk/rada-fm/actions/workflows/ci.yml/badge.svg)](https://github.com/formikadesk/rada-fm/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/formikadesk/rada-fm?include_prereleases)](https://github.com/formikadesk/rada-fm/releases)
[![crates.io](https://img.shields.io/crates/v/rada?include_prereleases)](https://crates.io/crates/rada)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

![rada in the panels view on the Neon theme: look around, select three items, copy them with a plan first, settle the name conflict with Keep both, see the card, take it back with Ctrl+Z](docs/media/demo-panels.gif)

rada is written in Rust and driven by keyboard or mouse. Vim keys and the usual desktop shortcuts work together, the interface is in English and Italian, and it looks the same in any modern terminal.

![The panels view on the Neon theme: NAV and the ring of disk space on the left, the folder in the middle with a selection, the picture under the cursor and its details on the right](docs/screenshots/panels-neon.png)

## Why rada

- **Plan before action.** Copy, move, rename, trash and delete show what will happen first: files, bytes, symlinks, permissions, name conflicts. Nothing runs until you confirm.
- **Journal and undo.** Every finished operation is recorded and can be undone, even after a restart. A file you changed afterwards is left alone, not overwritten.
- **Recovery after a crash.** The journal is written as it goes. If rada is killed mid-copy, the next start removes the half-written file, keeps both copies of a cut move, and tells you. Tested with `SIGKILL`, full disks and property tests that check copy → undo gives back the starting state.
- **Signed updates.** `rada update` checks a [minisign](https://jedisct1.github.io/minisign/) signature made with an offline key before it installs anything, and can roll back. Signed releases start with 0.6.0-rc.9.
- **Phones and USB sticks.** On Linux, phones, cameras and unmounted sticks appear in the sidebar; you can mount, copy to and eject them, and the plan says what a phone cannot keep.
- **Fits the window.** Three boxes, two, or the classic layout, chosen from the size of the window.
- **Linux, macOS, Windows.** Linux is supported; macOS and Windows are a pre-release (Trash and Recycle Bin, Finder tags, Quick Look, drive ejection) and still have rough edges.

| ![The plan window](docs/screenshots/piano.png) | ![The window of an undo](docs/screenshots/annulla.png) |
|---|---|
| Every operation shows a **plan** first. | **Undo** says what comes back or goes away, line by line. |

rada is young and written by one person. Read [what is guaranteed and what is not](docs/SAFETY.md), keep backups of what matters, and report anything strange.

## Layouts

rada chooses the layout from the size of the window (`layout = "auto"`, the default).

- **Panels.** From 140×36 cells, NAV, the folder and the preview are three boxes. From 70×24 there are two, and `Alt+P` swaps NAV and the preview.
- **Classic.** A sidebar and a list. Smaller windows get it by themselves; `layout = "classic"` or the View menu keeps it at any size.

| ![Panels on the Neon theme: three boxes, the disk ring and a real picture in the preview](docs/screenshots/panels-neon.png) | ![Panels on the Gesso theme, the light one](docs/screenshots/principale-pannelli-gesso.png) |
|---|---|
| Panels, Neon theme: the ring is the disk space, the preview draws the photo. | Panels, light theme. |
| ![Panels in a window of 78×45 cells: two boxes](docs/screenshots/principale-due-riquadri.png) | ![The classic layout on the default theme: sidebar, list with the cursor on a photo, and its preview](docs/screenshots/classic.png) |
| Two boxes in a narrower window. | Classic: sidebar, list and preview, default theme. |

## Install

Prebuilt binaries (Linux x86_64 and aarch64, static; macOS Intel and Apple silicon; Windows x86_64) are on the [releases page](https://github.com/formikadesk/rada-fm/releases). Unpack the archive and put `rada` (`rada.exe`) on your `PATH`; commands for each system are in [docs/install.md](docs/install.md).

With Rust 1.93 or newer ([rustup](https://rustup.rs)), from crates.io:

```sh
cargo install rada --locked --version 0.6.0-rc.17
```

Only pre-releases are published so far, and cargo does not pick a pre-release by itself, so the version has to be named. With 0.6.0 `cargo install rada --locked` will be enough. More in [docs/install.md](docs/install.md).

Or from source:

```sh
git clone https://github.com/formikadesk/rada-fm rada
cd rada
cargo install --path crates/rada
```

Arch Linux (AUR): coming soon. A [Nerd Font](https://www.nerdfonts.com/) is recommended for file-type icons (`rada setup font` installs one); without it rada uses Unicode markers.

## Quick start

Run `rada`, `rada ~/Pictures`, or `rada photo.png`. Press `?` for every key; both key schemes work at once.

| Do this | Vim | Classic |
|---|---|---|
| Move · open · parent folder | `j` `k` · `l` · `h` | arrows · `Enter` · `Backspace` |
| Select | `Space` | `Space` |
| Copy · cut · paste (a plan comes first) | `y` `x` `p` | `Ctrl+C` `Ctrl+X` `Ctrl+V` |
| Move to trash | `d` | `Del` |
| Rename | `r` | `F2` |
| Undo · history of operations | `u` · `U` | `Ctrl+Z` · `F3` |
| Jump to a path, search, run a command | `m` | `Ctrl+P` |
| Filter this folder | `/` | `Ctrl+F` |
| New tab · close tab | `Ctrl+T` · `Ctrl+W` | same |
| Context menu | right click | `Shift+F10` |
| Help · quit | `?` · `q` | `F1` · `Ctrl+Q` |

## A closer look

<details>
<summary>More screenshots</summary>

| ![Light theme (Gesso)](docs/screenshots/principale-gesso.png) | ![Italian interface](docs/screenshots/principale-scuro.png) |
|---|---|
| The light **Carta** theme. | The same screen in Italian: the language follows the system, or `language = "it"` / `"en"`. |
| ![Icons view](docs/screenshots/icone.png) | ![Columns view](docs/screenshots/colonne.png) |
| **Icons** view. | **Columns** view, as in the Finder. |
| ![Context menu](docs/screenshots/menu.png) | ![Open with](docs/screenshots/apri-con.png) |
| The right-click menu shows each key. | **Open with…** lists the programs that can open the file. |
| ![Tags sidebar](docs/screenshots/tag-blu.png) | ![Notification cards](docs/screenshots/notifiche.png) |
| Tags, and everything that carries one. | Cards say what was done, with **Undo** and **Show**. |
| ![Progress](docs/screenshots/avanzamento.png) | ![Permanent deletion](docs/screenshots/elimina.png) |
| Progress counted in bytes. | Permanent deletion asks for a typed word. |
| ![Tag menu](docs/screenshots/tag-menu.png) | ![Tag menu on the light theme](docs/screenshots/tag-menu-gesso.png) |
| The Tag submenu, dark. | The same on Carta. |
| ![Basic profile](docs/screenshots/basic.png) | ![Narrow terminal](docs/screenshots/stretta.png) |
| The basic profile (`TERM=vt100`): ASCII, 16 colours. | A narrow terminal: the sidebar becomes icons. |
| ![Catppuccin](docs/screenshots/tema-catppuccin.png) | ![Tokyo Night](docs/screenshots/tema-tokyo-night.png) |
| Catppuccin theme. | Tokyo Night theme. |
| ![Recent](docs/screenshots/recenti.png) | ![Recent on the light theme](docs/screenshots/recenti-gesso.png) |
| **Recent**, from the system's own list. | The same on Carta. |
| ![Error over a done card](docs/screenshots/notifiche-gesso.png) | ![Help](docs/screenshots/aiuto.png) |
| An error stays until you close it. | The help lists vim and classic keys side by side. |

![Giving a file a tag from the menu, with its plan, and the list of everything that carries it](docs/media/tag.gif)

</details>

## Verify downloads

Releases from 0.6.0-rc.9 on are signed with [minisign](https://jedisct1.github.io/minisign/) by a key kept offline; earlier ones are not. The two public keys, the same ones built into `rada update`:

| | public key |
|---|---|
| main | `RWTqJn1Q6BACL1Uum9OJEEM2ZyPMK3StBCnXcQXli1oYz0lZ19+S0OoQ` |
| reserve | `RWQ39NbBsAIJ5OJx/sP8k6/3iYAsNmaXG1ZASMIfQVXZkTcI2rw+BrCF` |

How to check a download by hand: [docs/verify.md](docs/verify.md).

## Documentation

- [Features](docs/features.md): everything rada does, in full
- [Using rada](docs/usage.md): first steps, command line, all keys, mouse, requests as JSON
- [The interface](docs/interface.md): tabs, bar, folder views, panes, menus, notifications
- [Configuration](docs/configuration.md): every setting of `config.toml`
- [Themes](docs/themes.md) and [terminals](docs/terminals.md): colours, icons, images, profiles
- [Archives](docs/archives.md), [tags](docs/tags.md), [phones, cameras and sticks](docs/devices.md)
- [Updates](docs/updates.md), [installing](docs/install.md), [desktop integration](docs/desktop.md) ([what works on each system](docs/platforms.md#system-integration))
- [Linux, macOS and Windows](docs/platforms.md): what works on each
- [Safety: what is guaranteed and what is not](docs/SAFETY.md)
- [Verifying downloads](docs/verify.md) and [how a release is made](docs/release.md)
- [Development](docs/development.md), [roadmap](docs/roadmap.md), [CONTRIBUTING.md](CONTRIBUTING.md), [CHANGELOG.md](CHANGELOG.md)

## License

MIT © Alessandro Formica. See [LICENSE](LICENSE).

## Support rada

rada is free and open source, and I want to keep it that way. If it saves you time and you would like to help me keep working on it, you can support it on [Ko-fi](https://ko-fi.com/formikadesk). Bug reports, reviews of the operations engine and a star are just as welcome.
