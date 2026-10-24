//! The plan of a rename onto an existing name asks what to do, as a paste does.

mod common;

use common::*;
use rada_core::ops::ConflictPolicy;
use rada_core::testutil::*;
use rada_tui::app::Modal;
use rada_tui::hits::Target;

#[test]
fn choosing_replace_in_the_plan_renames_over_it() {
    let sb = Sandbox::new();
    sb.write("w/a.txt", "new");
    sb.write("w/b.txt", "old");
    let dir = sb.path("w");
    let mut h = H::new(sb, dir.clone(), 120, 30);
    h.press("F2");
    h.press("ctrl+u");
    h.keys("b.txt");
    h.press("enter");
    plan_modal(&mut h);
    h.press("enter");
    assert!(matches!(h.app.modal, Some(Modal::Plan(_))), "nothing runs");
    h.click(&Target::Policy(ConflictPolicy::Overwrite));
    h.wait("replanned", |a| match &a.modal {
        Some(Modal::Plan(pv)) => pv.plan.is_executable() && pv.replanning.is_none(),
        _ => false,
    });
    h.press("enter");
    h.wait("done", |a| a.ops.running.is_none() && a.modal.is_none());
    assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "new");
}
