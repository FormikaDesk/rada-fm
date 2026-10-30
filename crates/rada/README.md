# rada

A safe harbor for your files: a terminal file manager where every operation shows its plan
before it runs, every change is written to a journal that can take it back, and copying
checks what it wrote.

![rada in the panels view on the Neon theme: look around, select three items, copy them with a plan first, settle the name conflict with Keep both, see the card, take it back with Ctrl+Z](https://raw.githubusercontent.com/formikadesk/rada-fm/main/docs/media/demo-panels.gif)

![The panels view on the Neon theme: NAV and the ring of disk space on the left, the folder in the middle with a selection, the picture under the cursor and its details on the right](https://raw.githubusercontent.com/formikadesk/rada-fm/main/docs/screenshots/panels-neon.png)

| ![The plan window](https://raw.githubusercontent.com/formikadesk/rada-fm/main/docs/screenshots/piano.png) | ![The window of an undo](https://raw.githubusercontent.com/formikadesk/rada-fm/main/docs/screenshots/annulla.png) |
|---|---|
| Every operation shows a **plan** first. | **Undo** says what comes back or goes away, line by line. |

## Install

Binaries are on the [releases page](https://github.com/formikadesk/rada-fm/releases); each system below downloads one and puts it in a folder of your own, with no administrator rights. [docs/install.md](https://github.com/formikadesk/rada-fm/blob/main/docs/install.md) has everything else: which file is yours, checking the signature, the `PATH`, the font and the launcher, updating and uninstalling.

**Linux** (x86_64 and aarch64, static):

```sh
v=0.6.0-rc.20; n="rada-$v-$(uname -m)-unknown-linux-musl"
curl -fLO "https://github.com/formikadesk/rada-fm/releases/download/v$v/$n.tar.gz"
tar xzf "$n.tar.gz" && mkdir -p ~/.local/bin && install -m 755 "$n/rada" ~/.local/bin/rada
rada --version      # not found? add ~/.local/bin to your PATH: docs/install.md#linux
```

**macOS** (Apple silicon; for an Intel Mac, `x86_64-apple-darwin`):

```sh
v=0.6.0-rc.20; n="rada-$v-aarch64-apple-darwin"
curl -fLO "https://github.com/formikadesk/rada-fm/releases/download/v$v/$n.tar.gz"
tar xzf "$n.tar.gz" && mkdir -p ~/.local/bin && install -m 755 "$n/rada" ~/.local/bin/rada
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc    # then open a new terminal; rada --version
```

**Windows** (x86_64, PowerShell; [Windows Terminal](https://aka.ms/terminal) recommended):

```powershell
$v = "0.6.0-rc.20"; $n = "rada-$v-x86_64-pc-windows-msvc"; $d = "$env:LOCALAPPDATA\Programs\rada"
Invoke-WebRequest "https://github.com/formikadesk/rada-fm/releases/download/v$v/$n.zip" -OutFile "$env:TEMP\$n.zip"
Expand-Archive -Force "$env:TEMP\$n.zip" "$env:TEMP\rada-unpacked"; New-Item -ItemType Directory -Force $d | Out-Null; Copy-Item -Force "$env:TEMP\rada-unpacked\$n\rada.exe" $d
[Environment]::SetEnvironmentVariable("Path", ([Environment]::GetEnvironmentVariable("Path", "User") + ";$d"), "User")   # then open a new terminal; rada --version
```

**Arch Linux:** AUR coming soon; until then use the binary or the `PKGBUILD` in the repository ([how](https://github.com/formikadesk/rada-fm/blob/main/docs/install.md#arch-linux-aur)).

**With cargo** (Rust 1.93 or newer, [rustup](https://rustup.rs)), from crates.io or from a checkout:

```sh
cargo install rada --locked --version 0.6.0-rc.20      # only pre-releases so far: the version has to be named
cargo install --path crates/rada                        # from a clone of this repository
```

Check a download against its signature first: [docs/verify.md](https://github.com/formikadesk/rada-fm/blob/main/docs/verify.md). A [Nerd Font](https://www.nerdfonts.com/) is recommended for file-type icons (`rada setup font` installs one); without it rada uses Unicode markers.

## More

- [Features](https://github.com/formikadesk/rada-fm/blob/main/docs/features.md) and [using rada](https://github.com/formikadesk/rada-fm/blob/main/docs/usage.md)
- [What is guaranteed and what is not](https://github.com/formikadesk/rada-fm/blob/main/docs/SAFETY.md)
- [Installing](https://github.com/formikadesk/rada-fm/blob/main/docs/install.md) and [updates](https://github.com/formikadesk/rada-fm/blob/main/docs/updates.md)
- [Source and issues](https://github.com/formikadesk/rada-fm)

This crate is the program. The engine is [`rada-core`](https://crates.io/crates/rada-core) and
the interface is [`rada-tui`](https://crates.io/crates/rada-tui); they are released together
with it and have no stable API of their own.

MIT licence.
