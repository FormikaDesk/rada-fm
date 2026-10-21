//! The state of a folder in a git repository, asked of the real git on repositories made in the
//! test's own temporary folder (never a real one), kept from the person's own settings, and of
//! made-up programs for a git that is missing, that refuses or that takes too long.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crossbeam_channel::{Receiver, unbounded};
use rada_core::events::CoreEvent;
use rada_core::git::{FileState, Git, GitStatus};
use rada_core::testutil::*;

/// A `Git` kept from the person's settings: no global or system configuration, a home folder of
/// the test's own, and no looking for a repository above the test's `work` folder (the
/// sandbox itself is inside the repository this program is built from).
fn isolated(sb: &Sandbox) -> Git {
    let home = sb.mkdir("home-for-git");
    Git::new(true, None)
        .with_env(
            "GIT_CEILING_DIRECTORIES",
            &sb.path("work").to_string_lossy(),
        )
        .with_env("GIT_CONFIG_GLOBAL", "/dev/null")
        .with_env("GIT_CONFIG_NOSYSTEM", "1")
        .with_env("HOME", &home.to_string_lossy())
        .with_env("XDG_CONFIG_HOME", &home.to_string_lossy())
}

fn git_installed() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Run git in `dir` the way a person would, to make the repository the test looks at.
fn git(dir: &Path, home: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
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

/// What the worker answers about `dir`, or `None` when it sends nothing in `wait`.
fn ask(git: &Git, dir: &Path, wait: Duration) -> Option<Option<GitStatus>> {
    let (tx, rx): (_, Receiver<CoreEvent>) = unbounded();
    git.status(dir, tx);
    match rx.recv_timeout(wait) {
        Ok(CoreEvent::Git { dir: d, status }) => {
            assert_eq!(d, dir);
            Some(status)
        }
        Ok(_) => panic!("another event"),
        Err(_) => None,
    }
}

/// A repository with `a.txt` and `src/lib.rs` committed, then `a.txt` changed, `src/lib.rs`
/// changed, `new.txt` made and `src/extra.rs` made.
fn repo(sb: &Sandbox) -> PathBuf {
    let dir = sb.mkdir("work/repo");
    let home = sb.mkdir("home-for-git");
    git(&dir, &home, &["init", "-q"]);
    sb.write("work/repo/a.txt", "one\n");
    sb.write("work/repo/b.txt", "two\n");
    sb.write("work/repo/src/lib.rs", "pub fn f() {}\n");
    git(&dir, &home, &["add", "."]);
    git(&dir, &home, &["commit", "-q", "-m", "first"]);
    sb.write("work/repo/a.txt", "one, changed\n");
    sb.write("work/repo/src/lib.rs", "pub fn g() {}\n");
    sb.write("work/repo/new.txt", "new\n");
    sb.write("work/repo/src/extra.rs", "pub fn h() {}\n");
    dir
}

#[test]
fn a_repository_shows_its_branch_its_counts_and_the_items_of_the_folder() {
    if !git_installed() {
        eprintln!("git is not installed: test skipped");
        return;
    }
    let sb = Sandbox::new();
    let dir = repo(&sb);
    let git = isolated(&sb);
    let st = ask(&git, &dir, Duration::from_secs(20))
        .expect("an answer")
        .expect("a repository");
    assert_eq!(st.branch, "main");
    assert_eq!((st.ahead, st.behind), (0, 0), "no upstream");
    assert_eq!((st.modified, st.new), (2, 2));
    assert_eq!(st.items["a.txt"], FileState::Modified);
    assert_eq!(st.items["new.txt"], FileState::New);
    assert_eq!(
        st.items["src"],
        FileState::Modified,
        "a change inside beats a new file inside"
    );
    assert!(
        !st.items.contains_key("b.txt"),
        "unchanged: nothing to show"
    );

    // From a folder of the repository: the counts are the repository's, the items the folder's.
    let st = ask(&git, &dir.join("src"), Duration::from_secs(20))
        .unwrap()
        .unwrap();
    assert_eq!((st.modified, st.new), (2, 2));
    assert_eq!(st.items.len(), 2, "{:?}", st.items);
    assert_eq!(st.items["lib.rs"], FileState::Modified);
    assert_eq!(st.items["extra.rs"], FileState::New);
}

#[test]
fn a_committed_repository_with_nothing_changed_is_clean() {
    if !git_installed() {
        eprintln!("git is not installed: test skipped");
        return;
    }
    let sb = Sandbox::new();
    let dir = repo(&sb);
    let home = sb.path("home-for-git");
    git(&dir, &home, &["add", "."]);
    git(&dir, &home, &["commit", "-q", "-m", "second"]);
    let st = ask(&isolated(&sb), &dir, Duration::from_secs(20))
        .unwrap()
        .unwrap();
    assert!(st.is_clean() && st.items.is_empty());
    // A new branch, and a commit checked out by its name: no branch, the start of the commit.
    git(&dir, &home, &["checkout", "-q", "-b", "feat/pannelli"]);
    let st = ask(&isolated(&sb), &dir, Duration::from_secs(20))
        .unwrap()
        .unwrap();
    assert_eq!(st.branch, "feat/pannelli");
    git(&dir, &home, &["checkout", "-q", "--detach"]);
    let st = ask(&isolated(&sb), &dir, Duration::from_secs(20))
        .unwrap()
        .unwrap();
    assert_eq!(st.branch.len(), 7, "{}", st.branch);
}

#[test]
fn a_folder_that_is_not_in_a_repository_says_so() {
    if !git_installed() {
        eprintln!("git is not installed: test skipped");
        return;
    }
    let sb = Sandbox::new();
    let plain = sb.mkdir("work/plain");
    sb.write("work/plain/x.txt", "x");
    let git = isolated(&sb);
    assert_eq!(ask(&git, &plain, Duration::from_secs(20)), Some(None));
}

#[test]
fn nothing_happens_where_git_is_missing_off_or_unsure() {
    let sb = Sandbox::new();
    let dir = sb.mkdir("work/x");
    // Not installed: no answer, no error.
    let missing = Git::new(true, Some(sb.path("no/such/git")));
    assert_eq!(ask(&missing, &dir, Duration::from_millis(500)), None);
    // Switched off.
    assert_eq!(ask(&Git::off(), &dir, Duration::from_millis(300)), None);
}

#[cfg(unix)]
#[test]
fn a_git_that_takes_too_long_is_given_up_on_and_stopped() {
    use std::os::unix::fs::PermissionsExt;
    let sb = Sandbox::new();
    let dir = sb.mkdir("work/x");
    // A program that never answers in time: it sleeps, and writes a file if it ever wakes up.
    let marker = sb.path("woke");
    let script = sb.write(
        "bin/git",
        format!("#!/bin/sh\nsleep 5\ntouch '{}'\n", marker.display()),
    );
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let slow = Git::new(true, Some(script)).with_patience(Duration::from_millis(300));
    let started = std::time::Instant::now();
    assert_eq!(ask(&slow, &dir, Duration::from_millis(1500)), None);
    assert!(started.elapsed() < Duration::from_secs(4));
    // It was stopped, not left running: it never gets to write what it writes when it is done.
    std::thread::sleep(Duration::from_millis(600));
    assert!(!marker.exists());
}

#[cfg(unix)]
#[test]
fn a_git_that_fails_is_not_a_repository() {
    use std::os::unix::fs::PermissionsExt;
    let sb = Sandbox::new();
    let dir = sb.mkdir("work/x");
    let script = sb.write(
        "bin/git",
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo git version 2; exit 0; fi\nexit 128\n",
    );
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let refusing = Git::new(true, Some(script));
    assert_eq!(ask(&refusing, &dir, Duration::from_secs(5)), Some(None));
}
