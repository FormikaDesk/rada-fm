# The AUR package

`rada/PKGBUILD` builds rada from the tag of a release (the source is GitHub's archive of the
tag, checked with a SHA-256); `rada/.SRCINFO` is what `makepkg --printsrcinfo` says about it.
**Nothing here is published**: the AUR is a git repository of its own, and pushing to it is
done by hand (below).

## The version

A `pkgver` cannot hold a hyphen, so the tag `v0.6.0-rc.16` becomes `0.6.0rc16`: the suffix
without its hyphen and its dot. That is the form pacman orders correctly, as `vercmp` shows:

```sh
vercmp 0.6.0rc16 0.6.0      # -1: the release candidate comes before the release
vercmp 0.6.0rc9 0.6.0rc10   # -1
vercmp 0.6.0_rc16 0.6.0     #  1: with an underscore it would come after it (wrong)
```

The PKGBUILD keeps the tag as `_tag` (for the download and for the folder GitHub makes) and
the Arch form as `pkgver`. The release itself is plain: `v0.6.0` is `0.6.0`.

## A new release

```sh
scripts/aur-bump v0.6.0-rc.17    # a pre-release
scripts/aur-bump v0.6.0          # the release
```

It sets `pkgver`, `_tag`, `pkgrel=1`, the SHA-256 of the archive and `.SRCINFO`. The tag has to
be pushed first: the script downloads what GitHub makes for it. A change to the PKGBUILD alone
(same tag) raises `pkgrel` by hand and regenerates `.SRCINFO` (`makepkg --printsrcinfo >
.SRCINFO`).

## What the PKGBUILD does

- `prepare()`: `cargo fetch --locked`, the only step that uses the network.
- `build()`: `cargo build --frozen --release -p rada`, in `$srcdir`.
- `check()`: the unit tests of the libraries (`cargo test --frozen --release --workspace
  --lib`) and that `rada --version` says the version of the tag. The whole suite is the
  project's CI.
- `package()`: only under `$pkgdir`: the program, the MIT `LICENSE` in
  `/usr/share/licenses/rada/`, the README, the `.desktop` file and the icons.
- Cargo keeps what it downloads in its own folder (`~/.cargo`), as it does for every Rust
  package of the AUR; that is a cache, not part of the package.

## Trying it

Build the package in a folder of its own (outside the repository), without installing it:

```sh
tmp=$(mktemp -d) && cp packaging/aur/rada/PKGBUILD "$tmp" && cd "$tmp"
makepkg --verifysource                      # the archive is the one the checksum names
CARGO_BUILD_JOBS=2 makepkg                  # no -i, no -s: nothing goes into the system
tar -tf rada-*.pkg.tar.zst                  # what it holds
tar -xf rada-*.pkg.tar.zst usr/bin/rada && usr/bin/rada --version
namcap PKGBUILD rada-*.pkg.tar.zst          # needs `sudo pacman -S namcap`
```

`makepkg` without `-s` needs `cargo` (or `rustup` with a toolchain) installed; with
`RUSTUP_TOOLCHAIN=stable` set by the PKGBUILD, a rustup user needs the `stable` toolchain.

## Publishing (by hand)

1. Make an account on <https://aur.archlinux.org/register> and add the **public** SSH key
   (`~/.ssh/id_ed25519.pub`, made with `ssh-keygen -t ed25519` if you have none) in *My
   Account*.
2. Take the name and make the repository (the name `rada` was free when this was written;
   `rada-bin` too):
   ```sh
   git clone ssh://aur@aur.archlinux.org/rada.git /path/outside/rada-aur
   ```
   Git warns that you cloned an empty repository: that is normal, and is how the AUR reserves
   the name.
3. Copy `PKGBUILD` and `.SRCINFO` in, commit and push (the branch must be `master`):
   ```sh
   cp packaging/aur/rada/PKGBUILD packaging/aur/rada/.SRCINFO /path/outside/rada-aur/
   cd /path/outside/rada-aur && git add PKGBUILD .SRCINFO
   git commit -m "rada 0.6.0rc16" && git push origin HEAD:master
   ```
4. In the repository of rada, replace "Arch Linux (AUR): coming soon" in `README.md` and the
   "Not published yet" paragraph of `docs/install.md` with `yay -S rada` (or `paru -S rada`).
   Do it after the push, not before: nobody can install it until then.

For each later release: `scripts/aur-bump <tag>`, copy the two files, commit
(`git commit -m "rada <pkgver>"`), push.
