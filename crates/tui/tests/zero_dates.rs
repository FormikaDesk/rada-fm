//! A date of zero (the start of 1970) is what a filesystem gives when it has no date, as a phone's
//! often does: the list and the details say "—", never 01/01/1970, whatever the filesystem.

mod common;

use std::time::{Duration, SystemTime};

use common::*;
use rada_core::testutil::*;

#[test]
fn a_file_with_no_date_shows_a_dash_in_the_list_and_in_the_details_and_no_1970() {
    let sb = Sandbox::new();
    let dated = sb.write("d/dated.txt", "a");
    let nodate = sb.write("d/nodate.txt", "b");
    set_mtime(&dated, SystemTime::now() - Duration::from_secs(5 * 86_400));
    set_mtime(&nodate, SystemTime::UNIX_EPOCH);
    let dir = sb.path("d");
    let mut h = H::new(sb, dir, 150, 34);
    h.wait("the files", |a| a.listing.all().len() == 2);
    let screen = h.screen();
    assert!(!screen.contains("1970"), "{screen}");
    let row = |name: &str| {
        screen
            .lines()
            .find(|l| l.contains(name))
            .unwrap_or_else(|| panic!("{name} not in:\n{screen}"))
            .to_string()
    };
    assert!(row("nodate.txt").contains('—'), "{}", row("nodate.txt"));
    assert!(!row("dated.txt").contains('—'), "{}", row("dated.txt"));
    // The details of it.
    h.keys("j");
    h.wait("the details", |a| a.preview.name == "nodate.txt");
    let screen = h.screen();
    assert!(!screen.contains("1970"), "{screen}");
    let modified = screen
        .lines()
        .find(|l| l.contains("Modified"))
        .unwrap_or_else(|| panic!("the details have a Modified row:\n{screen}"));
    assert!(modified.contains('—'), "{modified}");
}
