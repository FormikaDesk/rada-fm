# Installing

## Binaries

Binaries for Linux (static, x86_64 and aarch64), Windows (x86_64) and macOS (Intel and Apple silicon) are on the [releases page](https://github.com/formikadesk/rada-fm/releases), with a `SHA256SUMS` file and its signature, `SHA256SUMS.minisig`. Unpack the archive and put `rada` (`rada.exe`) somewhere on your `PATH` (on a Mac, `~/.local/bin` or `/usr/local/bin` will do; no Homebrew is needed). Once it is installed, rada can [update itself](updates.md).

Windows and macOS builds are new: please report anything odd in an issue.

Linux and macOS, for example (take the file name for your system from the releases page; `x86_64-unknown-linux-musl` is the static Linux build):

```sh
curl -LO https://github.com/formikadesk/rada-fm/releases/download/v0.6.0-rc.19/rada-0.6.0-rc.19-x86_64-unknown-linux-musl.tar.gz
tar xzf rada-0.6.0-rc.19-x86_64-unknown-linux-musl.tar.gz
install -m 755 rada-0.6.0-rc.19-x86_64-unknown-linux-musl/rada ~/.local/bin/rada
```

Check the signature first when the release has one: [verify.md](verify.md).

## Arch Linux (AUR)

Not published yet. Until it is, use the binary or build from source.

## With cargo (crates.io)

This builds rada from crates.io (it needs a Rust toolchain of version 1.93 or newer, [rustup](https://rustup.rs)) and puts it in `~/.cargo/bin`:

```sh
cargo install rada --locked --version 0.6.0-rc.19
```

`--locked` builds with the exact versions of the dependencies rada was tested with. While only pre-releases have been published, cargo does not pick one by itself, so the version has to be named; with 0.6.0 `cargo install rada --locked` will be enough. A copy installed this way is not updated by `rada update` (it does not replace a file cargo looks after): it says to run `cargo install rada --locked` again, which is also how you update it (while the newer version is a pre-release, the command it gives names it: `cargo install rada --locked --version 0.6.0-rc.19`). This is not signed by the author's key like the binaries; cargo checks the download against the checksum of the crates.io index.

## From source

You need a Rust toolchain of version 1.93 or newer ([rustup](https://rustup.rs)).

```sh
git clone https://github.com/formikadesk/rada-fm rada
cd rada
cargo install --path crates/rada
```

Or just build it: `cargo build --release` and use `target/release/rada`.

A [Nerd Font](https://www.nerdfonts.com/) in your terminal is recommended: rada then shows file-type icons. It looks for the font in the configuration of Ghostty, kitty, WezTerm, Alacritty, foot, Windows Terminal and Konsole; otherwise it uses Unicode markers, chosen for what the fonts of the system can draw (`--icons nerd|unicode-icons|unicode-std|none` overrides; see [Terminals](terminals.md)).

Optional: make your shell follow rada's last folder (`v` instead of `rada`):

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

A block is the two lines `# >>> rada >>>` and `# <<< rada <<<` around the call. `--yes` skips the question; run again and it says it is installed and writes nothing; `rada setup shell --remove` takes away exactly that file or block. Then open a new terminal and use `v` instead of `rada`. To do it by hand, `rada --init <shell>` prints the function: `eval "$(rada --init bash)"`.

Optional: add rada to your application menu (Linux), the Applications folder (macOS) or the Start menu (Windows) — see [Desktop integration](desktop.md) — and install a font for the icons with `rada setup font`.

## On a Mac

- **Use [Ghostty](https://ghostty.org)** if you can: rada draws pictures with the kitty graphics protocol, the icons work with no font to install (Ghostty ships a Nerd Font symbols fallback, which rada detects), and Ghostty passes the Cmd key on (see [macOS](platforms.md#macos)). iTerm2, WezTerm, kitty and Terminal also work, with less.
- **Allow your terminal to see your folders.** macOS asks before a program may read the Desktop, Documents, Downloads, iCloud Drive or an external disk, once per terminal. If you said no — or a folder just shows "Operation not permitted" — rada says "macOS has not given *Ghostty* access to *Documents*" and a button opens *System Settings → Privacy & Security → Files & Folders* for you; switch the terminal on there, or give it *Full Disk Access* to cover everything. Then quit the terminal and start it again.
- `rada setup desktop` makes **Rada.app** in `~/Applications`: a click opens rada in Ghostty (iTerm2 or Terminal if that is not installed). See [Desktop integration](desktop.md).
