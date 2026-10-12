//! Updates in the interface: the notice in the status bar, the window with the release
//! notes, what each way of updating offers, and an update that fails.

mod common;

use std::time::Duration;

use common::*;
use rada_core::events::CoreEvent;
use rada_core::update::{
    Applied, Channel, Ctx, Method, Mode, Notice, Settings, UpdateEvent, Version,
};
use rada_tui::app::{Modal, UpdateStatus, UpdateView};
use rada_tui::hits::Target;
use rada_tui::keymap::Action;

fn notice() -> Notice {
    Notice {
        version: "0.7.0".into(),
        tag: "v0.7.0".into(),
        prerelease: false,
        notes: "### Added\n- Tags in the Finder's colours\n- A very long line that goes on and on and on to see that the notes are wrapped inside the window and not cut at its edge\n\n### Fixed\n- Something".into(),
    }
}

fn offer(h: &mut H, how: Result<Mode, String>) {
    h.app
        .on_core_event(CoreEvent::Update(UpdateEvent::Available {
            notice: notice(),
            how,
        }));
}

#[test]
fn a_newer_version_shows_in_the_status_bar_with_the_key() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 30);
    assert!(!h.screen().contains("New version"));
    offer(&mut h, Ok(Mode::Binary));
    let s = h.screen();
    assert!(
        s.contains("New version 0.7.0 available — press Alt+u to update"),
        "{s}"
    );
}

#[test]
fn a_narrow_bar_keeps_the_version_and_the_key() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 56, 24);
    offer(&mut h, Ok(Mode::Binary));
    let s = h.screen();
    assert!(s.contains("↑ 0.7.0 · Alt+u"), "{s}");
    assert!(s.contains("3 items"), "the count keeps its place: {s}");
}

#[test]
fn clicking_the_notice_or_pressing_its_key_opens_the_notes() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 36);
    offer(&mut h, Ok(Mode::Binary));
    h.click(&Target::Act(Action::Update));
    let s = h.screen();
    assert!(matches!(h.app.modal, Some(Modal::Update(_))), "{s}");
    assert!(s.contains("rada 0.7.0 is available"), "{s}");
    assert!(s.contains("What is new"), "{s}");
    assert!(s.contains("Tags in the Finder's colours"), "{s}");
    // Long lines are wrapped, not cut.
    assert!(s.contains("window and not cut at its edge"), "{s}");
    assert!(s.contains("Update now"), "{s}");
    assert!(s.contains("replace this one"), "{s}");
    h.key(crossterm::event::KeyCode::Esc);
    assert!(h.app.modal.is_none());
    // Later, by key.
    h.press("alt+u");
    assert!(matches!(
        h.app.modal,
        Some(Modal::Update(UpdateView::Offer { .. }))
    ));
}

#[test]
fn a_program_of_a_package_manager_is_told_what_to_run_and_cannot_update_itself() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 36);
    offer(
        &mut h,
        Ok(Mode::Package {
            hint: "Homebrew installed it: run `brew upgrade rada`.".into(),
        }),
    );
    h.press("alt+u");
    let s = h.screen();
    assert!(s.contains("brew upgrade rada"), "{s}");
    assert!(!s.contains("Update now"), "{s}");
    h.key(crossterm::event::KeyCode::Enter);
    assert!(
        matches!(h.app.modal, Some(Modal::Update(_))),
        "Enter must not start anything for a package"
    );
    assert!(matches!(h.app.update, UpdateStatus::Available { .. }));
    h.key(crossterm::event::KeyCode::Esc);
    assert!(h.app.modal.is_none());
}

#[test]
fn a_source_checkout_says_what_it_will_do() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 36);
    offer(
        &mut h,
        Ok(Mode::Source {
            dir: "/home/u/rada-fm".into(),
        }),
    );
    h.press("alt+u");
    let s = h.screen();
    assert!(s.contains("/home/u/rada-fm"), "{s}");
    assert!(s.contains("on main"), "{s}");
    assert!(s.contains("Update now"), "{s}");
}

#[test]
fn without_an_update_worker_the_key_says_so() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 30);
    h.press("alt+u");
    assert!(h.screen().contains("updates are not available here"));
    assert!(h.app.modal.is_none());
}

#[test]
fn progress_and_the_end_show_in_the_bar_and_the_interface_stays_usable() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 150, 30);
    offer(&mut h, Ok(Mode::Binary));
    h.app.on_core_event(CoreEvent::Update(UpdateEvent::Progress(
        "building: serde (12 compiled)".into(),
    )));
    let s = h.screen();
    assert!(s.contains("Updating: building: serde (12 compiled)"), "{s}");
    // The folder is still there to browse while it works.
    h.keys("j");
    assert_eq!(h.cursor_name(), "a.txt");
    h.press("alt+u");
    assert!(h.screen().contains("an update is already running"));

    h.app
        .on_core_event(CoreEvent::Update(UpdateEvent::Done(Applied {
            version: "0.7.0".into(),
            note: "x".into(),
            restart: true,
        })));
    let s = h.screen();
    assert!(
        s.contains("rada 0.7.0 installed — start rada again to use it"),
        "{s}"
    );
}

#[test]
fn a_failed_update_explains_itself_and_offers_the_log() {
    let (sb, dir) = sandbox_with_files();
    let log = dir.join("last.log");
    std::fs::write(&log, "the log").unwrap();
    let mut h = H::new(sb, dir, 140, 36);
    h.app.on_core_event(CoreEvent::Update(UpdateEvent::Failed {
        message: "the build failed; the program that was running is untouched.".into(),
        log: log.clone(),
    }));
    let s = h.screen();
    assert!(
        matches!(h.app.modal, Some(Modal::Update(UpdateView::Failed { .. }))),
        "{s}"
    );
    assert!(s.contains("The update did not work"), "{s}");
    assert!(s.contains("untouched"), "{s}");
    assert!(s.contains("Open the log"), "{s}");
    h.key(crossterm::event::KeyCode::Esc);
    // It stays in the bar so it can be looked at again.
    assert!(
        h.screen()
            .contains("The update did not work — press Alt+u to see why")
    );
    h.press("alt+u");
    assert!(matches!(
        h.app.modal,
        Some(Modal::Update(UpdateView::Failed { .. }))
    ));
}

#[test]
fn accepting_starts_the_worker_and_a_worker_that_cannot_work_ends_in_the_failure_window() {
    let (sb, dir) = sandbox_with_files();
    let state = sb.mkdir("state/rada");
    // No release list anywhere: a closed local port, so the answer is immediate and no real
    // network is ever involved.
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let api = format!("http://{}/releases", dead.local_addr().unwrap());
    drop(dead);
    let ctx = Ctx {
        current: Version::parse("0.6.0-rc.1").unwrap(),
        settings: Settings {
            check: false,
            channel: Channel::Auto,
            method: Method::Binary,
            source_dir: None,
        },
        api_url: api,
        exe: sb.path("bin/rada"),
        state_dir: state,
        home: sb.path("home"),
        target: Some("x86_64-unknown-linux-musl".into()),
        user_agent: "rada/test".into(),
        os_id: None,
        git: "git".into(),
        cargo: "cargo".into(),
        run_check: false,
        extra_env: Vec::new(),
    };
    let mut h = H::new(sb, dir, 140, 36);
    h.app.start_updates(ctx);
    offer(&mut h, Ok(Mode::Binary));
    h.press("alt+u");
    h.key(crossterm::event::KeyCode::Enter);
    assert!(h.app.modal.is_none());
    assert!(
        matches!(h.app.update, UpdateStatus::Working { .. }),
        "the bar says it is working at once"
    );
    h.wait("the failure", |a| {
        matches!(a.update, UpdateStatus::Failed { .. })
    });
    let s = h.screen();
    assert!(
        matches!(h.app.modal, Some(Modal::Update(UpdateView::Failed { .. }))),
        "{s}"
    );
    assert!(s.contains("cannot reach"), "{s}");
    let _ = Duration::from_secs(0);
}
