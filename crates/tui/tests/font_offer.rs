//! The offer to install a Nerd Font: made once, never without being asked, and only on the
//! person's yes does anything get downloaded.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use crossterm::event::KeyCode;
use rada_tui::FontOffer;
use rada_tui::app::Modal;

fn offer(installs: &Arc<AtomicUsize>, shown: &Arc<AtomicUsize>, fail: bool) -> FontOffer {
    let (i, s) = (installs.clone(), shown.clone());
    FontOffer {
        family: "JetBrainsMono Nerd Font Mono".into(),
        megabytes: 9.9,
        install: Arc::new(move || {
            i.fetch_add(1, Ordering::SeqCst);
            if fail {
                Err("no network".into())
            } else {
                Ok("JetBrainsMono Nerd Font Mono is installed: choose it as the font of your terminal, then start rada again".into())
            }
        }),
        shown: Arc::new(move || {
            s.fetch_add(1, Ordering::SeqCst);
        }),
    }
}

fn with_offer(fail: bool) -> (H, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let (sb, dir) = sandbox_with_files();
    let (installs, shown) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let o = offer(&installs, &shown, fail);
    let h = H::with_config(sb, dir, 140, 36, move |c| c.font_offer = Some(o));
    (h, installs, shown)
}

#[test]
fn the_offer_explains_itself_and_notes_that_it_was_made() {
    let (mut h, installs, shown) = with_offer(false);
    assert!(matches!(h.app.modal, Some(Modal::Font(_))));
    let raw = h.screen();
    // The text is wrapped inside the window: compare it as running text, the window alone (its
    // left edge is a `││`, the pane's border and its own), so that what the sidebar has on the
    // same rows does not come between the words.
    let s = raw
        .lines()
        .filter_map(|l| l.split_once("││").map(|(_, window)| window))
        .map(|window| window.split('│').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        s.contains("Your terminal cannot draw rada's icons yet"),
        "{raw}"
    );
    assert!(s.contains("JetBrainsMono Nerd Font Mono"), "{raw}");
    assert!(s.contains("about 10 MB"), "{raw}");
    assert!(s.contains("checked against its checksums"), "{raw}");
    assert!(s.contains("asked only once"), "{raw}");
    assert!(
        s.contains("Install the font") && s.contains("Not now"),
        "{raw}"
    );
    assert_eq!(shown.load(Ordering::SeqCst), 1, "made once, noted at once");
    assert_eq!(
        installs.load(Ordering::SeqCst),
        0,
        "nothing happens by itself"
    );
}

#[test]
fn not_now_closes_it_and_installs_nothing() {
    let (mut h, installs, _) = with_offer(false);
    h.key(KeyCode::Esc);
    assert!(h.app.modal.is_none());
    h.pump(200);
    assert_eq!(installs.load(Ordering::SeqCst), 0);
}

#[test]
fn yes_installs_in_the_background_and_says_what_to_do_next() {
    let (mut h, installs, _) = with_offer(false);
    h.key(KeyCode::Enter);
    assert!(h.screen().contains("Downloading and installing"));
    h.wait("installed", |a| a.modal.is_none());
    assert_eq!(installs.load(Ordering::SeqCst), 1);
    // The card wraps what it says: read it as running text (the details pane, which shares
    // its rows, put away).
    h.app.details = Some(false);
    let raw = h.screen();
    let s = raw
        .replace(['│', '┃', '╭', '╮', '╰', '╯', '─', '━'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        s.contains("choose it as the font of your terminal"),
        "{raw}"
    );
}

#[test]
fn a_failure_is_said_and_the_offer_is_not_repeated() {
    let (mut h, installs, shown) = with_offer(true);
    h.key(KeyCode::Enter);
    h.wait("the failure", |a| a.modal.is_none());
    let s = h.screen();
    assert!(s.contains("The font was not installed: no network"), "{s}");
    assert_eq!(
        (
            installs.load(Ordering::SeqCst),
            shown.load(Ordering::SeqCst)
        ),
        (1, 1)
    );
}

#[test]
fn without_an_offer_there_is_no_window() {
    let (sb, dir) = sandbox_with_files();
    let h = H::new(sb, dir, 140, 36);
    assert!(h.app.modal.is_none());
}
