//! Windows on a terminal that is hardly bigger than they are: nothing panics, the buttons are
//! on the screen, and whatever answers a click lies on cells that were drawn.

mod common;

use common::*;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use rada_core::events::{CoreEvent, FailureInfo};
use rada_core::i18n::Key;
use rada_core::model::SortKey;
use rada_core::ops::{Cancel, ErrorChoice};
use rada_core::update::{Mode, Notice as Release, UpdateEvent};
use rada_tui::app::{
    EjectStage, EjectView, FontOffer, FontStage, FontView, Modal, OpenWithView, ResultView,
    ToastKind,
};
use rada_tui::hits::Target;

/// The smallest the interface draws, and the sizes a person is likely to have.
const SIZES: [(u16, u16); 7] = [
    (24, 8),
    (40, 12),
    (40, 14),
    (40, 16),
    (60, 20),
    (80, 24),
    (100, 30),
];

fn scene(w: u16, h: u16) -> H {
    let sb = rada_core::testutil::Sandbox::new();
    for n in ["a.txt", "b.txt", "c.txt"] {
        sb.write(format!("w/{n}"), n);
    }
    sb.write("dest/a.txt", "old");
    let dir = sb.path("w");
    H::new(sb, dir, w, h)
}

/// A window to look at, and the buttons it must show.
struct Case {
    name: &'static str,
    open: fn(&mut H),
    buttons: fn() -> Vec<&'static str>,
}

fn plan_copy(h: &mut H) {
    h.press("ctrl+a");
    h.press("ctrl+c");
    h.press("ctrl+v");
    plan_modal(h);
}

fn plan_delete(h: &mut H) {
    h.press("ctrl+a");
    h.keys("D");
    plan_modal(h);
}

fn failure(h: &mut H, journal: bool) {
    let (reply, rx) = crossbeam_channel::bounded::<ErrorChoice>(1);
    std::mem::forget(rx);
    h.app.windows.modal = Some(Modal::Failure {
        job: 1,
        info: FailureInfo {
            index: 2,
            description: "copy".into(),
            message: "cannot write b.txt: no space left on device".into(),
            path: "b".into(),
            attempt: 1,
            journal,
            denied: None,
        },
        reply,
    });
}

fn scanning(h: &mut H) {
    h.app.windows.modal = Some(Modal::Scanning {
        job: 1,
        what: "Copying 3 items".into(),
        files: 120,
        dirs: 9,
        bytes: 4_000_000,
        current: "/home/user/projects/a/very/long/path/to/some/file.txt".into(),
        cancel: Cancel::new(),
    });
}

fn update(h: &mut H) {
    let notes = "### Added\n- Tags in the Finder's colours\n- A long line that goes on and on to see that the notes are wrapped inside the window\n\n### Fixed\n- Something";
    h.app
        .on_core_event(CoreEvent::Update(UpdateEvent::Available {
            notice: Release {
                version: "0.7.0".into(),
                tag: "v0.7.0".into(),
                prerelease: false,
                notes: notes.into(),
            },
            how: Ok(Mode::Binary),
        }));
    h.press("alt+u");
}

fn history(h: &mut H) {
    h.press("F3");
    h.wait(
        "the history",
        |a| matches!(&a.windows.modal, Some(Modal::History(v)) if !v.loading),
    );
}

fn menu(h: &mut H) {
    let (x, y) = h.where_is(&Target::Row(h.app.tab.sel.cursor));
    h.mouse(
        MouseEventKind::Down(MouseButton::Right),
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
}

fn palette(h: &mut H) {
    let items = Vec::new();
    h.app.windows.modal = Some(Modal::Palette(Box::new(
        rada_tui::palette::PaletteView::new(items, "/home/user".into()),
    )));
}

fn result(h: &mut H) {
    let lines = vec![
        (
            ToastKind::Error,
            "cannot copy b.txt: permission denied while reading the file".into(),
        ),
        (ToastKind::Warn, "2 items were skipped".into()),
    ];
    h.app.windows.modal = Some(Modal::Result(ResultView {
        title: "Copy finished with problems".into(),
        lines,
        scroll: 0,
    }));
}

fn access(h: &mut H) {
    h.app.windows.modal = Some(Modal::Access(rada_core::platform::AccessDenied {
        who: "Terminal".into(),
        folder: "/Users/me/Documents".into(),
        folder_name: "Documents".into(),
        settings_url: "x-apple.systempreferences:files",
        full_disk_url: "x-apple.systempreferences:disk",
    }));
}

fn eject(h: &mut H, stage: EjectStage) {
    let volume = rada_core::platform::Volume {
        mount_point: "/run/media/me/STICK".into(),
        label: Some("STICK".into()),
        fs_type: "vfat".into(),
        device: "/dev/sdb1".into(),
        kind: rada_core::platform::VolumeKind::Removable,
        drive_letter: None,
        total: Some(8_000_000_000),
        available: Some(1_000_000_000),
        read_only: false,
        responsive: true,
    };
    h.app.windows.modal = Some(Modal::Eject(EjectView {
        name: "STICK".into(),
        volume,
        stage,
    }));
}

fn font(h: &mut H) {
    let offer = FontOffer {
        family: "JetBrainsMono Nerd Font".into(),
        megabytes: 92.0,
        install: std::sync::Arc::new(|| Ok("done".into())),
        shown: std::sync::Arc::new(|| {}),
    };
    h.app.windows.modal = Some(Modal::Font(FontView {
        offer,
        stage: FontStage::Ask,
    }));
}

fn open_with(h: &mut H) {
    let app = |id: &str, name: &str, is_default: bool| rada_core::platform::OpenApp {
        id: id.into(),
        name: name.into(),
        exec: "x".into(),
        terminal: false,
        is_default,
        source: None,
        icon: None,
    };
    h.app.windows.modal = Some(Modal::OpenWith(Box::new(OpenWithView {
        path: "/tmp/a.txt".into(),
        file: "a.txt".into(),
        kind: Some("text/plain".into()),
        apps: vec![
            app("a.desktop", "Text Editor", true),
            app("b.desktop", "Vim", false),
        ],
        always_id: None,
        loading: false,
        filter: String::new(),
        cursor: 0,
        selected: 0,
        always: false,
        under: None,
    })));
}

fn window_cases() -> Vec<Case> {
    vec![
        Case {
            name: "plan (copy)",
            open: plan_copy,
            buttons: || vec![Key::BtnCancel.tr(), "Copy"],
        },
        Case {
            name: "plan (delete)",
            open: plan_delete,
            buttons: || vec![Key::BtnCancel.tr(), "Delete"],
        },
        Case {
            name: "help",
            open: |h| h.key(KeyCode::F(1)),
            buttons: Vec::new,
        },
        Case {
            name: "palette",
            open: palette,
            buttons: Vec::new,
        },
        Case {
            name: "failure (journal)",
            open: |h| failure(h, true),
            buttons: || vec![Key::BtnAbort.tr(), Key::BtnContinueNoUndo.tr()],
        },
        Case {
            name: "failure (step)",
            open: |h| failure(h, false),
            buttons: || vec![Key::BtnSkip.tr(), Key::BtnRetry.tr(), Key::BtnAbort.tr()],
        },
        Case {
            name: "input (rename)",
            open: |h| h.press("f2"),
            buttons: || vec![Key::BtnCancel.tr()],
        },
        Case {
            name: "menu",
            open: menu,
            buttons: Vec::new,
        },
    ]
}

fn busy_cases() -> Vec<Case> {
    vec![
        Case {
            name: "scanning",
            open: scanning,
            buttons: Vec::new,
        },
        Case {
            name: "quit anyway",
            open: |h| h.app.windows.modal = Some(Modal::ConfirmQuit),
            buttons: || vec![Key::BtnQuitAnyway.tr(), Key::BtnStay.tr()],
        },
        Case {
            name: "properties",
            open: |h| h.press("alt+enter"),
            buttons: || vec![Key::BtnClose.tr()],
        },
        Case {
            name: "history",
            open: history,
            buttons: || vec![Key::BtnClose.tr()],
        },
        Case {
            name: "update",
            open: update,
            buttons: || vec![Key::BtnUpdateNow.tr(), Key::BtnLater.tr()],
        },
    ]
}

fn dialog_cases() -> Vec<Case> {
    vec![
        Case {
            name: "result",
            open: result,
            buttons: || vec![Key::BtnUndoDone.tr(), Key::BtnClose.tr()],
        },
        Case {
            name: "no access",
            open: access,
            buttons: || {
                vec![
                    Key::BtnOpenFilesSettings.tr(),
                    Key::BtnFullDisk.tr(),
                    Key::BtnClose.tr(),
                ]
            },
        },
        Case {
            name: "eject",
            open: |h| eject(h, EjectStage::Confirm),
            buttons: || vec![Key::BtnEject.tr(), Key::BtnCancel.tr()],
        },
        Case {
            name: "eject (failed)",
            open: |h| eject(h, EjectStage::Failed("the disk is busy".into())),
            buttons: || vec![Key::BtnTryAgain.tr(), Key::BtnClose.tr()],
        },
        Case {
            name: "font",
            open: font,
            buttons: || vec![Key::BtnInstallFont.tr(), Key::BtnNotNow.tr()],
        },
        Case {
            name: "open with",
            open: open_with,
            buttons: Vec::new,
        },
    ]
}

fn cases() -> Vec<Case> {
    let mut all = window_cases();
    all.extend(busy_cases());
    all.extend(dialog_cases());
    all
}

/// Whether `rect` is on the screen and, for a one-row target, has something drawn in it.
fn undrawn(buf: &ratatui::buffer::Buffer, rect: ratatui::layout::Rect) -> bool {
    let area = buf.area;
    if rect.right() > area.width || rect.bottom() > area.height {
        return true;
    }
    rect.height == 1 && (rect.x..rect.right()).all(|x| buf[(x, rect.y)].symbol().trim().is_empty())
}

/// What is wrong with the window `case` at `w`x`h`, one sentence for each thing.
fn problems(case: &Case, w: u16, h: u16) -> Vec<String> {
    let mut hh = scene(w, h);
    (case.open)(&mut hh);
    let buf = hh.redrawn_buffer();
    let text = hh.screen();
    let mut out = Vec::new();
    for b in (case.buttons)() {
        // A narrow window cuts a long label with an ellipsis; its beginning must show.
        let shown: String = if w >= 60 {
            b.into()
        } else {
            b.chars().take(6).collect()
        };
        if !text.contains(&shown) {
            out.push(format!("the button \"{b}\" is not on the screen"));
        }
    }
    for (rect, target) in hh.app.frame.hits.all() {
        if undrawn(&buf, *rect) {
            out.push(format!(
                "{target:?} at {rect:?} lies on cells that were not drawn"
            ));
        }
    }
    out
}

#[test]
fn every_window_fits_every_size_with_its_buttons_and_its_clicks() {
    let mut all = Vec::new();
    for case in cases() {
        for (w, h) in SIZES {
            let run = std::panic::catch_unwind(|| problems(&case, w, h));
            match run {
                Err(_) => all.push(format!("{} at {w}x{h}: panicked", case.name)),
                Ok(list) => all.extend(
                    list.into_iter()
                        .map(|p| format!("{} at {w}x{h}: {p}", case.name)),
                ),
            }
        }
    }
    assert!(
        all.is_empty(),
        "{} problems:\n{}",
        all.len(),
        all.join("\n")
    );
}

fn notify_three(h: &mut H) {
    use rada_tui::app::{Notice, NoticeAction, NoticeKind};
    h.app
        .notify(Notice::new(NoticeKind::Error, "cannot rename setup.sh"));
    h.app.notify(
        Notice::new(NoticeKind::Ok, "3 items moved to the trash").with_action(NoticeAction::Undo),
    );
    h.app
        .notify(Notice::new(NoticeKind::Warn, "2 items were skipped"));
}

#[test]
fn three_notifications_never_take_the_whole_list() {
    for (w, h) in SIZES {
        let run = std::panic::catch_unwind(|| {
            let mut hh = scene(w, h);
            notify_three(&mut hh);
            let buf = hh.redrawn_buffer();
            let text = hh.screen();
            let rows = hh.app.frame.hits.find(&Target::Row(0)).is_some();
            let card = text.contains("Cannot rename");
            let undrawn: Vec<_> = hh
                .app
                .frame
                .hits
                .all()
                .filter(|(r, t)| matches!(t, Target::Notice(..)) && undrawn(&buf, *r))
                .map(|(r, t)| format!("{t:?} at {r:?}"))
                .collect();
            (rows, card, undrawn, text)
        });
        let Ok((rows, card, undrawn, text)) = run else {
            panic!("{w}x{h}: panicked");
        };
        assert!(undrawn.is_empty(), "{w}x{h}: {undrawn:?}\n{text}");
        if w >= 40 && h >= 16 {
            assert!(rows, "{w}x{h}: the list has no row left\n{text}");
            assert!(card, "{w}x{h}: no card is visible\n{text}");
        }
    }
}

#[test]
fn the_titles_of_the_columns_answer_no_click_under_the_preview_drawn_over_them() {
    let mut h = scene(60, 20);
    let buf = h.redrawn_buffer();
    let name = h
        .app
        .frame
        .hits
        .find(&Target::SortBy(SortKey::Name))
        .unwrap();
    // The pane starts at its rule, between the title of the names and the edge of the box.
    let pane = (name.x + 1..buf.area.width - 1)
        .find(|x| buf[(*x, name.y)].symbol() == "│")
        .expect("the preview over the list");
    // Left of the pane the title of the names sorts.
    let click = |h: &mut H, x: u16| {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            h.mouse(kind, x, name.y, crossterm::event::KeyModifiers::NONE);
        }
    };
    let sort = h.app.tab.sort;
    click(&mut h, name.x);
    assert_ne!(h.app.tab.sort, sort);
    let sort = h.app.tab.sort;
    for x in pane..buf.area.width - 1 {
        assert!(
            !matches!(h.app.frame.hits.at(x, name.y), Some(Target::SortBy(_))),
            "a title answers at {x},{} under the preview",
            name.y
        );
        click(&mut h, x);
    }
    assert_eq!(h.app.tab.sort, sort, "the order did not change");
}

#[test]
fn open_with_shows_its_title_and_the_program_chosen_at_every_size() {
    for (w, hh) in SIZES {
        let mut h = scene(w, hh);
        open_with(&mut h);
        let text = h.screen();
        // A narrow card cuts the title with an ellipsis; its beginning must show.
        assert!(text.contains("Open"), "{w}x{hh}: no title\n{text}");
        assert!(text.contains("Tex"), "{w}x{hh}: no program\n{text}");
        assert!(
            h.app.frame.hits.find(&Target::OpenWithRow(0)).is_some(),
            "{w}x{hh}: the program does not answer a click"
        );
    }
}
