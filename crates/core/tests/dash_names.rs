//! A file or folder whose name starts with `-` (`-rf`, `--help`) reaches the programs rada runs
//! for it (poppler's tools, git) as a file, never as an option.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crossbeam_channel::unbounded;
use rada_core::events::CoreEvent;
use rada_core::git::{FileState, Git};
use rada_core::preview::pdf::{self, Tools};
use rada_core::preview::{ImageInfo, ImageLimits};
use rada_core::testutil::*;

/// A program that writes the arguments it was given, one per line, to `log`.
fn recorder(sb: &Sandbox, name: &str, log: &Path) -> PathBuf {
    let p = sb.mkdir("bin").join(name);
    let body = format!(
        "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\" >> '{}'; done\n",
        log.display()
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn given(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn the_poppler_tools_get_a_file_that_starts_with_a_dash_as_a_file() {
    let sb = Sandbox::new();
    let limits = ImageLimits {
        pdf_cache: Some(sb.path("cache")),
        ..Default::default()
    };
    let info = ImageInfo {
        format: "pdf".into(),
        width: None,
        height: None,
        size: 1,
        modified: None,
        doc: None,
        thumb: false,
    };
    let (info_log, text_log, ppm_log) =
        (sb.path("info.log"), sb.path("text.log"), sb.path("ppm.log"));
    let tools = Tools {
        pdfinfo: Some(recorder(&sb, "pdfinfo", &info_log)),
        pdftotext: Some(recorder(&sb, "pdftotext", &text_log)),
        pdftoppm: Some(recorder(&sb, "pdftoppm", &ppm_log)),
    };
    for name in ["-rf.pdf", "--help.pdf"] {
        let _ = std::fs::remove_file(&info_log);
        let _ = std::fs::remove_file(&text_log);
        let _ = std::fs::remove_file(&ppm_log);
        // Relative on purpose: a name that starts with a dash is only a danger bare.
        let path = Path::new(name);
        let _ = pdf::facts(&tools, path, &limits, &|| false);
        let _ = pdf::first_page_text(&tools, path, &limits, 4096, &|| false);
        let _ = pdf::render_first_page(&tools, path, &info, &limits, &|| false);
        for (log, tool) in [
            (&info_log, "pdfinfo"),
            (&text_log, "pdftotext"),
            (&ppm_log, "pdftoppm"),
        ] {
            let args = given(log);
            assert!(
                !args.iter().any(|a| a == name),
                "{tool} was given {name:?} bare: {args:?}"
            );
            assert!(
                args.iter().any(|a| *a == format!("./{name}")),
                "{tool}: {args:?}"
            );
        }
    }
}

#[test]
fn a_folder_called_like_an_option_is_still_a_repository_to_git() {
    if !Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("git is not installed: test skipped");
        return;
    }
    let sb = Sandbox::new();
    let home = sb.mkdir("home-for-git");
    for name in ["--version", "-rf", "--help"] {
        let dir = sb.mkdir(format!("work/{name}"));
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
                .args(args)
                .current_dir(&dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", &home)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        };
        run(&["init", "-q"]);
        std::fs::write(dir.join("-n"), "x").unwrap();
        std::fs::write(dir.join("--help"), "x").unwrap();
        let git = Git::new(true, None)
            .with_env(
                "GIT_CEILING_DIRECTORIES",
                &sb.path("work").to_string_lossy(),
            )
            .with_env("GIT_CONFIG_GLOBAL", "/dev/null")
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env("HOME", &home.to_string_lossy());
        let (tx, rx) = unbounded();
        git.status(&dir, tx);
        let Ok(CoreEvent::Git { status, .. }) = rx.recv_timeout(Duration::from_secs(20)) else {
            panic!("no answer for {name}");
        };
        let st = status.unwrap_or_else(|| panic!("{name} is not seen as a repository"));
        assert_eq!(st.items["-n"], FileState::New, "{name}");
        assert_eq!(st.items["--help"], FileState::New, "{name}");
    }
}
