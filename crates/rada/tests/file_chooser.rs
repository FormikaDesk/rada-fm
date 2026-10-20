//! rada as the system's file chooser, from the outside: the options of the command line, the
//! script the portal runs (with a terminal and a rada that are stand-ins) and
//! `rada setup file-picker` on a home folder made for the test. Nothing here starts a real
//! terminal, touches a real folder or reads the real portal configuration.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct World {
    root: tempfile::TempDir,
}

impl World {
    fn new() -> World {
        let root = tempfile::Builder::new()
            .prefix("rada-chooser-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        for d in ["home", "config", "data", "state", "cache", "share", "bin"] {
            fs::create_dir_all(root.path().join(d)).unwrap();
        }
        World { root }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    fn rada_cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_rada"));
        c.args(args)
            .env("LC_ALL", "en_US.UTF-8")
            .env("HOME", self.p("home"))
            .env("USERPROFILE", self.p("home"))
            .env("XDG_CONFIG_HOME", self.p("config"))
            .env("XDG_DATA_HOME", self.p("data"))
            .env("XDG_STATE_HOME", self.p("state"))
            .env("XDG_CACHE_HOME", self.p("cache"))
            .env("XDG_DATA_DIRS", self.p("share"))
            .env("XDG_CURRENT_DESKTOP", "Hyprland")
            .stdin(Stdio::null());
        c
    }

    fn rada(&self, args: &[&str]) -> Output {
        self.rada_cmd(args).output().unwrap()
    }

    /// `rada setup file-picker …` with `answer` typed on its input.
    #[cfg(target_os = "linux")]
    fn setup(&self, args: &[&str], answer: &str) -> Output {
        use std::io::Write;
        let mut c = self.rada_cmd(args);
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(answer.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

// ------------------------------------------------------------------ the command line

#[test]
fn the_chooser_options_need_the_file_to_write_to() {
    let w = World::new();
    for args in [
        &["--chooser-multiple"][..],
        &["--chooser-dir"],
        &["--chooser-save=x"],
    ] {
        let out = w.rada(args);
        assert!(!out.status.success(), "{args:?}");
        assert!(text(&out).contains("--chooser-file"), "{}", text(&out));
    }
}

#[test]
fn saving_is_not_choosing_several_or_a_folder() {
    let w = World::new();
    let answer = w.p("answer");
    let a = answer.to_str().unwrap();
    for extra in ["--chooser-multiple", "--chooser-dir"] {
        let out = w.rada(&["--chooser-file", a, "--chooser-save=x", extra]);
        assert!(!out.status.success(), "{extra}: {}", text(&out));
    }
}

#[test]
fn a_chooser_is_not_an_operation_or_a_demo() {
    let w = World::new();
    let answer = w.p("answer");
    let out = w.rada(&["--chooser-file", answer.to_str().unwrap(), "--request", "-"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("--chooser-file cannot be used"),
        "{}",
        text(&out)
    );
    assert!(!answer.exists());
}

#[test]
fn the_help_describes_the_chooser_in_both_languages() {
    let w = World::new();
    let en = text(&w.rada(&["--help"]));
    for want in [
        "--chooser-file",
        "--chooser-multiple",
        "--chooser-dir",
        "--chooser-save",
    ] {
        assert!(en.contains(want), "{want}: {en}");
    }
    let mut c = w.rada_cmd(&["--help"]);
    c.env("LC_ALL", "it_IT.UTF-8").env("LANG", "it_IT.UTF-8");
    let it = String::from_utf8_lossy(&c.output().unwrap().stdout).into_owned();
    if cfg!(target_os = "linux") {
        assert!(it.contains("Fa da finestra di scelta file"), "{it}");
    }
    let setup = text(&w.rada(&["setup", "--help"]));
    assert!(setup.contains("file-picker"), "{setup}");
}

#[cfg(unix)]
#[test]
fn without_a_terminal_nothing_is_written() {
    // The interface needs a terminal; with none it stops, and the answer file is not made.
    let w = World::new();
    let answer = w.p("answer");
    let out = w.rada(&["--chooser-file", answer.to_str().unwrap(), "/"]);
    assert!(!out.status.success());
    assert!(!answer.exists());
}

// ------------------------------------------------------------------ rada setup file-picker

#[cfg(target_os = "linux")]
fn portal_files(w: &World) -> (PathBuf, PathBuf, PathBuf) {
    (
        w.p("config/xdg-desktop-portal-termfilechooser/rada-wrapper.sh"),
        w.p("config/xdg-desktop-portal-termfilechooser/config"),
        w.p("config/xdg-desktop-portal/hyprland-portals.conf"),
    )
}

#[cfg(target_os = "linux")]
#[test]
fn the_setup_shows_what_it_would_write_and_writes_nothing_without_a_yes() {
    let w = World::new();
    let (wrapper, config, portals) = portal_files(&w);
    for answer in ["", "n\n", "\n", "no\n"] {
        let out = w.setup(&["setup", "file-picker"], answer);
        assert!(out.status.success(), "{}", text(&out));
        let t = text(&out);
        assert!(t.contains("create "), "{t}");
        assert!(t.contains(&config.display().to_string()), "{t}");
        assert!(t.contains("+ create_help_file=0"), "{t}");
        assert!(t.contains("hyprland-portals.conf"), "{t}");
        assert!(t.contains("Write these files? [y/N]"), "{t}");
        assert!(t.contains("nothing was written"), "{t}");
        assert!(t.contains("Nothing needs administrator rights"), "{t}");
        // The portal is not installed in this world.
        assert!(
            t.contains("yay -S xdg-desktop-portal-termfilechooser-hunkyburrito-git"),
            "{t}"
        );
        for f in [&wrapper, &config, &portals] {
            assert!(!f.exists(), "{}", f.display());
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn the_setup_writes_after_a_yes_and_remove_goes_back() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    let (wrapper, config, portals) = portal_files(&w);
    // What the user already has.
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::create_dir_all(portals.parent().unwrap()).unwrap();
    let mine = "[filechooser]\ncmd=yazi-wrapper.sh\nenv=TERMCMD=foot\n";
    fs::write(&config, mine).unwrap();
    fs::write(&portals, "[preferred]\ndefault=gtk\n").unwrap();

    let out = w.setup(&["setup", "file-picker"], "y\n");
    assert!(out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("copy kept"), "{t}");
    assert!(
        t.contains("systemctl --user restart xdg-desktop-portal"),
        "{t}"
    );
    assert!(
        t.contains("widget.use-xdg-desktop-portal.file-picker"),
        "{t}"
    );
    let now = fs::read_to_string(&config).unwrap();
    assert!(now.contains(&format!("cmd={}", wrapper.display())), "{now}");
    assert!(
        now.contains("create_help_file=0") && now.contains("env=TERMCMD=foot"),
        "{now}"
    );
    assert!(now.contains("env=RADA_BIN="), "{now}");
    assert_eq!(
        fs::read_to_string(&portals).unwrap(),
        "[preferred]\ndefault=gtk\norg.freedesktop.impl.portal.FileChooser=termfilechooser\n"
    );
    assert!(wrapper.is_file());
    assert_ne!(
        fs::metadata(&wrapper).unwrap().permissions().mode() & 0o111,
        0
    );
    assert_eq!(
        fs::read_to_string(format!("{}.rada-backup", config.display())).unwrap(),
        mine
    );

    // A second time there is nothing to do, and nothing asked.
    let again = w.setup(&["setup", "file-picker"], "");
    assert!(
        text(&again).contains("already as it should be"),
        "{}",
        text(&again)
    );
    assert!(
        !text(&again).contains("Write these files?"),
        "{}",
        text(&again)
    );

    let out = w.setup(&["setup", "file-picker", "--remove"], "");
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("restored"), "{}", text(&out));
    assert_eq!(fs::read_to_string(&config).unwrap(), mine);
    assert_eq!(
        fs::read_to_string(&portals).unwrap(),
        "[preferred]\ndefault=gtk\n"
    );
    assert!(!wrapper.exists());
    assert!(!Path::new(&format!("{}.rada-backup", config.display())).exists());

    let none = w.setup(&["setup", "file-picker", "--remove"], "");
    assert!(text(&none).contains("nothing to remove"), "{}", text(&none));
}

#[cfg(target_os = "linux")]
#[test]
fn yes_writes_without_asking_and_the_systems_defaults_are_kept() {
    let w = World::new();
    let (wrapper, _, portals) = portal_files(&w);
    // What the system says for this desktop, found through XDG_DATA_DIRS.
    let sys = w.p("share/xdg-desktop-portal");
    fs::create_dir_all(sys.join("portals")).unwrap();
    fs::write(
        sys.join("hyprland-portals.conf"),
        "[preferred]\ndefault=hyprland;gtk",
    )
    .unwrap();
    fs::write(sys.join("portals/termfilechooser.portal"), "[portal]\n").unwrap();
    let out = w.setup(&["setup", "file-picker", "--yes"], "");
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(!t.contains("Write these files?"), "{t}");
    assert!(!t.contains("hunkyburrito"), "the portal is installed: {t}");
    assert_eq!(
        fs::read_to_string(&portals).unwrap(),
        "[preferred]\ndefault=hyprland;gtk\norg.freedesktop.impl.portal.FileChooser=termfilechooser\n"
    );
    assert!(wrapper.is_file());
    // Taking it away takes the file of ours away: the system's own is there again.
    let out = w.setup(&["setup", "file-picker", "--remove"], "");
    assert!(out.status.success(), "{}", text(&out));
    assert!(!portals.exists() && !wrapper.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn nothing_outside_the_home_of_the_test_is_touched() {
    let w = World::new();
    let before = fs::read_dir(w.p("home")).unwrap().count();
    let out = w.setup(&["setup", "file-picker", "--yes"], "");
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(fs::read_dir(w.p("home")).unwrap().count(), before);
    // Everything is under the configuration folder (and the state folder, for the record).
    for entry in walk(&w.p("config")) {
        assert!(entry.starts_with(w.p("config")));
    }
}

#[cfg(target_os = "linux")]
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            }
            out.push(p);
        }
    }
    out
}

// ------------------------------------------------------------------ the script the portal runs

#[cfg(unix)]
mod wrapper {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Stand-ins: a terminal that writes down how it was called and runs what it was given
    /// (from the rada that follows its own options), and a rada that writes down its
    /// arguments and answers.
    struct Stage {
        w: World,
        record: PathBuf,
        rada_record: PathBuf,
    }

    const TERMINALS: &[&str] = &[
        "ghostty",
        "kitty",
        "foot",
        "alacritty",
        "wezterm",
        "konsole",
        "gnome-terminal",
        "xterm",
        "faketerm",
    ];

    impl Stage {
        fn new() -> Stage {
            let w = World::new();
            let record = w.p("terminal.log");
            let rada_record = w.p("rada.log");
            let stage = Stage {
                w,
                record,
                rada_record,
            };
            stage.script(
                "rada-fake",
                r#"#!/bin/sh
for a in "$@"; do printf '%s\n' "$a"; done > "$RADA_RECORD"
out=""
while [ $# -gt 0 ]; do
    if [ "$1" = "--chooser-file" ]; then out="$2"; fi
    shift
done
if [ -z "$RADA_CANCEL" ]; then printf '%s\n' "/chosen/one" "/chosen/two" > "$out"; fi
exit "${RADA_EXIT:-0}"
"#,
            );
            for t in TERMINALS {
                stage.script(
                    t,
                    r#"#!/bin/sh
name="${0##*/}"
{ printf '%s\n' "$name"; for a in "$@"; do printf '%s\n' "$a"; done; } > "$TERM_RECORD"
while [ $# -gt 0 ] && [ "$1" != "$RADA_BIN" ]; do shift; done
[ $# -gt 0 ] || exit 9
"$@"
status=$?
[ -z "$TERM_EXIT" ] || exit "$TERM_EXIT"
exit $status
"#,
                );
            }
            stage
        }

        fn script(&self, name: &str, body: &str) {
            let p = self.w.p("bin").join(name);
            fs::write(&p, body).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }

        /// Run the script of the source as the portal does: six arguments.
        fn run(&self, args: [&str; 6], env: &[(&str, &str)]) -> Output {
            let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/linux/rada-wrapper.sh");
            let mut c = Command::new("/bin/sh");
            c.arg(script)
                .args(args)
                .env_clear()
                // Only the stand-ins: a real terminal on this machine must never be started.
                .env("PATH", self.w.p("bin"))
                .env("HOME", self.w.p("home"))
                .env("RADA_BIN", self.w.p("bin/rada-fake"))
                .env("RADA_RECORD", &self.rada_record)
                .env("TERM_RECORD", &self.record);
            for (k, v) in env {
                c.env(k, v);
            }
            c.output().unwrap()
        }

        fn terminal(&self) -> Vec<String> {
            fs::read_to_string(&self.record)
                .unwrap_or_default()
                .lines()
                .map(String::from)
                .collect()
        }

        fn rada_args(&self) -> Vec<String> {
            fs::read_to_string(&self.rada_record)
                .unwrap_or_default()
                .lines()
                .map(String::from)
                .collect()
        }
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn one_file_is_asked_for_and_the_answer_is_where_the_portal_looks() {
        let s = Stage::new();
        let out = s.w.p("answer");
        let o = s.run(
            ["0", "0", "0", "/home/u/docs", out.to_str().unwrap(), "0"],
            &[("TERMINAL", "kitty")],
        );
        assert!(o.status.success(), "{}", text(&o));
        assert_eq!(
            s.rada_args(),
            [
                "--chooser-file",
                out.to_str().unwrap(),
                "--",
                "/home/u/docs"
            ]
        );
        assert_eq!(
            fs::read_to_string(&out).unwrap(),
            "/chosen/one\n/chosen/two\n"
        );
        let t = s.terminal();
        assert_eq!(t[0], "kitty");
        assert!(
            t.contains(&"com.formikadesk.rada.chooser".to_string()),
            "{t:?}"
        );
    }

    #[test]
    fn several_files_a_folder_and_several_folders() {
        let s = Stage::new();
        let out = s.w.p("answer");
        let o = out.to_str().unwrap();
        for (args, want) in [
            (["1", "0", "0", "/p", o, "0"], vec!["--chooser-multiple"]),
            (["0", "1", "0", "/p", o, "0"], vec!["--chooser-dir"]),
            (
                ["1", "1", "0", "/p", o, "0"],
                vec!["--chooser-dir", "--chooser-multiple"],
            ),
        ] {
            let r = s.run(args, &[("TERMINAL", "foot")]);
            assert!(r.status.success(), "{}", text(&r));
            let mut expected = strs(&["--chooser-file", o]);
            expected.extend(strs(&want));
            expected.extend(strs(&["--", "/p"]));
            assert_eq!(s.rada_args(), expected, "{args:?}");
        }
    }

    #[test]
    fn saving_splits_the_folder_from_the_name() {
        let s = Stage::new();
        let out = s.w.p("answer");
        let o = out.to_str().unwrap();
        s.run(
            ["0", "0", "1", "/home/u/My Docs/report final.pdf", o, "0"],
            &[("TERMINAL", "alacritty")],
        );
        assert_eq!(
            s.rada_args(),
            [
                "--chooser-file",
                o,
                "--chooser-save=report final.pdf",
                "--",
                "/home/u/My Docs"
            ]
        );
        // Quotes and other awkward characters reach rada as they are.
        s.run(
            ["0", "0", "1", "/home/u/it's \"x\" $HOME `a`.pdf", o, "0"],
            &[("TERMINAL", "alacritty")],
        );
        assert_eq!(s.rada_args()[2], "--chooser-save=it's \"x\" $HOME `a`.pdf");
        assert_eq!(s.rada_args()[4], "/home/u");
        // A folder with no name: nothing is suggested.
        s.run(
            ["0", "0", "1", "/home/u/", o, "0"],
            &[("TERMINAL", "alacritty")],
        );
        assert_eq!(s.rada_args()[2], "--chooser-save=");
    }

    #[test]
    fn a_folder_with_spaces_and_quotes_arrives_whole() {
        let s = Stage::new();
        let out = s.w.p("answer");
        s.run(
            [
                "0",
                "0",
                "0",
                "/tmp/a b/it's \"here\"",
                out.to_str().unwrap(),
                "0",
            ],
            &[("TERMINAL", "xterm")],
        );
        assert_eq!(s.rada_args()[3], "/tmp/a b/it's \"here\"");
    }

    #[test]
    fn no_path_means_the_home_folder() {
        let s = Stage::new();
        let out = s.w.p("answer");
        s.run(
            ["0", "0", "0", "", out.to_str().unwrap(), "0"],
            &[("TERMINAL", "foot")],
        );
        assert_eq!(
            s.rada_args().last().unwrap(),
            &s.w.p("home").display().to_string()
        );
    }

    #[test]
    fn each_terminal_is_given_the_options_that_keep_it_in_front() {
        let s = Stage::new();
        let out = s.w.p("answer");
        let o = out.to_str().unwrap();
        let rada = s.w.p("bin/rada-fake").display().to_string();
        for (term, must) in [
            ("ghostty", &["--gtk-single-instance=false", "-e"][..]),
            ("wezterm", &["start", "--always-new-process", "--"]),
            ("konsole", &["--nofork", "-e"]),
            ("gnome-terminal", &["--wait", "--"]),
            ("alacritty", &["-e"]),
            ("xterm", &["-e"]),
        ] {
            let r = s.run(["0", "0", "0", "/p", o, "0"], &[("TERMINAL", term)]);
            assert!(r.status.success(), "{term}: {}", text(&r));
            let t = s.terminal();
            assert_eq!(t[0], term);
            for m in must {
                assert!(t.contains(&m.to_string()), "{term}: {t:?}");
            }
            // The command comes after the terminal's own options, in one piece.
            let at = t.iter().position(|a| *a == rada).unwrap();
            assert!(
                t[at..].contains(&"--chooser-file".to_string()),
                "{term}: {t:?}"
            );
        }
    }

    #[test]
    fn terminal_with_its_own_options_in_the_variable() {
        let s = Stage::new();
        let out = s.w.p("answer");
        s.run(
            ["0", "0", "0", "/p", out.to_str().unwrap(), "0"],
            &[("TERMINAL", "alacritty --class mine")],
        );
        let t = s.terminal();
        let i = t.iter().position(|a| a == "mine").unwrap();
        assert_eq!(t[i - 1], "--class");
        assert!(t.contains(&"-e".to_string()));
    }

    #[test]
    fn termcmd_is_read_the_way_the_portals_other_wrappers_read_it() {
        let s = Stage::new();
        let out = s.w.p("answer");
        let r = s.run(
            ["0", "0", "0", "/p", out.to_str().unwrap(), "0"],
            &[
                ("TERMCMD", "faketerm --title \"a b\" -e"),
                ("TERMINAL", "kitty"),
            ],
        );
        assert!(r.status.success(), "{}", text(&r));
        let t = s.terminal();
        assert_eq!(t[0], "faketerm", "TERMCMD wins over TERMINAL: {t:?}");
        assert!(t.contains(&"a b".to_string()), "{t:?}");
        assert_eq!(
            fs::read_to_string(&out).unwrap(),
            "/chosen/one\n/chosen/two\n"
        );
    }

    #[test]
    fn a_terminal_that_is_not_there_gives_way_to_the_first_that_is() {
        let s = Stage::new();
        let out = s.w.p("answer");
        // ghostty and kitty are the first of the list: take them away, foot is next.
        for gone in ["ghostty", "kitty"] {
            fs::remove_file(s.w.p("bin").join(gone)).unwrap();
        }
        let r = s.run(
            ["0", "0", "0", "/p", out.to_str().unwrap(), "0"],
            &[("TERMINAL", "not-installed-anywhere")],
        );
        assert!(r.status.success(), "{}", text(&r));
        assert_eq!(s.terminal()[0], "foot");
        let r = s.run(["0", "0", "0", "/p", out.to_str().unwrap(), "0"], &[]);
        assert!(r.status.success(), "{}", text(&r));
        assert_eq!(s.terminal()[0], "foot");
    }

    #[test]
    fn a_cancel_leaves_the_answer_empty_and_the_portal_is_told_nothing_went_wrong() {
        let s = Stage::new();
        let out = s.w.p("answer");
        fs::write(&out, "").unwrap();
        let r = s.run(
            ["0", "0", "0", "/p", out.to_str().unwrap(), "0"],
            &[("TERMINAL", "foot"), ("RADA_CANCEL", "1")],
        );
        assert!(r.status.success(), "{}", text(&r));
        assert_eq!(fs::read_to_string(&out).unwrap(), "");
    }

    #[test]
    fn what_the_terminal_returns_is_what_the_script_returns() {
        let s = Stage::new();
        let out = s.w.p("answer");
        let r = s.run(
            ["0", "0", "0", "/p", out.to_str().unwrap(), "0"],
            &[("TERMINAL", "foot"), ("TERM_EXIT", "3")],
        );
        assert_eq!(r.status.code(), Some(3));
    }

    #[test]
    fn the_debug_flag_traces_and_still_works() {
        let s = Stage::new();
        let out = s.w.p("answer");
        let r = s.run(
            ["0", "0", "0", "/p", out.to_str().unwrap(), "1"],
            &[("TERMINAL", "foot")],
        );
        assert!(r.status.success());
        assert!(
            String::from_utf8_lossy(&r.stderr).contains("+ "),
            "{}",
            text(&r)
        );
    }

    #[test]
    fn the_script_is_valid_and_is_the_one_the_setup_installs() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/linux/rada-wrapper.sh");
        let check = Command::new("/bin/sh")
            .arg("-n")
            .arg(&script)
            .output()
            .unwrap();
        assert!(check.status.success(), "{}", text(&check));
        let first = fs::read_to_string(&script).unwrap();
        assert!(first.starts_with("#!/usr/bin/env sh\n"));
        assert_ne!(
            fs::metadata(&script).unwrap().permissions().mode() & 0o111,
            0,
            "the script in the source is executable"
        );
    }
}
