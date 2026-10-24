//! zoxide in the command bar and in the folders rada enters. The program is a made-up script in
//! the sandbox (the real zoxide is never called): it writes what it was asked to a log and
//! answers `query` with a list of folders.
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::*;
use crossterm::event::KeyCode;
use rada_core::testutil::Sandbox;
use rada_core::zoxide::Zoxide;
use rada_tui::app::BarItem;

struct Fake {
    program: PathBuf,
    log: PathBuf,
    /// The folders the script answers with.
    known: Vec<PathBuf>,
}

/// A sandbox with a work folder, `n` folders zoxide "knows" and the script.
fn world(n: usize) -> (Sandbox, PathBuf, Fake) {
    let sb = Sandbox::new();
    sb.write("w/alpha.txt", "a");
    let work = sb.path("w");
    let known: Vec<PathBuf> = (0..n).map(|i| sb.mkdir(format!("known/proj{i}"))).collect();
    let list = sb.write(
        "tools/list.txt",
        known
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let log = sb.path("tools/log.txt");
    let program = sb.write(
        "tools/zoxide",
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nif [ \"$1\" = query ]; then cat '{}'; fi\n",
            log.display(),
            list.display()
        ),
    );
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    (
        sb,
        work,
        Fake {
            program,
            log,
            known,
        },
    )
}

fn logged(f: &Fake) -> String {
    std::fs::read_to_string(&f.log).unwrap_or_default()
}

fn zoxide_hits(h: &H) -> Vec<PathBuf> {
    h.app
        .windows
        .address
        .as_ref()
        .map(|a| {
            a.hits
                .iter()
                .filter_map(|x| match &x.item {
                    BarItem::Zoxide(p) => Some(p.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Wait up to `secs` for `done`, with the events of the workers going through.
fn until(h: &mut H, secs: u64, mut done: impl FnMut(&mut H) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    let rx = h.app.events();
    while Instant::now() < end {
        if done(h) {
            return true;
        }
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(20)) {
            h.app.on_core_event(ev);
        }
    }
    done(h)
}

#[test]
fn what_matches_nothing_else_is_offered_from_zoxide_and_enter_goes_there() {
    let (sb, work, fake) = world(7);
    let mut h = H::new(sb, work, 140, 36);
    h.app
        .use_zoxide(Zoxide::new(true, Some(fake.program.clone())));
    h.press("ctrl+p");
    h.keys("qzx");
    assert!(
        until(&mut h, 10, |h| !zoxide_hits(h).is_empty()),
        "no answer"
    );
    let hits = zoxide_hits(&h);
    assert_eq!(hits, fake.known[..5], "at most five, as zoxide listed them");
    assert!(
        logged(&fake).contains("query --list -- qzx"),
        "{}",
        logged(&fake)
    );
    let screen = h.screen();
    assert!(
        screen.contains("zoxide") && screen.contains("proj0"),
        "{screen}"
    );
    // Enter on the arrow-picked one opens it.
    h.key(KeyCode::Down);
    h.key(KeyCode::Down);
    h.key(KeyCode::Enter);
    h.settle();
    let want = fake.known[1].clone();
    h.wait("the folder opens", |a| a.tab.cwd == want);
    assert!(h.app.windows.address.is_none());
}

#[test]
fn enter_with_nothing_picked_opens_the_first_folder() {
    let (sb, work, fake) = world(2);
    let mut h = H::new(sb, work, 140, 36);
    h.app
        .use_zoxide(Zoxide::new(true, Some(fake.program.clone())));
    h.press("ctrl+p");
    h.keys("qzx");
    assert!(
        until(&mut h, 10, |h| !zoxide_hits(h).is_empty()),
        "no answer"
    );
    h.key(KeyCode::Enter);
    let want = fake.known[0].clone();
    h.wait("the folder opens", |a| a.tab.cwd == want);
}

#[test]
fn a_command_or_an_existing_name_keeps_zoxide_out() {
    let (sb, work, fake) = world(3);
    let mut h = H::new(sb, work, 140, 36);
    h.app
        .use_zoxide(Zoxide::new(true, Some(fake.program.clone())));
    h.press("ctrl+p");
    // A command matches.
    h.keys("rename");
    // A file of the folder, by its whole name.
    h.press("ctrl+u");
    h.keys("alpha.txt");
    std::thread::sleep(Duration::from_millis(300));
    h.settle();
    assert!(zoxide_hits(&h).is_empty());
    // Typing "alpha.txt" asked about its first letters (they are no command); not about the
    // name that exists, nor about the command.
    let log = logged(&fake);
    assert!(
        !log.contains("-- alpha.txt") && !log.contains("-- rename"),
        "{log}"
    );
}

#[test]
fn without_zoxide_nothing_happens_and_nothing_is_shown() {
    let (sb, work, fake) = world(3);
    let missing = fake.program.with_file_name("not-there");
    let mut h = H::new(sb, work.clone(), 140, 36);
    h.app.use_zoxide(Zoxide::new(true, Some(missing)));
    h.press("ctrl+p");
    h.keys("qzx");
    std::thread::sleep(Duration::from_millis(300));
    h.settle();
    assert!(zoxide_hits(&h).is_empty());
    assert!(h.app.windows.notices.is_empty(), "no error is shown");
    h.key(KeyCode::Enter);
    h.settle();
    assert_eq!(h.app.tab.cwd, work);
}

#[test]
fn rada_tells_zoxide_about_the_folders_it_enters() {
    let (sb, work, fake) = world(1);
    let mut h = H::new(sb, work, 140, 36);
    h.app
        .use_zoxide(Zoxide::new(true, Some(fake.program.clone())));
    let target = fake.known[0].clone();
    h.app.open_dir(target.clone());
    h.wait("the folder opens", |a| a.tab.cwd == target);
    let want = format!("add -- {}", target.display());
    assert!(
        until(&mut h, 10, |_| logged(&fake).contains(&want)),
        "{}",
        logged(&fake)
    );
}

#[test]
fn switched_off_rada_never_calls_zoxide() {
    let (sb, work, fake) = world(2);
    let mut h = H::new(sb, work, 140, 36);
    h.app
        .use_zoxide(Zoxide::new(false, Some(fake.program.clone())));
    let target = fake.known[0].clone();
    h.app.open_dir(target.clone());
    h.wait("the folder opens", |a| a.tab.cwd == target);
    h.press("ctrl+p");
    h.keys("qzx");
    std::thread::sleep(Duration::from_millis(400));
    h.settle();
    assert!(zoxide_hits(&h).is_empty());
    assert_eq!(logged(&fake), "", "the script never ran");
}
