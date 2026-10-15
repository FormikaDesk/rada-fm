# Archives

`Enter` on an archive opens it like a folder, read-only. What counts as an archive is decided by the **content**, not the name: a zip called `backup.dat` opens, a `.docx` (which is a zip inside) opens in your word processor. Supported to read and extract:

| Format | How |
|---|---|
| zip, tar, `.tar.gz` `.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`, and single files `.gz` `.bz2` `.xz` `.zst` | built in, the same on every platform (zstd uses the bundled official library, everything else is pure Rust) |
| 7z | built in (not password-protected ones) |
| RAR | through `7z`/`7zz` or `unrar`, if installed (the licence does not allow including a RAR decoder); without one rada says so and how to install it |

Inside an archive you navigate, filter and sort as in any folder; the breadcrumb reads `archive.zip › folder`. Selecting an archive (without opening it) shows its format, how many files and folders, packed and unpacked size, and the first items. Text and pictures inside are previewed (taken out into the preview cache, with size limits). Rename, move, delete and new folder are refused in an archive; **copy** items out (`Ctrl+C`, then `Ctrl+V` in a folder) and that is a partial extraction with its own plan.

| Action | Vim | Classic | Menu |
|---|---|---|---|
| extract here | `e` | `Ctrl+E` | right-click an archive → *Extract here* |
| extract into a folder named after the archive (you can change the name) | `E` | `Alt+E` | *Extract to a folder…* |
| compress the selection (name and format; `Tab` changes the format) | `z` | `Alt+Z` | *Compress…* |

The folder is named from the archive's **file name** only (`photos.tar.gz` → `photos/`; a dot in a folder above never matters). If the archive already has a single folder at its top, no second one is made around it. The plan shows files, folders, links, total size, free space, name conflicts (skip / keep both / overwrite, with the old file going to the trash so undo brings it back) and the safety findings below, each with its reason.

- **Paths that would leave the destination** (`..`, absolute, drive letters, a `\` that climbs out) are not extracted, and the plan counts them. Nothing is written through a link that the archive itself made.
- **Links** are created as links and never followed; if one points outside the extracted folder the plan says so.
- **Bombs**: an extraction above 8 GiB or above 200× its packed size asks for a typed `yes`. Whatever the list says, a member can never write more than the archive declared for it. Limits are in `[archives]`.
- **Password-protected** zip and 7z are recognised and said so; passwords are not supported yet.
- **Old zip names** (code page 437) are decoded correctly; names that are not text are kept as they are.
- Extraction writes every file under a temporary name and renames it when complete, counts progress in real bytes, can be cancelled (what was made is removed) and undone (exactly what was extracted; a file you edited is left alone). A `SIGKILL` in the middle leaves no half file and the next start cleans up. Setuid/setgid bits are dropped from extracted files.
- **Compress** makes zip, `.tar.gz`, `.tar.zst` or `.tar.xz` with contents, permissions, times and symlinks (zip stores them as links; the plan warns that some programs, Windows Explorer for one, extract those as small files). The estimated size comes with the plan; cancelling removes the half-written archive, undo removes the finished one.

```toml
[archives]
max_extract_gb = 8      # an extraction that would write more asks for a typed "yes"
max_ratio = 200         # ...and so does one that unpacks this many times its size
preview_entries = 200   # items listed in the preview of an archive
preview_seconds = 1.5   # how long counting an archive's members may take in the preview
```

What is guaranteed, and what is not (RAR tested with 7-Zip and a stand-in, no owner or ACLs in archives you create…), is in [docs/SAFETY.md](SAFETY.md).
