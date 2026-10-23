//! "Open with": the applications found for a file from `.desktop` files, `mimeinfo.cache` and
//! `mimeapps.list` that these tests write in a folder of their own (the system folders of the
//! freedesktop.org lists are empty ones in the sandbox), and a program launched from one of
//! them. The program is a script of the test, and what it was given is read from its log.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rada_core::platform::Platform;
use rada_core::platform::linux::LinuxPlatform;
use rada_core::platform::openwith::{self, Prefs};
use rada_core::testutil::*;

fn desktop(sb: &Sandbox, id: &str, body: &str) -> PathBuf {
    let dir = sb.dirs.data.join("applications");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(id);
    std::fs::write(&p, format!("[Desktop Entry]\nType=Application\n{body}\n")).unwrap();
    p
}

/// An executable that appends what it was given to `log`.
fn script(sb: &Sandbox, name: &str, log: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = sb.root.join("programs");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\" >> '{}'; done\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn mime_database(sb: &Sandbox) {
    let dir = sb.dirs.data.join("mime");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("globs2"),
        "# test\n50:text/plain:*.txt\n50:text/x-rust:*.rs\n50:image/png:*.png\n50:application/pdf:*.pdf\n",
    )
    .unwrap();
    std::fs::write(dir.join("subclasses"), "text/x-rust text/plain\n").unwrap();
}

fn names(p: &dyn Platform, file: &Path) -> Vec<String> {
    p.opener()
        .apps_for(file)
        .apps
        .into_iter()
        .map(|a| a.name)
        .collect()
}

#[test]
fn the_applications_that_fit_the_type_are_listed_the_default_first() {
    let sb = Sandbox::new();
    mime_database(&sb);
    let bin = script(&sb, "prog", &sb.root.join("unused.log"));
    let exec = bin.display();
    desktop(
        &sb,
        "kate.desktop",
        &format!("Name=Kate\nExec={exec} %f\nMimeType=text/plain;"),
    );
    desktop(
        &sb,
        "vim.desktop",
        &format!(
            "Name=Vim\nName[it]=Vim in terminale\nExec={exec} %F\nTerminal=true\nMimeType=text/plain;"
        ),
    );
    desktop(
        &sb,
        "rustrover.desktop",
        &format!("Name=Rust Editor\nExec={exec} %f\nMimeType=text/x-rust;"),
    );
    desktop(
        &sb,
        "viewer.desktop",
        &format!("Name=Viewer\nExec={exec} %f\nMimeType=image/png;image/jpeg;"),
    );
    desktop(
        &sb,
        "hidden.desktop",
        &format!("Name=Hidden\nExec={exec} %f\nMimeType=text/plain;\nNoDisplay=true"),
    );
    desktop(
        &sb,
        "gone.desktop",
        "Name=Gone\nExec=/nonexistent/program %f\nMimeType=text/plain;",
    );
    desktop(
        &sb,
        "tryexec.desktop",
        &format!("Name=Needs\nExec={exec} %f\nTryExec=/nonexistent/needed\nMimeType=text/plain;"),
    );
    // A folder in applications/ is part of the id.
    let sub = sb.dirs.data.join("applications/vendor");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("tool.desktop"), format!("[Desktop Entry]\nType=Application\nName=Vendor Tool\nExec={exec} %f\nMimeType=text/plain;\n")).unwrap();
    // The user's own default for plain text, and a program taken away from it.
    std::fs::create_dir_all(&sb.dirs.config).unwrap();
    std::fs::write(
        sb.dirs.config.join("mimeapps.list"),
        "[Default Applications]\ntext/plain=vim.desktop;\n[Removed Associations]\ntext/plain=vendor-tool.desktop;\n",
    )
    .unwrap();

    let p = sb.platform();
    let txt = sb.write("notes.txt", "hello");
    let apps = p.opener().apps_for(&txt);
    assert_eq!(apps.kind.as_deref(), Some("text/plain"));
    let list: Vec<(&str, bool)> = apps
        .apps
        .iter()
        .map(|a| (a.name.as_str(), a.is_default))
        .collect();
    assert_eq!(list[0], ("Vim", true), "the default comes first: {list:?}");
    let mut rest: Vec<&str> = list[1..].iter().map(|(n, _)| *n).collect();
    rest.sort();
    assert_eq!(
        rest,
        ["Kate"],
        "hidden, missing and removed ones are not offered: {list:?}"
    );
    assert!(apps.apps[0].terminal);
    assert_eq!(apps.apps[0].id, "vim.desktop");

    // A Rust file is a text file too: the editors of both are offered.
    let rs = sb.write("main.rs", "fn main() {}");
    let mut got = names(p.as_ref(), &rs);
    got.sort();
    assert_eq!(got, ["Kate", "Rust Editor", "Vim"]);
    assert_eq!(p.opener().kind_of(&rs).as_deref(), Some("text/x-rust"));

    // A picture: only what says it opens pictures.
    let png = sb.write("pic.png", "\u{89}PNG");
    assert_eq!(names(p.as_ref(), &png), ["Viewer"]);

    // No extension but text inside: text. Binary data: nothing fits.
    let readme = sb.write("README", "just words\n");
    assert!(names(p.as_ref(), &readme).contains(&"Kate".to_string()));
    let blob = sb.write("blob.bin", [0u8, 1, 2, 3, 0, 255]);
    assert!(names(p.as_ref(), &blob).is_empty());
}

#[test]
fn mimeinfo_cache_adds_applications() {
    let sb = Sandbox::new();
    mime_database(&sb);
    let bin = script(&sb, "prog", &sb.root.join("unused.log"));
    // The file names no MIME type; the cache says it opens PDFs, as `update-desktop-database`
    // would have written it.
    desktop(
        &sb,
        "reader.desktop",
        &format!("Name=Reader\nExec={} %f", bin.display()),
    );
    std::fs::write(
        sb.dirs.data.join("applications/mimeinfo.cache"),
        "[MIME Cache]\napplication/pdf=reader.desktop;missing.desktop;\n",
    )
    .unwrap();
    desktop(
        &sb,
        "extra.desktop",
        &format!("Name=Extra\nExec={} %f", bin.display()),
    );
    std::fs::create_dir_all(&sb.dirs.config).unwrap();
    std::fs::write(
        sb.dirs.config.join("mimeapps.list"),
        "[Added Associations]\napplication/pdf=extra.desktop;\n",
    )
    .unwrap();
    let pdf = sb.write("doc.pdf", "%PDF-1.4");
    let mut got = names(sb.platform().as_ref(), &pdf);
    got.sort();
    assert_eq!(got, ["Extra", "Reader"]);
}

#[test]
fn the_first_folder_wins_when_an_id_is_in_two() {
    let sb = Sandbox::new();
    mime_database(&sb);
    let bin = script(&sb, "prog", &sb.root.join("unused.log"));
    desktop(
        &sb,
        "ed.desktop",
        &format!("Name=Mine\nExec={} %f\nMimeType=text/plain;", bin.display()),
    );
    // A system folder through XDG_DATA_DIRS is not used in the sandbox; the point is shown with
    // the folders given directly.
    let system = sb.root.join("system/applications");
    std::fs::create_dir_all(&system).unwrap();
    std::fs::write(
        system.join("ed.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Theirs\nExec={} %f\nMimeType=text/plain;\n",
            bin.display()
        ),
    )
    .unwrap();
    let dirs = openwith::AppDirs {
        data: vec![sb.dirs.data.clone(), sb.root.join("system")],
        config: vec![],
    };
    let db = openwith::MimeDb::load(&dirs.data);
    let f = sb.write("a.txt", "x");
    let (_, apps) = openwith::find_apps(&dirs, &db, &f, "en");
    assert_eq!(
        apps.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        ["Mine"]
    );
}

fn wait_for(log: &Path) -> Vec<String> {
    let start = Instant::now();
    loop {
        if let Ok(t) = std::fs::read_to_string(log)
            && !t.is_empty()
        {
            return t.lines().map(String::from).collect();
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the program never ran"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_chosen_application_is_started_with_the_file_and_nothing_else() {
    let sb = Sandbox::new();
    mime_database(&sb);
    let log = sb.root.join("run.log");
    let bin = script(&sb, "editor", &log);
    desktop(
        &sb,
        "ed.desktop",
        &format!(
            "Name=Ed\nExec={} --reuse \"{}\" %f\nMimeType=text/plain;",
            bin.display(),
            "a b"
        ),
    );
    // The real opener of this system, over the sandbox's folders: nothing of the desktop
    // is touched, and the program is a script of the test.
    let real = LinuxPlatform::new(sb.dirs.clone());
    let f = sb.write("My Notes.txt", "hello");
    let apps = real.opener().apps_for(&f).apps;
    assert_eq!(apps.len(), 1);
    real.opener().open_with_app(&f, &apps[0]).unwrap();
    assert_eq!(wait_for(&log), ["--reuse", "a b", f.to_str().unwrap()]);
}

#[test]
fn the_sandbox_never_starts_an_application() {
    let sb = Sandbox::new();
    mime_database(&sb);
    let log = sb.root.join("run.log");
    let bin = script(&sb, "editor", &log);
    desktop(
        &sb,
        "ed.desktop",
        &format!("Name=Ed\nExec={} %f\nMimeType=text/plain;", bin.display()),
    );
    let f = sb.write("a.txt", "x");
    let p = sb.platform();
    let app = p.opener().apps_for(&f).apps.remove(0);
    p.opener().open_with_app(&f, &app).unwrap();
    assert_eq!(
        sb.opened(),
        [format!("open_with_app ed.desktop {}", f.display())]
    );
    std::thread::sleep(Duration::from_millis(150));
    assert!(!log.exists(), "the program must not have run");
}

#[test]
fn recent_and_always_choices_order_the_list_and_live_in_radas_own_folder() {
    let sb = Sandbox::new();
    mime_database(&sb);
    let bin = script(&sb, "prog", &sb.root.join("unused.log"));
    for (id, name) in [
        ("a.desktop", "Alpha"),
        ("b.desktop", "Beta"),
        ("c.desktop", "Gamma"),
    ] {
        desktop(
            &sb,
            id,
            &format!(
                "Name={name}\nExec={} %f\nMimeType=text/plain;",
                bin.display()
            ),
        );
    }
    let p = sb.platform();
    let f = sb.write("a.txt", "x");
    let choices = p.opener().apps_for(&f);
    let kind = choices.kind.clone().unwrap();
    let state = sb.dirs.rada_state();
    let mut prefs = Prefs::load(&state);
    prefs.used("c.desktop");
    prefs.always.insert(kind.clone(), "b.desktop".into());
    prefs.save(&state).unwrap();
    let again = Prefs::load(&state);
    let order: Vec<String> = openwith::ordered(choices.apps, &again, &kind)
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert_eq!(order, ["Beta", "Gamma", "Alpha"]);
    // The system's own lists were not written.
    assert!(!sb.dirs.config.join("mimeapps.list").exists());
    assert!(!sb.dirs.data.join("applications/mimeapps.list").exists());
    assert!(state.join("open_with.json").exists());
}
