# Updates

rada looks for a newer version **at most once a day**, in a background worker that never slows the start. It reads GitHub's public list of releases and sends nothing but `User-Agent: rada/<version>`; a network error is silent. A stable version only sees stable releases; a pre-release (`0.6.0-rc.17`) also sees later pre-releases (`channel = "stable"` or `"prerelease"` decides by hand, `check = false` turns the background check off).

When one exists you see it twice: a pill in the accent colour in the tab bar, **↑ Update to X**, and the status bar, **New version X available — press Alt+U to update** (click either). `U` is the history of operations in the vim keys, so the update has `Alt+U`. A window shows the release notes first; `Enter` updates, `Esc` leaves it for later.

The status bar always shows the version that is running, **v0.6.0-rc.17 ⟳**: a click asks the network right away (whatever the day's check said) and answers *rada is up to date* or opens the notes. The **Help ▾** button of the tab bar is a menu with the keys, *Check for updates* and *About rada* (the version, the channel it follows, how it would be updated and where the program is). From the command line:

```sh
rada update --check     # only say whether a newer version exists, and what is in it
rada update             # show the notes, ask, update
rada update --rollback  # go back to the version that was running before
```

How it updates depends on how this copy got here (`method = "auto"`, or choose):

| | |
|---|---|
| **binary** — downloaded from the releases | downloads the archive for this system and processor (Intel or Apple silicon on a Mac; under Rosetta, the native one), checks the minisign signature of the release's `SHA256SUMS` (against two public keys built into the program; the signed comment must name this release, so an old signed release cannot be shown again) and the archive against it, starts the new program once with `--version`, then replaces the running one atomically (on Windows the running file is renamed out of the way first). The old one is kept for `--rollback`. A file rada writes has no macOS quarantine flag; rada checks and clears it anyway. |
| **source** — built from a git checkout (`source_dir`, or the checkout the program sits in) | in that folder: stops with an explanation unless the working tree is clean and the branch is `main`; `git pull --ff-only`; `cargo build --release --locked`; puts the new program in place of the running one; `cargo clean`. Progress shows in the status bar, the interface stays usable, and if a step fails the old program is untouched and the log (`$XDG_STATE_HOME/rada/update/last.log`) can be opened from the window. |
| **package** — installed by a package manager (a system path, the AUR, Scoop, Homebrew) or with `cargo install rada` (cargo's `.crates.toml` next to the `bin` folder lists `rada` from crates.io) | touches nothing and says which tool to use (`cargo install rada --locked` for cargo, with `--version <new version>` added while the new version is a pre-release, which cargo does not pick by itself). A program put in `~/.cargo/bin` by `cargo install --path` (from a checkout) or copied there is not this one: it is updated as before. |

An update that is not signed by the author is refused, and nothing is installed; the old program and the way back (`--rollback`) stay as they were. A *source* update trusts the git repository you chose, and a *package* the package manager: neither goes through the signature.
