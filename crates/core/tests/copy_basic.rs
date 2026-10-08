use vela_core::ops::*;
use vela_core::testutil::*;

fn copy_all(sb: &Sandbox, srcs: &[std::path::PathBuf], dest: &std::path::Path, policy: ConflictPolicy) -> (Plan, ExecReport) {
    let e = sb.engine();
    let scan = scan(&e, srcs);
    let plan = e.plan_transfer(&scan, dest, &TransferOptions::copy(policy));
    let rep = run(&e, &plan);
    (plan, rep)
}

#[test]
fn copies_a_tree_with_files_dirs_and_modes() {
    let sb = Sandbox::new();
    sb.write("src/a.txt", "hello");
    sb.write("src/sub/b.bin", vec![7u8; 3 << 20]);
    sb.write("src/empty/.keep", "");
    std::fs::remove_file(sb.path("src/empty/.keep")).unwrap();
    chmod(&sb.path("src/a.txt"), 0o640);
    let dest = sb.mkdir("dest");
    let (plan, rep) = copy_all(&sb, &[sb.path("src")], &dest, ConflictPolicy::Skip);
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(snapshot(&sb.path("src")), snapshot(&dest.join("src")));
    assert_eq!(plan.totals.files, 2);
    assert_eq!(plan.totals.bytes, 5 + (3 << 20));
}
