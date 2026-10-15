# Roadmap

### Next

- **Fidelity and robustness**, continuing: more ways to break the engine on purpose (other filesystems, network shares, power-loss simulation), file flags and capabilities, and fewer limits in [docs/SAFETY.md](SAFETY.md).
- **Windows and macOS**, from pre-release to supported: ACLs and alternate data streams on Windows, extended attributes and resource forks on macOS, junction handling, "Put Back" metadata in the Finder's trash, and people who use them every day.
- **Easy install**: signed binaries and packages for the common distributions.
- **Dual pane**, with tabs and layout restore.
- **Archives**, continuing: passwords (zip and 7z), creating 7z, keeping owners and extended attributes in the archives you make.

### Ideas, not planned yet

- **Space analysis**: find what takes up room and suggest cleanups, with an optional assistant that runs locally, is off by default and only sees names and sizes — never contents. Every suggestion would still go through the usual plan and undo.
- **Remote filesystems** (SFTP) through the same engine.
- **Plugins**.
- Git integration, syntax-highlighted preview, recursive search, a trash browser, comparing and syncing two folders with a dry-run plan.
