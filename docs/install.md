# Installing

Pick your system: [Linux](#linux), [Arch Linux](#arch-linux-aur), [macOS](#macos), [Windows](#windows), [with cargo](#with-cargo-cratesio) or [from source](#from-source). Each section is a list of commands to copy, in order; nothing needs administrator rights except the Arch package. Afterwards, [set up the font, the shell and the launcher](#after-installing), and know how to [update](#updating) and [uninstall](#uninstalling).

Windows and macOS builds are new: please report anything odd in an issue.

## Which file do I need?

Every release on the [releases page](https://github.com/formikadesk/rada-fm/releases) has one archive per system and processor, a `SHA256SUMS` file and its signature, `SHA256SUMS.minisig`.

| System | Processor | The file |
|---|---|---|
| Linux | Intel or AMD (`x86_64`) | `rada-0.6.0-rc.20-x86_64-unknown-linux-musl.tar.gz` |
| Linux | ARM (`aarch64`) | `rada-0.6.0-rc.20-aarch64-unknown-linux-musl.tar.gz` |
| macOS | Apple silicon (M1 and later) | `rada-0.6.0-rc.20-aarch64-apple-darwin.tar.gz` |
| macOS | Intel | `rada-0.6.0-rc.20-x86_64-apple-darwin.tar.gz` |
| Windows | 64-bit Intel or AMD (`x86_64`) | `rada-0.6.0-rc.20-x86_64-pc-windows-msvc.zip` |

Which processor do you have? On Linux and on a Mac, ask the terminal:

```sh
uname -m
```

It prints `x86_64` (Intel or AMD), `aarch64` (Linux on ARM) or `arm64` (a Mac with Apple silicon). If a Mac prints `x86_64` but is an Apple silicon one, the terminal runs under Rosetta: `sysctl -n sysctl.proc_translated` then prints `1`, and the file you want is the Apple silicon one. On Windows there is only one build, for `x86_64`; there is none for Windows on ARM.

The Linux builds are static (musl): they run on every distribution, with nothing else to install. Inside each archive is a folder with the same name as the archive (without `.tar.gz` or `.zip`), and in it the program, `rada` (`rada.exe` on Windows), with `README.md`, `LICENSE` and `CHANGELOG.md`.

## Linux

The same commands serve both processors: `uname -m` gives the first half of the file name. They are for bash and zsh; for fish, which writes variables differently, see [Using fish](#using-fish-linux).

```sh
rv=0.6.0-rc.20
target="$(uname -m)-unknown-linux-musl"
name="rada-$rv-$target"
base="https://github.com/formikadesk/rada-fm/releases/download/v$rv"

cd "$(mktemp -d)"
curl -fLO "$base/$name.tar.gz"
curl -fLO "$base/SHA256SUMS"
curl -fLO "$base/SHA256SUMS.minisig"
```

**Check the download.** The signature tells you the files are the ones the author published (more in [verify.md](verify.md)). You need [minisign](https://jedisct1.github.io/minisign/), which most distributions have (`pacman -S minisign`, `apt install minisign`, `dnf install minisign`):

```sh
minisign -Vm SHA256SUMS -P RWTqJn1Q6BACL1Uum9OJEEM2ZyPMK3StBCnXcQXli1oYz0lZ19+S0OoQ
sha256sum --check --ignore-missing SHA256SUMS
```

The first command must print `Signature and comment signature verified` and `Trusted comment: rada v0.6.0-rc.20`; the second must say `OK` for your archive. If either fails, stop and read [what to do](verify.md#if-a-signature-does-not-verify).

**Unpack and install** into `~/.local/bin`, a folder that belongs to you:

```sh
tar xzf "$name.tar.gz"
mkdir -p ~/.local/bin
install -m 755 "$name/rada" ~/.local/bin/rada
```

**Is `~/.local/bin` on your `PATH`?** Run `echo $PATH`. If `.local/bin` is not in it, add it for your shell (once), then open a new terminal:

```sh
# bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
# zsh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
# fish
fish_add_path ~/.local/bin
```

**Check:**

```sh
rada --version      # rada 0.6.0-rc.20
```

If it says `command not found`, the folder is not on the `PATH` of this terminal yet: open a new one, or run `~/.local/bin/rada --version` to see that the file itself works. Once it is installed, rada can [update itself](updates.md).

### Using fish (Linux)

fish sets variables with `set`, not `name=value`, and keeps the name `version` for its own version, so these commands call the variable `rv`. The commands to check the download above (`minisign`, `sha256sum`) are the same in fish.

```fish
set rv 0.6.0-rc.20
set target (uname -m)-unknown-linux-musl
set name rada-$rv-$target
set base https://github.com/formikadesk/rada-fm/releases/download/v$rv

cd (mktemp -d)
curl -fLO $base/$name.tar.gz
curl -fLO $base/SHA256SUMS
curl -fLO $base/SHA256SUMS.minisig
```

Then, after checking the download, unpack and install into `~/.local/bin` and put that folder on the `PATH` (once; fish remembers it):

```fish
tar xzf $name.tar.gz
mkdir -p ~/.local/bin
install -m 755 $name/rada ~/.local/bin/rada
fish_add_path ~/.local/bin
```

## Arch Linux (AUR)

Not published yet: the AUR package is coming soon. Until it is, use the [binary above](#linux) or build the package from the `PKGBUILD` in the repository, which makes a normal pacman package (`rada` can then be removed with `pacman -R rada`):

```sh
sudo pacman -S --needed base-devel     # makepkg and the tools it uses
mkdir ~/rada-pkg && cd ~/rada-pkg
curl -LO https://raw.githubusercontent.com/formikadesk/rada-fm/main/packaging/aur/rada/PKGBUILD
less PKGBUILD                          # short; look at it before you build
makepkg -si
```

`makepkg -si` downloads the source of the release named in `pkgver` (checked against the SHA-256 written in the file), installs `cargo` if it is missing, builds rada, runs the unit tests of its libraries (a few minutes in all) and installs the package, asking for your password where it needs it. If you use `rustup`, have the `stable` toolchain installed (`rustup toolchain install stable`). The package also installs the launcher and the icons, so `rada setup desktop` is not needed. How it is made: [packaging/aur/README.md](../packaging/aur/README.md).

## macOS

Apple silicon and Intel, with the Terminal. No Homebrew is needed. Take the file for your Mac from [the table](#which-file-do-i-need): the first line of the commands below is the only one that changes. They are for zsh and bash; for fish, see [Using fish](#using-fish-macos).

```sh
target=aarch64-apple-darwin          # Apple silicon; for an Intel Mac: x86_64-apple-darwin
rv=0.6.0-rc.20
name="rada-$rv-$target"
base="https://github.com/formikadesk/rada-fm/releases/download/v$rv"

cd "$(mktemp -d)"
curl -fLO "$base/$name.tar.gz"
curl -fLO "$base/SHA256SUMS"
curl -fLO "$base/SHA256SUMS.minisig"
```

**Check the download** ([verify.md](verify.md)); you need `minisign`, for example `brew install minisign` if you have Homebrew:

```sh
minisign -Vm SHA256SUMS -P RWTqJn1Q6BACL1Uum9OJEEM2ZyPMK3StBCnXcQXli1oYz0lZ19+S0OoQ
shasum -a 256 --check --ignore-missing SHA256SUMS
```

The first command must print `Signature and comment signature verified` and `Trusted comment: rada v0.6.0-rc.20`; the second must say `OK` for your archive.

**Unpack and install** into `~/.local/bin`:

```sh
tar xzf "$name.tar.gz"
mkdir -p ~/.local/bin
install -m 755 "$name/rada" ~/.local/bin/rada
```

**Put `~/.local/bin` on your `PATH`.** The shell of a Mac is zsh; once, then open a new terminal window:

```sh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
```

(For bash on a Mac the file is `~/.bash_profile`; for fish, `fish_add_path ~/.local/bin`.)

**Check:**

```sh
rada --version      # rada 0.6.0-rc.20
```

**If macOS refuses to open it.** A file that `curl` downloaded is not marked and opens at once. A file that a **browser** downloaded (or an archive you opened with a double click in the Finder) carries macOS's quarantine flag, `com.apple.quarantine`, and the first run may end in *"rada" cannot be opened because the developer cannot be verified* (the binaries are not notarized by Apple). Take the flag off the program, which is what rada itself does to the files it writes when it updates:

```sh
xattr -d com.apple.quarantine ~/.local/bin/rada
```

`xattr ~/.local/bin/rada` lists the flags a file carries; if `com.apple.quarantine` is not among them, the flag was not the problem. Instead of the command you can also open *System Settings → Privacy & Security*, scroll to the message about `rada` and choose *Open Anyway*. Once rada is installed, [it updates itself](updates.md).

### Using fish (macOS)

fish sets variables with `set`, not `name=value`, and keeps the name `version` for its own version, so these commands call the variable `rv`. The commands to check the download above (`minisign`, `shasum`) are the same in fish.

```fish
set target aarch64-apple-darwin          # Apple silicon; for an Intel Mac: x86_64-apple-darwin
set rv 0.6.0-rc.20
set name rada-$rv-$target
set base https://github.com/formikadesk/rada-fm/releases/download/v$rv

cd (mktemp -d)
curl -fLO $base/$name.tar.gz
curl -fLO $base/SHA256SUMS
curl -fLO $base/SHA256SUMS.minisig
```

Then, after checking the download, unpack and install into `~/.local/bin` and put that folder on the `PATH` (once; fish remembers it):

```fish
tar xzf $name.tar.gz
mkdir -p ~/.local/bin
install -m 755 $name/rada ~/.local/bin/rada
fish_add_path ~/.local/bin
```

### On a Mac

- **Use [Ghostty](https://ghostty.org)** if you can: rada draws pictures with the kitty graphics protocol, the icons work with no font to install (Ghostty ships a Nerd Font symbols fallback, which rada detects), and Ghostty passes the Cmd key on (see [macOS](platforms.md#macos)). iTerm2, WezTerm, kitty and Terminal also work, with less.
- **Allow your terminal to see your folders.** macOS asks before a program may read the Desktop, Documents, Downloads, iCloud Drive or an external disk, once per terminal. If you said no — or a folder just shows "Operation not permitted" — rada says "macOS has not given *Ghostty* access to *Documents*" and a button opens *System Settings → Privacy & Security → Files & Folders* for you; switch the terminal on there, or give it *Full Disk Access* to cover everything. Then quit the terminal and start it again.
- `rada setup desktop` makes **Rada.app** in `~/Applications`: a click opens rada in Ghostty (iTerm2 or Terminal if that is not installed). See [Desktop integration](desktop.md).

## Windows

Windows 10 or 11, 64-bit Intel or AMD, with PowerShell (the one that comes with Windows is enough; nothing here needs administrator rights). rada is happiest in [Windows Terminal](https://aka.ms/terminal), which is part of Windows 11 and an app of the Microsoft Store on Windows 10 (`winget install --id Microsoft.WindowsTerminal`): it draws pictures and shows the Nerd Font. The old console window works, with less ([Terminals](terminals.md)).

**Download:**

```powershell
$ProgressPreference = "SilentlyContinue"     # makes Invoke-WebRequest much faster
$rv = "0.6.0-rc.20"
$name = "rada-$rv-x86_64-pc-windows-msvc"
$base = "https://github.com/formikadesk/rada-fm/releases/download/v$rv"

Set-Location $env:TEMP
Invoke-WebRequest "$base/$name.zip" -OutFile "$name.zip"
Invoke-WebRequest "$base/SHA256SUMS" -OutFile SHA256SUMS
Invoke-WebRequest "$base/SHA256SUMS.minisig" -OutFile SHA256SUMS.minisig
```

**Check the download.** With `minisign` installed, the commands of [verify.md](verify.md) work in PowerShell too: `minisign -Vm SHA256SUMS -P RWTqJn1Q6BACL1Uum9OJEEM2ZyPMK3StBCnXcQXli1oYz0lZ19+S0OoQ` must print `Signature and comment signature verified` and `Trusted comment: rada v0.6.0-rc.20`. Without it, you can at least see that the file is not damaged: the two lines below must show the same hash (this does not prove who made the file).

```powershell
(Get-FileHash "$name.zip" -Algorithm SHA256).Hash.ToLower()
Select-String -Path SHA256SUMS -Pattern "$name.zip"
```

**Unpack and install** into a folder of your own, `%LOCALAPPDATA%\Programs\rada`:

```powershell
$dest = "$env:LOCALAPPDATA\Programs\rada"
New-Item -ItemType Directory -Force $dest | Out-Null
Expand-Archive -Force "$name.zip" -DestinationPath "$env:TEMP\rada-unpacked"
Copy-Item -Force "$env:TEMP\rada-unpacked\$name\rada.exe" $dest
```

**Add the folder to your `PATH`** (the one of your user account, not the machine's):

```powershell
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$parts = @($userPath -split ";" | Where-Object { $_ })
if ($parts -notcontains $dest) {
    [Environment]::SetEnvironmentVariable("Path", (($parts + $dest) -join ";"), "User")
}
```

**Close the terminal and open a new one** (a terminal reads the `PATH` when it starts), then:

```powershell
rada --version      # rada 0.6.0-rc.20
```

**SmartScreen.** rada.exe is not signed with a certificate from Microsoft's list, so Windows may say *"Windows protected your PC"* the first time you run a file that came from a browser. Click *More info*, then *Run anyway*; or take the mark of the download off before the first run (the file `Invoke-WebRequest` saved has none, so with the commands above this is not needed):

```powershell
Unblock-File "$dest\rada.exe"
```

An antivirus that quarantines the file is a different matter: the [signature](verify.md) is how you know it is the one the author published, and an issue on GitHub is the way to tell us. Once rada is installed, [it updates itself](updates.md); the shortcut in the Start menu comes from `rada setup desktop`.

## With cargo (crates.io)

This builds rada from crates.io (it needs a Rust toolchain of version 1.93 or newer, [rustup](https://rustup.rs)) and puts it in `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows, which rustup adds to the `PATH`):

```sh
cargo install rada --locked --version 0.6.0-rc.20
```

`--locked` builds with the exact versions of the dependencies rada was tested with. While only pre-releases have been published, cargo does not pick one by itself, so the version has to be named; with 0.6.0 `cargo install rada --locked` will be enough. A copy installed this way is not updated by `rada update` (it does not replace a file cargo looks after): it says to run `cargo install rada --locked` again, which is also how you update it (while the newer version is a pre-release, the command it gives names it: `cargo install rada --locked --version 0.6.0-rc.20`). This is not signed by the author's key like the binaries; cargo checks the download against the checksum of the crates.io index.

## From source

You need a Rust toolchain of version 1.93 or newer ([rustup](https://rustup.rs)).

```sh
git clone https://github.com/formikadesk/rada-fm rada
cd rada
cargo install --path crates/rada
```

Or just build it: `cargo build --release` and use `target/release/rada`.

## After installing

All of this is optional. Each step is for your user only, needs no administrator rights and has a `--remove` that takes away exactly what it put.

**Icons: a font.** A [Nerd Font](https://www.nerdfonts.com/) in your terminal is recommended: rada then shows file-type icons. It looks for the font in the configuration of Ghostty, kitty, WezTerm, Alacritty, foot, Windows Terminal and Konsole; otherwise it uses Unicode markers, chosen for what the fonts of the system can draw (`--icons nerd|unicode-icons|unicode-std|none` overrides; see [Terminals](terminals.md)). In Ghostty nothing is needed. Elsewhere:

```sh
rada setup font
```

It downloads JetBrainsMono Nerd Font Mono (every file is checked against a SHA-256 written in rada's source, and nothing is installed unless all match) and installs it for your account: `~/.local/share/fonts` and `fc-cache` on Linux, `~/Library/Fonts` on a Mac, your own fonts folder and the per-user list of fonts in the registry on Windows. Then choose "JetBrainsMono Nerd Font Mono" in the font setting of your terminal. At the first start where the icons would not draw, rada offers to do this once.

**Shell: `v` instead of `rada`.** Make your shell follow rada's last folder:

```sh
rada setup shell                  # shows what it would write and where, then asks
```

It finds your shell in `$SHELL` (`--shell bash|zsh|fish|nushell|powershell` to choose; PowerShell on Windows) and writes, for this user only:

| Shell | Where | What |
|---|---|---|
| fish | `~/.config/fish/conf.d/rada.fish` | a file of its own: `rada --init fish \| source` |
| bash | `~/.bashrc` (`~/.bash_profile` on a Mac) | a block with `eval "$(rada --init bash)"` |
| zsh | `$ZDOTDIR/.zshrc`, else `~/.zshrc` | a block with `eval "$(rada --init zsh)"` |
| PowerShell | `~/.config/powershell/Microsoft.PowerShell_profile.ps1` (Windows: `Documents\PowerShell\…`) | a block with `rada --init powershell \| Out-String \| Invoke-Expression` |
| nushell | `vendor/autoload/rada.nu` in nushell's data folder | a file of its own with the function (nushell cannot `source` the output of a command) |

A block is the two lines `# >>> rada >>>` and `# <<< rada <<<` around the call. `--yes` skips the question; run again and it says it is installed and writes nothing. Then open a new terminal and use `v` instead of `rada`. To do it by hand, `rada --init <shell>` prints the function: `eval "$(rada --init bash)"`. On Windows it is for PowerShell and nushell; `cmd.exe` has no way to change its own folder from a program.

**Launcher: the application menu.** What it makes ([Desktop integration](desktop.md) has the details):

```sh
rada setup desktop                # then, optionally: --desktop, --default-file-manager
```

| System | What `rada setup desktop` makes |
|---|---|
| Linux | `rada.desktop` in `~/.local/share/applications` and the icon in `~/.local/share/icons/hicolor`, so rada is in the application menu with its own icon. `--default-file-manager` makes it the program that opens folders. |
| macOS | `~/Applications/Rada.app`, a launcher that opens rada in Ghostty (iTerm2 or Terminal if that is not installed). `--desktop` adds a link on the Desktop. |
| Windows | A shortcut in the Start menu (`--desktop`: on the Desktop too), and a profile "rada" in Windows Terminal when it is installed. `--default-file-manager` makes rada the answer when a program asks Windows to open a folder (it shows the registry keys, in your own `HKEY_CURRENT_USER`, and asks first). |

On Linux, `rada setup file-picker` can also make rada the file picker of the desktop ([how](desktop.md#use-rada-as-the-file-picker)).

**First start.** Run `rada` (or `rada ~/Pictures`, or `rada photo.png`) and press `?` for every key; both key schemes work at once ([Using rada](usage.md)). There is nothing to configure first: the configuration file is optional ([Configuration](configuration.md)). `rada doctor` says what rada found out about your terminal and which profile it uses, which is the first thing to run if something looks wrong.

## Updating

```sh
rada update --check     # only say whether a newer version exists, and what is in it
rada update             # show the release notes, ask, update
rada update --rollback  # go back to the version that was running before
```

For a copy downloaded from the releases, `rada update` fetches the archive for your system, checks the signature and installs it in place of the running program; the old one is kept for `--rollback`. rada also looks for a new version once a day and shows it in the tab bar (`Alt+U` updates). Details, and what happens with the other ways to install, in [Updates](updates.md).

A copy installed with cargo is updated with cargo: `cargo install rada --locked` (while the newer version is a pre-release, with `--version <the new version>` added: `rada update` prints the exact command). On Arch, build the package again (`makepkg -si`) with the new `PKGBUILD`.

## Uninstalling

**1. Take away what `setup` installed**, while rada is still there. Each command removes exactly what its twin put:

```sh
rada setup shell --remove
rada setup desktop --remove       # also puts back the handler of folders, if you set it
rada setup font --remove
rada setup file-picker --remove   # Linux only
```

(`rada setup shell --remove --shell fish` if the shell you use now is not the one you set up.)

**2. Remove the program.**

```sh
# Linux and macOS, installed as above
rm ~/.local/bin/rada
# installed with cargo
cargo uninstall rada
# Arch package
sudo pacman -R rada
```

```powershell
# Windows, installed as above
$dest = "$env:LOCALAPPDATA\Programs\rada"
Remove-Item -Recurse $dest
$parts = @([Environment]::GetEnvironmentVariable("Path", "User") -split ";" | Where-Object { $_ -and $_ -ne $dest })
[Environment]::SetEnvironmentVariable("Path", ($parts -join ";"), "User")
```

The line you added to `~/.bashrc`, `~/.zshrc` or `~/.bash_profile` for the `PATH` can go too.

**3. Optional: rada's own folders.** rada keeps a configuration file (only if you wrote one) and a state folder: the **history of operations that undo reads** (`journal.jsonl`), what it remembers of the interface between runs, the update's files (including the copy `--rollback` goes back to), the log, and the cache of PDF previews. Removing the state folder means there is nothing left to undo, so do it when you no longer need that.

| System | Configuration | State and log | Cache |
|---|---|---|---|
| Linux | `~/.config/rada` | `~/.local/state/rada` | `~/.cache/rada` |
| macOS | `~/Library/Application Support/rada` | `~/Library/Application Support/rada` (the same folder) | `~/Library/Caches/rada` |
| Windows | `%APPDATA%\rada` | `%LOCALAPPDATA%\rada` | `%LOCALAPPDATA%\rada` (the same folder) |

If `XDG_CONFIG_HOME`, `XDG_STATE_HOME` or `XDG_CACHE_HOME` are set to an absolute path, rada uses that on every system instead of the default in the table. The files you moved to the trash with rada are in the system's trash and stay there.
