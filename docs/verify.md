# Verifying a download

Every release of rada is signed. The checksums in `SHA256SUMS` tell you a download is not
damaged; the **signature** tells you the files are the ones the author published, even if the
account on GitHub were taken over. rada itself checks the signature before it installs an update
(`rada update`); this page is how to check by hand a file you downloaded from the
[releases page](https://github.com/formikadesk/rada-fm/releases).

## The public keys

There are two, made together before the first signed release. Releases are signed with the
main key; the reserve key is kept apart and is only used if the main one is lost.

| | public key |
|---|---|
| main | `PLACEHOLDER-main-key-not-made-yet` |
| reserve | `PLACEHOLDER-reserve-key-not-made-yet` |

The same two keys are built into the program (`crates/core/src/update/sign.rs`) and written in
the README. If they are not the same everywhere, do not trust the files.

## Checking by hand

You need [minisign](https://jedisct1.github.io/minisign/) (`minisign` in most package
managers; `rsign2` from `cargo install rsign2` reads the same format). Put the archive,
`SHA256SUMS` and `SHA256SUMS.minisig` in one folder, then:

```sh
minisign -Vm SHA256SUMS -P <the main public key>
sha256sum --check --ignore-missing SHA256SUMS      # macOS: shasum -a 256 --check --ignore-missing SHA256SUMS
```

The first command must say `Signature and comment signature verified`, and print
`Trusted comment: rada v<version>` with the version you meant to install: a genuine signature of an
older release is not a signature of this one. The second command must say `OK` for your
archive. If either fails, do not run the program.

On Windows, use the same `minisign` commands in a terminal, and `certutil -hashfile
<archive> SHA256` to compare the hash with the line in `SHA256SUMS`.

## If a signature does not verify

* Try the other key (the reserve) before you worry: it is only used after the main one is lost.
* Download again, from the releases page, not from a mirror.
* If it still fails, please open an issue. Say which file, and paste the output of the command.
  Do not run the program.

## What the signature does not tell you

It says the author signed that `SHA256SUMS`; it does not say the code is free of mistakes. It
does not cover a copy you built from source yourself, or one a package manager made.
