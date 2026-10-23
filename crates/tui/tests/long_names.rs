//! A name too long for its column is cut in the middle, so that the end (and the extension)
//! stays visible: five files that differ only at the end do not all read the same.

mod common;

use common::*;
use rada_core::testutil::*;

const FILES: [&str; 3] = [
    "CV_Alessandro_Formica_Accenture.pdf",
    "CV_Alessandro_Formica_Deloitte.pdf",
    "CV_Alessandro_Formica_Capgemini.pdf",
];

fn scene(width: u16) -> H {
    let sb = Sandbox::new();
    for name in FILES {
        sb.write(format!("work/{name}"), "x");
    }
    sb.mkdir("work/Progetti_vecchi_di_famiglia_2019");
    let start = sb.path("work");
    H::new(sb, start, width, 30)
}

#[test]
fn the_list_keeps_the_start_and_the_end_of_a_long_name() {
    let mut h = scene(110);
    h.wait("the files", |a| !a.is_loading() && a.visible.len() == 4);
    let screen = h.screen();
    for cut in [
        "CV_Alessa…ccenture.pdf",
        "CV_Alessa…apgemini.pdf",
        "CV_Alessa…Deloitte.pdf",
        "Progetti_…amiglia_2019",
    ] {
        assert!(screen.contains(cut), "{cut}:\n{screen}");
    }
}

#[test]
fn a_name_that_fits_is_shown_whole() {
    let mut h = scene(170);
    h.wait("the files", |a| !a.is_loading() && a.visible.len() == 4);
    let screen = h.screen();
    for file in FILES {
        assert!(screen.contains(file), "{file}:\n{screen}");
    }
}
