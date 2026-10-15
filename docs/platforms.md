# Platforms

## Status

Version 0.6.0-rc.8. **Linux is supported.** Windows and macOS are a **pre-release**: the Recycle Bin / Trash, drives and volumes, the standard folders, opening files, and — new in this release — the things people expect of the Finder and Explorer (packages, localized names, ejecting, Quick Look, tags on macOS; localized names and ejecting on Windows) are implemented, and the whole test suite runs on all three systems in CI. The Mac version has had its first real-world try-out; nobody lives in the Windows one yet, so expect rough edges, and the parts of a copy that preserve attributes (ACLs, alternate data streams, resource forks, and the extended attributes other than tags) are not done there: the plan says so before anything is copied.

rada is young and written by one person. The parts that touch your files — planning, copying, the journal, undo — are covered by unit tests, fault-injection tests (full disk, revoked rights, files that change or vanish), tests that kill the process with `SIGKILL` in the middle of an operation, and property tests that generate random trees and check that copy → undo and move → undo give back exactly the starting state, and that compress → extract gives back the same tree. That is a lot more than nothing and much less than years of use. Read [what is guaranteed and what is not](SAFETY.md), keep backups of what matters, and please report anything strange. Review of the operations engine by other people is the help the project needs most ([CONTRIBUTING.md](../CONTRIBUTING.md)).

## macOS

What a Mac user expects of a file manager, as far as a terminal allows:

- **Permissions.** When macOS refuses a folder until you allow it, rada says so in words and opens the right page of System Settings (see [On a Mac](install.md#on-a-mac)); the sidebar marks the folders that are waiting.
- **Packages.** `.app`, `.bundle`, `.framework`, `.photoslibrary`, `.pages`, `.numbers`, `.key`, `.rtfd` and every folder the system treats as a package (asked of Launch Services, with the Finder's bundle bit as a second opinion — not guessed from the name) behave like files: `Enter` opens them with their program, they sort among the files, and the right-click menu has *Show package contents* (`Alt+↓`) to go inside.
- **Hidden like in the Finder.** `.DS_Store`, `._*` files, `.localized` and every folder flagged hidden (`~/Library`) stay out of sight until you show hidden files (`.`, or `Cmd+Shift+.`).
- **Names as the Finder shows them.** With the interface in Italian, `Desktop` is *Scrivania*, `Documents` *Documenti*, `Downloads` *Download*, `Pictures` *Immagini*, `Music` *Musica*, `Movies` *Filmati*, `Public` *Pubblica*, `Applications` *Applicazioni* and `Library` *Libreria*: in the sidebar, the list, the bar of the path and the tabs, from the system's own table of folder names (a short fixed one is the reserve), and only for the folders that carry the Finder's `.localized` marker, as in the Finder. *Macintosh HD* and the other disks come from the system too. In English the names stay as they are. Only what is shown changes: **Properties**, **Copy path**, search, copy, move, rename and the command line use the real name.
- **Recent** in the sidebar: the files you used lately, from Spotlight (see [Recent](interface.md#recent)).
- **Natural order** everywhere: `file2` before `file10`, capitals mixed with small letters (`natural_sort = false` for letter by letter).
- **Eject** in the right-click menu of a disk and with `Del` on it in the sidebar: a confirmation, then `diskutil eject`; a busy disk says who is using it ("Finder (process 412)…") and can be tried again. **iCloud Drive** is in the sidebar when it is there.
- **Quick Look**: *Quick Look* in the item's menu, a button under *Preview* in the details pane, `Alt+Y` (and `Cmd+Y`), and `Q` in the full-screen preview. `Space` still means *select*.
- **Previews without poppler**: PDFs, Office documents, Pages, Numbers, Keynote, RTF and videos (see [Features](features.md)).
- **Tags**: the Finder's own, see [Tags](tags.md).

**The Cmd key.** On a Mac, rada's classic scheme also answers `Cmd+C`, `Cmd+V`, `Cmd+X`, `Cmd+Backspace` (to the trash), `Cmd+I` (properties), `Cmd+O` (open), `Cmd+↑` (parent), `Cmd+[` and `Cmd+]` (back and forward), `Cmd+Shift+.` (hidden files), `Cmd+Z` / `Cmd+Shift+Z`, `Cmd+Y` (Quick Look), `Cmd+A` and `Cmd+Shift+N`. Two things must be true for that, and rada only controls the first:

1. *The terminal reports the Cmd key.* That is the kitty keyboard protocol: **Ghostty**, kitty and WezTerm (with `enable_kitty_keyboard = true`) and iTerm2 (with "Report keys using CSI u" on) do; **Terminal.app does not**. rada asks for it on a Mac (`cmd_keys = false` stops that) and shows `Cmd+…` in its hints only when the terminal answered.
2. *The terminal does not keep the key for itself.* Ghostty binds `Cmd+C`, `Cmd+V`, `Cmd+Z`, `Cmd+Shift+Z`, `Cmd+A`, `Cmd+Backspace`, `Cmd+↑`, `Cmd+[` and `Cmd+]` (checked against its source; the others rada uses are free). In the window `Rada.app` opens, a settings file frees them with `keybind = super+c=unbind` and so on. In **your own** Ghostty window those keys still belong to Ghostty until you add the same lines to your config (`~/Library/Application Support/com.mitchellh.ghostty/config`; the list is in `Rada.app/Contents/Resources/rada-ghostty.conf`). The Ctrl keys work everywhere.

Things that do **not** work: `Cmd+Q`, `Cmd+W`, `Cmd+T`, `Cmd+N` and the rest of the terminal's own shortcuts stay the terminal's; the Dock shows the terminal's icon; Finder-style drag and drop and "Put Back" from the trash are not there; ACLs, resource forks and extended attributes other than tags are not carried over by a copy.

## Windows

- **Names as Explorer shows them.** The sidebar and the breadcrumbs use the shell's display names for the Known Folders and the drives ("Documenti", "Disco locale (C:)"), not the names on disk; the real path stays in Properties and *Copy path*.
- **Eject** for removable drives (the right-click menu, `Del` in the sidebar): rada locks the volume — it fails with "a program still has a file open" if one does —, dismounts it and asks the drive to release the media, all without administrator rights. Fixed disks and network drives have no such entry.
- **PDF previews** use Windows' own renderer (`Windows.Data.Pdf`, Windows 10 and later) and other documents the shell's thumbnails, when Windows has a handler for the type. No program to install. The choice of the system renderer over a bundled one is explained in the [changelog](../CHANGELOG.md).
- **Not supported**: tags (the menu entry is absent), ACLs and alternate data streams in copies.
