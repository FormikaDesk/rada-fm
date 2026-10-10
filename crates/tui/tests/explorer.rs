mod common;
use common::*;

#[test]
fn print_scene() {
    for (w, hh) in [
        (140u16, 35u16),
        (120, 30),
        (100, 26),
        (80, 24),
        (60, 20),
        (40, 16),
    ] {
        let mut h = demo_scene(w, hh);
        h.wait("photo", |a| a.current().is_some());
        println!("===== {w}x{hh}\n{}", h.screen());
    }
}
