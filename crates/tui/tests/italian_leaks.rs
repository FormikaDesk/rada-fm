//! No English leaks into the Italian interface: the windows and menus of every kind are drawn
//! in Italian and none of the words the English interface uses for itself is on the screen.

mod common;

use std::sync::Mutex;
use std::time::Duration;

use common::*;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use rada_core::i18n::{Lang, set_lang};
use rada_tui::app::{Modal, NoticeAction, NoticeKind};
use rada_tui::hits::Target;

/// Words and phrases of the English interface. File names in a sandbox are not among them (they
/// are lower case, or have a dot).
const ENGLISH: &[&str] = &[
    "Cancel",
    "Close",
    "Undo",
    "Redo",
    "Open with",
    "Copy",
    "Cut",
    "Paste",
    "Delete",
    "Rename",
    "Move to",
    "Run  ",
    " Run ",
    "Tab buttons",
    "scroll",
    "You can undo",
    "will be",
    "Search files",
    "Sort by",
    "Date modified",
    "Details",
    "Permissions",
    "Properties",
    "items",
    " item ",
    "selected",
    "Keep both",
    "Overwrite",
    "already",
    "Show",
    "new folder",
    "Type ",
    "from ",
];

/// Run `f` with the Italian interface on for the whole process (the plans are made by worker
/// threads, which a thread-local language would not reach), one test at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct English;
impl Drop for English {
    fn drop(&mut self) {
        set_lang(Lang::En);
    }
}

fn in_italian(f: impl FnOnce()) {
    let _only = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let _back = English;
    set_lang(Lang::It);
    f();
}

fn leaks(h: &mut H, what: &str) {
    let s = h.screen();
    for w in ENGLISH {
        assert!(!s.contains(w), "{what}: {w:?} is English:\n{s}");
    }
}

fn sandbox() -> (H, std::path::PathBuf, std::path::PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    for n in ["a.txt", "b.txt", "c.txt"] {
        sb.write(format!("w/{n}"), n);
    }
    sb.write("Documenti/a.txt", "old");
    let dest = sb.path("Documenti");
    let dir = sb.path("w");
    let h = H::new(sb, dir.clone(), 150, 40);
    (h, dir, dest)
}

fn right_click_row(h: &mut H) {
    let (x, y) = h.where_is(&Target::Row(h.app.cursor));
    h.mouse(
        MouseEventKind::Down(MouseButton::Right),
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
}

#[test]
fn the_screen_the_menus_and_their_submenus_speak_italian() {
    in_italian(|| {
        let (mut h, _dir, _dest) = sandbox();
        h.keys("j");
        leaks(&mut h, "the folder");
        right_click_row(&mut h);
        leaks(&mut h, "the menu of an item");
        h.key(KeyCode::Esc);
        h.click(&Target::Menu(rada_tui::app::MenuKind::Sort));
        leaks(&mut h, "the sort menu");
        h.key(KeyCode::Esc);
        h.click(&Target::Menu(rada_tui::app::MenuKind::View));
        leaks(&mut h, "the view menu");
        h.key(KeyCode::Esc);
        h.click(&Target::Menu(rada_tui::app::MenuKind::New));
        leaks(&mut h, "the new menu");
        h.key(KeyCode::Esc);
        h.key(KeyCode::F(1));
        leaks(&mut h, "the help");
        assert!(matches!(h.app.windows.modal, Some(Modal::Help)));
        h.key(KeyCode::Esc);
    });
}

#[test]
fn the_plan_windows_speak_italian() {
    in_italian(|| {
        let (mut h, dir, dest) = sandbox();
        // A copy that meets a file of the same name.
        h.press("ctrl+a");
        h.press("ctrl+c");
        h.app.open_dir(dest.clone());
        let d = dest.clone();
        h.wait("the destination", move |a| a.cwd == d && !a.is_loading());
        h.press("ctrl+v");
        plan_modal(&mut h);
        leaks(&mut h, "the copy plan");
        let s = h.screen();
        assert!(s.contains("Copia 3 elementi in Documenti"), "{s}");
        assert!(
            s.contains("Tieni entrambi") && s.contains("esiste già"),
            "{s}"
        );
        h.key(KeyCode::Esc);
        // The trash, and a permanent deletion.
        h.app.open_dir(dir.clone());
        let d = dir.clone();
        h.wait("back", move |a| a.cwd == d && !a.is_loading());
        h.keys("j");
        h.press("delete");
        plan_modal(&mut h);
        leaks(&mut h, "the trash plan");
        assert!(h.screen().contains("Spostare 1 elemento nel cestino?"));
        h.key(KeyCode::Esc);
        h.keys("D");
        plan_modal(&mut h);
        leaks(&mut h, "the plan of a permanent deletion");
        assert!(h.screen().contains("Eliminare definitivamente 1 elemento?"));
        h.key(KeyCode::Esc);
        // A rename, and its undo.
        h.press("f2");
        h.keys("z");
        h.key(KeyCode::Enter);
        plan_modal(&mut h);
        leaks(&mut h, "the rename plan");
        run_plan_and_wait(&mut h);
        h.key(KeyCode::Esc);
        h.press("ctrl+z");
        plan_modal(&mut h);
        leaks(&mut h, "the undo plan");
        let s = h.screen();
        assert!(s.contains("Annullare la rinomina di 1 elemento?"), "{s}");
        assert!(
            s.contains("Annulla la rinomina") && s.contains("Chiudi"),
            "{s}"
        );
        assert!(s.contains("torna al suo posto"), "{s}");
    });
}

#[test]
fn the_cards_speak_italian() {
    in_italian(|| {
        let (mut h, _dir, dest) = sandbox();
        h.press("ctrl+a");
        h.press("ctrl+c");
        h.app.open_dir(dest.clone());
        let d = dest.clone();
        h.wait("the destination", move |a| a.cwd == d && !a.is_loading());
        h.press("ctrl+v");
        plan_modal(&mut h);
        h.key(KeyCode::Right);
        run_plan_and_wait(&mut h);
        leaks(&mut h, "the card of a copy");
        h.app.notify(
            rada_tui::app::Notice::new(NoticeKind::Ok, "3 elementi spostati nel cestino")
                .with_action(NoticeAction::Undo),
        );
        h.app.advance_notices(Duration::from_millis(1));
        leaks(&mut h, "two cards");
    });
}
