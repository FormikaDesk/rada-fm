# Desktop integration

Everything is installed for your user, nothing needs administrator rights, and nothing is done unless you ask. `rada setup desktop --remove` takes away exactly what was installed.

```sh
rada setup desktop            # launcher, icon and (where it applies) the window's own identity
rada setup desktop --desktop  # macOS and Windows: also a launcher on the Desktop
rada setup desktop --default-file-manager  # Linux, Windows: rada opens the folders other programs ask for
rada setup font               # download and install JetBrainsMono Nerd Font Mono for this user
rada setup file-picker        # Linux: make rada the file picker of the desktop (see the end of this page)
```

**Linux.** `setup desktop` writes `rada.desktop` to `~/.local/share/applications` and the icon to `~/.local/share/icons/hicolor` (SVG, and PNG at 48, 128 and 256 px), updates the desktop database when `update-desktop-database` is available, and prints each file it wrote. The entry declares `StartupWMClass=com.formikadesk.rada`, and `rada --spawn-terminal` (what the entry runs) starts the terminal with that identity — `ghostty --class=`, `kitty --class`, `alacritty --class`, `wezterm start --class`, `foot --app-id` — so a bar or workspace switcher shows **rada's icon** and not the terminal's. The entry does not claim folders, so a plain setup never changes what opens them; with `--default-file-manager` it declares `inode/directory` and uses `xdg-mime` to set rada as the handler (the previous handler is remembered and `--remove` puts it back). `--spawn-terminal` chooses, in this order, `xdg-terminal-exec`, `$TERMINAL`, then the first of ghostty, kitty, foot, alacritty, wezterm, konsole, gnome-terminal, xterm. The files for packagers are in `packaging/linux/`.

**macOS.** `setup desktop` makes `~/Applications/Rada.app`, built from files only (no code to sign): an `Info.plist`, rada's icon as a real `.icns` at every size, and a small launcher that opens rada in **Ghostty** if it is installed — in a 180×45 window with its own settings, which hand the Cmd keys back to rada (see [macOS](platforms.md#macos)) — else in **iTerm2**, else in **Terminal**. `--desktop` adds a link on the Desktop. **Known limits:** while rada runs, the Dock shows the terminal's icon, not rada's: a window belongs to the program that draws it.

*"Allow Ghostty to execute …?"* is Ghostty's own question. Its macOS application asks it for every file it is asked to open **as a document** (a double click in the Finder, a drop on the Dock, or a path among its arguments, which is how `ghostty -e /path/to/rada` reaches it, and then it opens a second tab for that "document" besides the one that runs the command), and for a command given with `-e`. Its author keeps the question on purpose (a security measure, [discussion #10203](https://github.com/ghostty-org/ghostty/discussions/10203)); there is no setting that switches it off, and rada does not try to get round it. What can be done is to never give Ghostty rada as a document, and `Rada.app` does just that: its launcher passes options only (`open -n -a Ghostty --args --config-file=…`) and the program is the `initial-command` of the settings file of its window, which Ghostty reads as configuration and not as a document: **one window, one tab, no question**. The same file has `window-save-state = never`, so that the windows of the last run are not given back beside it (macOS does that unless told not to). It also has `window-padding-color = background` and `window-padding-balance = true`: the margin Ghostty keeps around the grid takes the terminal's background, which rada sets to its theme's while it runs ([Themes](themes.md)), and the room left over is shared on every side, so the window has no frame of another colour. If your Rada.app was made by an older version of rada (the first ones started Ghostty with `-e`, which is the two tabs and the question), make it again: `rada setup desktop` replaces the whole bundle. `cat ~/Applications/Rada.app/Contents/MacOS/Rada` must show a line `open -n -a Ghostty --args --config-file=…` and nothing else after it. Opening rada from inside a Ghostty window you already have never asks. **If rada ends with an error** in a window that `Rada.app` opened, the message stays on screen until you press a key (the launchers set `RADA_FROM_APP=1`; an `Rada.app` made by an older version needs `rada setup desktop` again for this). The same error is in the log, `~/Library/Application Support/rada/log/rada.log.YYYY-MM-DD`, in a line marked `ERROR`.

**Windows.** `setup desktop` puts a shortcut with rada's icon in the Start menu (`--desktop`: on the Desktop too). If Windows Terminal is installed it adds a profile "rada" as a *fragment* (`%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\rada\`, the way Windows Terminal asks programs to add profiles, without editing your own settings) with the icon and — once the font is installed — the Nerd Font, and the shortcut opens that profile; otherwise the shortcut starts `rada.exe` itself, whose icon is built into the program, so the taskbar shows it.

**Font.** Icons need a [Nerd Font](https://www.nerdfonts.com/) in the terminal. `rada setup font` downloads **JetBrainsMono Nerd Font Mono** (Nerd Fonts v3.4.0, the four faces and the licence; every file is checked against a SHA-256 written in rada's source, and nothing is installed unless all match) and installs it for your account: `~/.local/share/fonts` and `fc-cache` on Linux, `~/Library/Fonts` on macOS, your own fonts folder and the per-user list in the registry on Windows. Then choose "JetBrainsMono Nerd Font Mono" in your terminal's font setting. The font is under the SIL Open Font License 1.1; the licence is installed beside it. At the first start where the icons would not draw, rada **offers** to do this — once, and only if you say yes. In **Ghostty** nothing is needed: its bundled symbols fallback draws the icons, and rada detects it and does not offer.

## Windows: rada as the program that opens folders

`rada setup desktop --default-file-manager` makes rada the answer when a program asks Windows to open a folder (the Run box, "Open containing folder" in many programs, a folder shortcut), as Total Commander and Directory Opus can be set up to be. It does not touch Explorer.

```sh
rada setup desktop --default-file-manager        # shows what it writes, asks, then writes
rada setup desktop --default-file-manager --yes  # without the question
rada setup desktop --remove                      # puts everything back (the shortcuts too)
```

What it writes, **only in your own registry** (`HKEY_CURRENT_USER\Software\Classes`; never `HKEY_LOCAL_MACHINE`, never administrator rights):

| Key | Value |
|---|---|
| `Directory\shell` | `(Default)` = `open` |
| `Directory\shell\open\command` | `(Default)` = `"…\rada.exe" "%1"` |
| `Directory\shell\open` | `DelegateExecute` taken away, **only if** another replacement left one there (it would win over the command) |

It reads each key first and writes down what was there (a value, or that the key was not), in `windows-folder-handler.json` of the state folder. `--remove` puts the values back as they were, takes away the keys it made (deepest first, and only when nothing else is in them) and leaves alone a value that was changed after the setup. If rada cannot run, by hand: `reg delete "HKCU\Software\Classes\Directory\shell\open" /f`, and set the `(Default)` of `Directory\shell` back if you had one.

What it leaves to Explorer, on purpose: the class `Folder` (This PC, the Control Panel and the other virtual folders, which a terminal program cannot show), `Drive`, and `opennewwindow` (*Open in new window* in a folder's right-click menu: there is always a way to Explorer). `rada.exe` is a console program: Windows opens it in a console window, or in Windows Terminal if that is your default terminal.

**Not tried on a real Windows yet.** The keys are the ones other file managers use, and the logic (what is read, written, undone, left alone) is tested against a registry in memory; what Windows does with them has not been seen. If it behaves badly, `--remove` is the way back.

## Use rada as the file picker

On Linux, programs ask the desktop for a file through **xdg-desktop-portal** ("upload a file" in a browser, "Open…" in a GTK or Qt program). **xdg-desktop-portal-termfilechooser** lets a terminal file manager answer: the portal starts a script, the script opens rada in a terminal window, and what you choose in it goes back to the program. It is what yazi and ranger do. Nothing here needs administrator rights, and rada installs no package.

1. **Install the portal.** On Arch Linux, from the AUR: `yay -S xdg-desktop-portal-termfilechooser-hunkyburrito-git` (the maintained fork, [source](https://github.com/hunkyburrito/xdg-desktop-portal-termfilechooser)). Elsewhere, build it from that repository. Nothing else is needed: rada brings its own script.
2. **Let rada write the settings.**

   ```sh
   rada setup file-picker
   ```

   It shows the three files it would write and where, with the new lines marked `+`, and asks. It writes only after you say yes (`--yes` skips the question). A file that is already there is copied next to itself (`config.rada-backup`) first, and your own lines stay.

   | File | What it gets |
   |---|---|
   | `~/.config/xdg-desktop-portal-termfilechooser/rada-wrapper.sh` | the script the portal runs (it is [`crates/rada/assets/linux/rada-wrapper.sh`](https://github.com/formikadesk/rada-fm/blob/main/crates/rada/assets/linux/rada-wrapper.sh) of the source) |
   | `~/.config/xdg-desktop-portal-termfilechooser/config` | `cmd=` that script, `create_help_file=0` (so that a name that does not exist yet can be saved) and `env=RADA_BIN=` the rada that ran the setup |
   | `~/.config/xdg-desktop-portal/<desktop>-portals.conf` | `org.freedesktop.impl.portal.FileChooser=termfilechooser` in `[preferred]`; `<desktop>` is the first name in `$XDG_CURRENT_DESKTOP`, in lower case (`hyprland-portals.conf`). If you have none of your own, it starts from the one the system ships, so the other portals keep their backends. |

   `rada setup file-picker --remove` puts everything back: the copies return, and the files rada made are taken away (a file you changed since is left alone).
3. **Restart the portals** so that they read it: `systemctl --user restart xdg-desktop-portal`.
4. **Firefox and Zen** use their own window unless told otherwise: in `about:config` set `widget.use-xdg-desktop-portal.file-picker` to `1`. For GTK programs, `GTK_USE_PORTAL=1` makes them ask the portal; other browsers have their own setting, if they do not use it already.
5. **Try it.** In the browser, choose *Upload* on any page, or from a terminal: `rada --chooser-file /tmp/answer ~; cat /tmp/answer`.

**The terminal.** The script opens rada in `$TERMCMD` if set (the terminal and its options, as the portal's other wrappers read it: `TERMCMD='foot --title "choose a file"'`; put it in `env=TERMCMD=…` of the portal's `config` for the portal to see it), else in `$TERMINAL`, else in the first of ghostty, kitty, foot, alacritty, wezterm, konsole, gnome-terminal and xterm that is installed. `TERMCMD` (and `$TERMINAL`) are read with `eval`, because they are a command line with its own quotes, as the portal's other wrappers read them: they are your own configuration, like the script, and nothing a program asks the portal for (the folder to start in, the name to suggest) is ever part of that command line. The window must stay in front until rada ends: for Ghostty the script passes `--gtk-single-instance=false`, so that it is a window of its own and not a tab of the one that is open. The window is named `com.formikadesk.rada.chooser`, so that a window rule on that class can make it float.

**What is asked.** The bar at the top says it: *Choose a file*, *Choose files* (`Space` marks, `Enter` chooses what is marked), *Choose a folder* (`Enter` takes the folder under the cursor, `Alt+Enter` the one you are in) or *Save as* (`Ctrl+S` or `Alt+Enter` asks for the name, with the one the program suggested filled in; a name that exists asks twice before it is replaced). `Esc` cancels and writes nothing. The list starts with the most recently changed first (the usual keys change the order). From 70 columns the preview of what is under the cursor is beside the list (the navigation pane keeps only its icons); on a narrower window `Alt+P` shows it in the place of the list and again hides it. Nothing can be deleted, moved or renamed in this mode. The command line behind it is in [usage.md](usage.md#as-a-file-chooser).

If a window does not appear, run `journalctl --user -eu xdg-desktop-portal-termfilechooser`, and try the script by hand: `~/.config/xdg-desktop-portal-termfilechooser/rada-wrapper.sh 0 0 0 "$HOME" /tmp/answer 1`.

