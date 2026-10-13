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

// ------------------------------------------------------------------ visible updates

use rada_core::testweb::Server;
use rada_tui::app::MenuKind;

/// A release list with the given tags (each with the files a release has), served from this
/// machine, and a context that reads it.
fn worker_with_releases(
    sb: &rada_core::testutil::Sandbox,
    tags: &[&str],
    current: &str,
) -> (Server, Ctx) {
    let server = Server::start();
    let target = "x86_64-unknown-linux-musl";
    let items: Vec<String> = tags
        .iter()
        .map(|tag| {
            let v = Version::parse(tag).unwrap();
            let a = rada_core::update::release::archive_name(&v, target);
            format!(
                r#"{{"tag_name":"{tag}","prerelease":{},"draft":false,"body":"Notes for {tag}","assets":[
                {{"name":"{a}","browser_download_url":"{}","size":1}},
                {{"name":"SHA256SUMS","browser_download_url":"{}","size":1}}]}}"#,
                v.is_prerelease(),
                server.url(&format!("/files/{tag}/{a}")),
                server.url(&format!("/files/{tag}/SHA256SUMS")),
            )
        })
        .collect();
    server.route("/releases", 200, format!("[{}]", items.join(",")));
    let ctx = Ctx {
        current: Version::parse(current).unwrap(),
        settings: Settings {
            check: false,
            channel: Channel::Auto,
            method: Method::Binary,
            source_dir: None,
        },
        api_url: server.url("/releases"),
        exe: sb.path("bin/rada"),
        state_dir: sb.mkdir("state/rada"),
        home: sb.path("home"),
        target: Some(target.into()),
        user_agent: "rada/test".into(),
        os_id: None,
        git: "git".into(),
        cargo: "cargo".into(),
        run_check: false,
        extra_env: Vec::new(),
    };
    (server, ctx)
}

#[test]
fn a_newer_version_is_a_pill_in_the_tab_bar_that_opens_the_notes() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 30);
    let top = |h: &mut H| h.screen().lines().next().unwrap().to_string();
    assert!(!top(&mut h).contains("Update to"), "no pill without news");
    offer(&mut h, Ok(Mode::Binary));
    let first = top(&mut h);
    assert!(first.contains("↑ Update to 0.7.0"), "{first}");
    // It is in the accent colour, like the other pills.
    let buf = h.term.backend().buffer().clone();
    let x = (0..buf.area.width)
        .find(|&x| buf[(x, 0)].symbol() == "↑")
        .expect("the arrow");
    assert_eq!(buf[(x, 0)].bg, h.app.th.accent);
    // A click is the same as the key.
    h.click(&Target::Act(Action::Update));
    let s = h.screen();
    assert!(
        matches!(h.app.modal, Some(Modal::Update(UpdateView::Offer { .. }))),
        "{s}"
    );
    assert!(s.contains("rada 0.7.0 is available"), "{s}");
}

#[test]
fn the_version_is_in_the_status_bar_and_a_click_asks_for_a_newer_one() {
    let (sb, dir) = sandbox_with_files();
    let (server, ctx) = worker_with_releases(&sb, &["0.6.0"], "0.6.0");
    let mut h = H::new(sb, dir, 140, 30);
    let shown = format!("v{} ⟳", h.app.version());
    assert!(h.screen().contains(&shown), "{}", h.screen());
    // Without the worker it says so.
    h.click(&Target::CheckUpdates);
    assert!(h.screen().contains("updates are not available here"));
    // With it, the network is asked at once (a day's rest does not count) and the answer is
    // a plain sentence.
    h.app.start_updates(ctx);
    assert_eq!(h.app.version(), "0.6.0");
    h.app.clear_notices();
    assert!(h.screen().contains("v0.6.0 ⟳"), "{}", h.screen());
    h.click(&Target::CheckUpdates);
    h.wait("the answer", |a| {
        a.current_notice()
            .is_some_and(|t| t.text == "rada is up to date")
    });
    assert!(h.app.modal.is_none());
    let asked = server.requests().len();
    assert!(asked >= 1, "the list was asked for");
    // And again: the second click asks again, it is not remembered for a day.
    h.click(&Target::CheckUpdates);
    h.wait("a second request", |_| server.requests().len() > asked);
}

#[test]
fn a_check_that_finds_something_opens_the_notes() {
    let (sb, dir) = sandbox_with_files();
    let (_server, ctx) = worker_with_releases(&sb, &["0.6.0", "0.7.0"], "0.6.0");
    let mut h = H::new(sb, dir, 140, 36);
    h.app.start_updates(ctx);
    h.click(&Target::CheckUpdates);
    h.wait("the notes", |a| matches!(a.modal, Some(Modal::Update(_))));
    let s = h.screen();
    assert!(s.contains("rada 0.7.0 is available"), "{s}");
    assert!(s.contains("Notes for 0.7.0"), "{s}");
    // The pill is there once the window is closed.
    h.key(crossterm::event::KeyCode::Esc);
    assert!(
        h.screen()
            .lines()
            .next()
            .unwrap()
            .contains("↑ Update to 0.7.0")
    );
}

#[test]
fn a_failed_check_says_so_when_somebody_asked() {
    let (sb, dir) = sandbox_with_files();
    let (server, ctx) = worker_with_releases(&sb, &["0.6.0"], "0.6.0");
    server.route("/releases", 500, "no");
    let mut h = H::new(sb, dir, 140, 30);
    h.app.start_updates(ctx);
    h.click(&Target::CheckUpdates);
    h.wait("the failure", |a| {
        a.current_notice()
            .is_some_and(|t| t.text.starts_with("cannot look for updates"))
    });
}

#[test]
fn the_help_button_is_a_menu_with_the_keys_the_check_and_about() {
    let (sb, dir) = sandbox_with_files();
    let (_server, ctx) = worker_with_releases(&sb, &["0.6.0-rc.4"], "0.6.0-rc.4");
    let mut h = H::new(sb, dir, 140, 36);
    h.app.start_updates(ctx);
    h.click(&Target::Menu(MenuKind::Help));
    let s = h.screen();
    for needle in ["Help", "Check for updates", "About rada"] {
        assert!(s.contains(needle), "{needle}: {s}");
    }
    // About: the version, the channel it follows and how it would be updated.
    let Some(Modal::Menu(m)) = &h.app.modal else {
        panic!("a menu: {s}")
    };
    let i = m
        .items
        .iter()
        .position(|i| i.label == "About rada")
        .unwrap();
    h.click(&Target::MenuItem(i));
    let s = h.screen();
    assert!(matches!(h.app.modal, Some(Modal::Properties(_))), "{s}");
    assert!(s.contains("About rada"), "{s}");
    assert!(s.contains("0.6.0-rc.4"), "{s}");
    assert!(s.contains("pre-releases (chosen from the version)"), "{s}");
    assert!(s.contains("downloaded from the releases"), "{s}");
    h.key(crossterm::event::KeyCode::Esc);
    // Check for updates, from the menu.
    h.click(&Target::Menu(MenuKind::Help));
    let Some(Modal::Menu(m)) = &h.app.modal else {
        panic!("a menu")
    };
    let i = m
        .items
        .iter()
        .position(|i| i.label == "Check for updates")
        .unwrap();
    h.click(&Target::MenuItem(i));
    h.wait("the answer", |a| {
        a.current_notice()
            .is_some_and(|t| t.text == "rada is up to date")
    });
    // The first entry is still the keys, which F1 and ? open directly.
    h.press("F1");
    assert!(matches!(h.app.modal, Some(Modal::Help)));
}
