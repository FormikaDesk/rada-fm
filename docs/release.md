# Making a release

This page is for the person who publishes rada. Anybody else can ignore it; how to *check* a
release is in [verify.md](verify.md).

## The idea

The release is built by GitHub (`.github/workflows/release.yml`) but **signed on the
maintainer's own computer**. The signing key is never on GitHub, so someone who takes over the
account, a token or the workflow can build a release but cannot publish one that rada will
install: the program refuses an update whose `SHA256SUMS` is not signed by one of the public keys
built into it.

```
push the tag  →  GitHub builds the binaries  →  a DRAFT release with SHA256SUMS
              →  scripts/sign-release.sh <tag> (on your computer, asks your passphrase)
              →  SHA256SUMS.minisig uploaded, release published
```

Nothing is published by the workflow. A release that stays a draft is invisible to everyone, and
`rada update` does not see it.

## Each release

1. Make `main` what you want to ship: the version in `Cargo.toml` (`[workspace.package]`), a
   `## [x.y.z] - date` section in `CHANGELOG.md` (the notes of the release are taken from it),
   the tests green.
2. Tag it and push the tag. The tag must be `v` + the version in `Cargo.toml`; the workflow stops
   if they differ. Only tags that start with `v` and a digit start a release.

   ```sh
   git tag v0.6.0
   git push origin v0.6.0
   ```
3. Wait for the workflow *Release* to finish. It builds five targets and creates a **draft** with the
   archives and `SHA256SUMS` (a pre-release when the version has a hyphen, as in `-rc.1`).
   Look at the draft on GitHub if you like: the notes, the files.
4. Sign and publish, from a clone of the repository (so that `gh` knows which one), on the computer
   that has the key:

   ```sh
   scripts/sign-release.sh v0.6.0
   ```

   It downloads the draft, checks every hash again, shows you what it signs, runs `rsign` (which
   asks for the passphrase of the key: the script never sees it), checks the signature it made,
   and, after you answer `y`, uploads `SHA256SUMS.minisig` and publishes. `--no-publish` stops before
   uploading; `--yes` does not ask. It refuses a release that is not a draft.
5. Check what was published: the release lists `SHA256SUMS.minisig` among its files
   (`gh release view <tag> --json assets --jq '.assets[].name'`), and a manual check as in
   [verify.md](verify.md) says `Signature and comment signature verified` with the trusted
   comment `rada <tag>`.
6. Try it: `rada update --check` from an older copy should see the new version, and `rada update`
   should install it. (The first signed release, `0.6.0-rc.9`, is reached from `rc.8`, which does
   not check signatures: that one step is covered by the hash only. From `rc.9` on, every update
   is checked.)

Needs: `gh` (logged in, with the right to publish releases of the repository), `rsign`
(`cargo install --locked rsign2`) and `sha256sum`.

`scripts/test-sign-release` tries the script against a stand-in for `gh` and a key made on the
spot; it touches nothing real.

## The keys

Two pairs of keys are made once, with `rsign`, in `~/.config/rada-release/` (folder `700`, private
keys `600`), and are never in the repository (`.gitignore` has `*.key`, `*.sec` and
`rada-release/`):

| file | what it is |
|---|---|
| `rada-main.key`, `rada-main.pub` | the key releases are signed with |
| `rada-reserve.key`, `rada-reserve.pub` | the reserve, kept apart and never used until the main key is lost |

```sh
rsign generate -p ~/.config/rada-release/rada-main.pub -s ~/.config/rada-release/rada-main.key
rsign generate -p ~/.config/rada-release/rada-reserve.pub -s ~/.config/rada-release/rada-reserve.key
```

Use a long passphrase for each, and keep a copy of the folder offline (a USB stick, and an
encrypted copy elsewhere). The `.pub` files hold the public keys: the second line of each (`RW…`) is
what goes in `crates/core/src/update/sign.rs` (`PUBLIC_KEYS`), in the README and in
[verify.md](verify.md). Write the public key somewhere else that GitHub does not control too (your
site, your profile), so that a changed key in the repository can be noticed.

### If the main key is lost or leaked

* **Lost, passphrase forgotten:** nothing signed can be made with it again. Sign the next release
  with the **reserve** key (`RADA_SIGN_KEY=~/.config/rada-release/rada-reserve.key
  scripts/sign-release.sh <tag>`). Copies of rada that are already installed accept it, since both
  keys are in their list. Then make a new reserve pair, put the new public key in `PUBLIC_KEYS`
  instead of the lost one, and release again.
* **Leaked:** the same, but also say so (an issue, the release notes): anything signed with the
  old key after the leak is not to be trusted. A copy of rada can only learn the new list from a
  release signed with a key it already trusts, so move quickly.
* **Both lost:** an installed rada can no longer be updated by itself; people have to download
  the new version by hand and check it as in [verify.md](verify.md).

### What the signature says

`SHA256SUMS.minisig` signs the text of `SHA256SUMS`, with the *trusted comment* `rada <tag>` (for
instance `rada v0.6.0`). rada accepts an update only if the signature is valid for one of its keys
**and** the comment is exactly `rada ` + the tag of the release it is installing. A genuine
signature of an old release therefore cannot be shown again as a new one. Only after that does it
believe the hash of the archive, and only after the hash does it unpack the archive and run the new
program once (`--version`).

## Trying the workflow without publishing

A push to a branch `release-dry-run/<name>` runs the whole workflow and creates a **draft** called
`dry-run-<commit>` (a draft has no tag). Delete the draft and the branch afterwards:

```sh
gh release delete dry-run-<commit> --yes
git push origin --delete release-dry-run/<name>
```

Running the workflow by hand (*Actions → Release → Run workflow*) builds and keeps the binaries
as artifacts of the run only.

## Not covered

Source builds (`method = "source"`: `git pull`, `cargo build`) trust the repository the person
chose, and packages (AUR, Homebrew, Scoop) trust their package manager; neither goes through
this signature. The binaries are not yet signed for the operating system (Apple Developer ID and
notarisation, Authenticode): macOS and Windows may warn about a downloaded program.
