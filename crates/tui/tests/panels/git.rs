//! The branch and the counts of a git repository, in the status of the folder.

use crate::common::*;
use crate::harness::*;
use rada_tui::IconSet;
use ratatui::style::Color;

/// Run git the way a person would, in the demo folder, to make the repository the test looks at.
fn git(dir: &std::path::Path, home: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The demo folder as a repository on `main`: everything committed but `setup.sh`, then
/// `main.rs` changed. Returns the harness, with git asked for in the way the tests do it.
fn git_scene() -> Option<H> {
    if !std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("git is not installed: test skipped");
        return None;
    }
    let mut h = scene(160, 46, "rada", false);
    let dir = h.app.cwd.clone();
    let home = h.app.home().to_path_buf();
    git(&dir, &home, &["init", "-q", "-b", "main"]);
    git(&dir, &home, &["add", "-A", "--", ".", ":!setup.sh"]);
    git(&dir, &home, &["commit", "-q", "-m", "first"]);
    std::fs::write(
        dir.join("main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .unwrap();
    h.app.use_git(
        rada_core::git::Git::new(true, None)
            .with_env("GIT_CEILING_DIRECTORIES", &home.to_string_lossy())
            .with_env("GIT_CONFIG_GLOBAL", "/dev/null")
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env("HOME", &home.to_string_lossy()),
    );
    h.wait("git's answer", |a| a.git_here().is_some());
    Some(h)
}

#[test]
fn git_folder_shows_branch_and_counts() {
    let Some(mut h) = git_scene() else { return };
    let s = h.screen();
    // On the top edge of the folder's box: the branch, one changed file, one new one.
    let title = s
        .lines()
        .find(|l| l.contains("[ ~/projects/demo ]"))
        .unwrap();
    assert!(
        title.contains("main") && title.contains("●1") && title.contains("?1"),
        "{title}"
    );
    // After the name of a file: M for a change, ? for what is new; nothing for the rest.
    let row = |name: &str| s.lines().find(|l| l.contains(name)).unwrap().to_string();
    assert!(row("main.rs").contains("main.rs M"), "{}", row("main.rs"));
    assert!(
        row("setup.sh").contains("setup.sh ?"),
        "{}",
        row("setup.sh")
    );
    assert!(
        !row("Cargo.toml").contains("Cargo.toml M") && !row("Cargo.toml").contains("Cargo.toml ?")
    );
    // The status bar names the branch too.
    let status = s.lines().find(|l| l.contains("19 items")).unwrap();
    assert!(status.contains("main"), "{status}");
    // Under the cursor: what git says of that file, in the box of details.
    put_cursor(&mut h, "main.rs");
    let s = h.screen();
    let git_row = s.lines().find(|l| l.contains(" Git ")).expect("a Git row");
    assert!(git_row.contains("M modified"), "{git_row}");
    put_cursor(&mut h, "setup.sh");
    assert!(h.screen().contains("? new"), "{}", h.screen());
    put_cursor(&mut h, "Cargo.toml");
    assert!(h.screen().contains("no changes"));
}

#[test]
fn leaving_the_repository_takes_the_branch_away() {
    let Some(mut h) = git_scene() else { return };
    let docs = h.app.home().join("Documents");
    h.app.open_dir(docs.clone());
    h.wait("Documents", |a| a.cwd == docs && !a.is_loading());
    // The answer for the new folder is that it is in no repository.
    h.wait("no git here", |a| a.git.is_none());
    let s = h.screen();
    assert!(!s.contains('●') || !s.contains("main"), "{s}");
    assert!(
        !s.lines()
            .any(|l| l.contains("[ ~/Documents ]") && l.contains("main"))
    );
}

#[test]
fn git_is_never_called_when_it_is_off() {
    // The harness gives the program no git at all, as the configuration `git = false` does.
    let mut h = scene(160, 46, "rada", false);
    h.pump(300);
    assert!(h.app.git.is_none());
    let s = h.screen();
    let title = s
        .lines()
        .find(|l| l.contains("[ ~/projects/demo ]"))
        .unwrap();
    assert!(!title.contains('●') && !title.contains('?'), "{title}");
}

#[test]
fn the_git_labels_are_pills_and_every_colour_of_them_is_the_themes() {
    let Some(mut h) = git_scene() else { return };
    h.app.icons = IconSet::Nerd;
    for name in ["main.rs", "setup.sh"] {
        put_cursor(&mut h, name);
        let th = h.app.th.clone();
        let buf = h.redrawn_buffer();
        let behind = [th.inner, th.canvas];
        assert_pills(&buf, &format!("git, on {name}"), &behind);
        let allowed = th.all_colors();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                for c in [buf[(x, y)].fg, buf[(x, y)].bg] {
                    assert!(
                        c == Color::Reset || allowed.contains(&c),
                        "git, on {name}: {x},{y} {:?} uses {c:?}",
                        buf[(x, y)].symbol()
                    );
                }
            }
        }
    }
}
