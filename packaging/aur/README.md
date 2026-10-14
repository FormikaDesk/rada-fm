# The AUR package

`rada/PKGBUILD` builds rada from the tag of a release (the source is GitHub's archive of the
tag, checked with a SHA-256); `rada/.SRCINFO` is what `makepkg --printsrcinfo` says about it.
**Nothing here is published**: the AUR is a git repository of its own, and pushing to it is
done by hand (below).

## A new release

```sh
scripts/aur-bump v0.6.0-rc.8     # pkgver, _tag, sha256sums and .SRCINFO, from the tag's archive
```

The tag has to be pushed first: the script downloads what GitHub makes for it.

## Trying it

Build the package in a folder of its own (outside the repository), without installing it:

```sh
tmp=$(mktemp -d) && cp packaging/aur/rada/PKGBUILD "$tmp" && cd "$tmp"
CARGO_BUILD_JOBS=2 makepkg                  # no -i, no -s: nothing goes into the system
tar -tf rada-*.pkg.tar.zst                  # what it holds
tar -xf rada-*.pkg.tar.zst usr/bin/rada && usr/bin/rada --version
```

## Publishing

1. Make an account on <https://aur.archlinux.org/register> and add the **public** SSH key
   (`~/.ssh/id_ed25519.pub`) in *My Account*.
2. Take the name and make the repository:
   ```sh
   git clone ssh://aur@aur.archlinux.org/rada.git /path/outside/rada-aur
   ```
   (an empty repository: that is how the AUR reserves the name).
3. Copy `PKGBUILD` and `.SRCINFO` in, commit and push (the branch must be `master`):
   ```sh
   cp packaging/aur/rada/PKGBUILD packaging/aur/rada/.SRCINFO /path/outside/rada-aur/
   cd /path/outside/rada-aur && git add PKGBUILD .SRCINFO
   git commit -m "rada 0.6.0rc7" && git push origin HEAD:master
   ```
4. Then add `yay -S rada` to the README (see the TODO comment there).
