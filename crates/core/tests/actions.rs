//! Actions: a new file made by another program, or computed from files. What is published is
//! one file under a name that was free; whatever goes wrong, nothing else is left.
//!
//! The program is never a real one: it is this very test binary, started again to run the one
//! test `stand_in`, which plays the part its arguments name. The plan reaches it as `fake`, a
//! name the configuration of the test gives to this file.

#[path = "actions/journal_061.rs"]
mod journal_061;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
use rada_core::journal::EntryStatus;
use rada_core::journal::{Journal, RecordKind};
use rada_core::ops::*;
use rada_core::testutil::*;

const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

fn programs() -> Programs {
    let exe = std::env::current_exe().unwrap();
    Programs::new([("fake".to_string(), exe)].into())
}

/// A call of the stand-in playing `role`, with `rest` after it.
fn fake(role: &str, rest: &[Arg]) -> ProgramCall {
    let mut args: Vec<Arg> = ["stand_in", "--exact", "--ignored", "--nocapture"]
        .iter()
        .map(|s| Arg::Lit(s.to_string()))
        .collect();
    args.push(Arg::Lit(format!("role:{role}")));
    args.extend(rest.iter().cloned());
    ProgramCall {
        program: "fake".into(),
        args,
    }
}

fn lit(p: &Path) -> Arg {
    Arg::Lit(p.display().to_string())
}

fn by(chain: Vec<ProgramCall>) -> ProducerSpec {
    ProducerSpec::Program {
        chain,
        timeout_secs: 60,
        ok_exit: vec![0],
        collect: Collect::Exact,
    }
}

fn produce(input: &Path, output: &Path, producer: ProducerSpec) -> SubRequest {
    SubRequest::Produce(ProduceSpec {
        inputs: vec![input.to_path_buf()],
        output: output.to_path_buf(),
        producer,
        size_hint: None,
    })
}

fn recipe(requests: Vec<SubRequest>) -> OpRequest {
    OpRequest::Recipe(RecipePlan {
        recipe: "test".into(),
        requests,
        title: None,
        notes: Vec::new(),
        failure: FailurePolicy::Keep,
    })
}

/// The request that copies `input` to `output` with the stand-in.
fn copy_request(input: &Path, output: &Path) -> OpRequest {
    let call = fake("copy", &[Arg::Input(0), Arg::Out]);
    recipe(vec![produce(input, output, by(vec![call]))])
}

fn try_plan(e: &Engine, req: &OpRequest) -> rada_core::Result<Plan> {
    let cancel = Cancel::new();
    let ctl = ScanControl {
        cancel: cancel.flag(),
        progress: &mut |_| {},
    };
    e.plan_request(req, None, ctl).map(|p| p.plan)
}

fn plan_of(e: &Engine, req: &OpRequest) -> Plan {
    let plan = try_plan(e, req).unwrap_or_else(|err| panic!("{req:?}: {err}"));
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    plan
}

struct Scene {
    sb: Sandbox,
    e: Engine,
    j: Journal,
    input: PathBuf,
    out: PathBuf,
}

fn scene() -> Scene {
    let sb = Sandbox::new();
    let e = sb.engine().with_programs(programs());
    let j = sb.journal();
    let input = sb.write("in.txt", "hello");
    let out = sb.path("out.txt");
    Scene {
        sb,
        e,
        j,
        input,
        out,
    }
}

/// A scene whose filesystem can be made to fail.
fn faulty_scene() -> (Scene, Arc<FaultFs>) {
    let mut s = scene();
    let ffs = FaultFs::new();
    s.e = s.sb.engine_with(ffs.clone()).with_programs(programs());
    (s, ffs)
}

/// What an action must never leave: its work folders and temporary files.
fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".rada-"))
        .collect()
}

fn text(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

/// Run `chain` on the scene's input and return the one failure it must end with.
fn failure_of(s: &Scene, producer: ProducerSpec) -> String {
    let plan = plan_of(&s.e, &recipe(vec![produce(&s.input, &s.out, producer)]));
    let ran = run_journaled(&s.e, &s.j, &plan);
    assert_eq!(ran.report.failed.len(), 1, "{:?}", ran.report);
    assert!(!s.out.exists(), "nothing is published");
    assert_eq!(leftovers(&s.sb.work), Vec::<String>::new());
    ran.report.failed[0].error.clone()
}

#[test]
fn a_program_that_ends_well_gets_its_file_published() {
    let s = scene();
    let plan = plan_of(&s.e, &copy_request(&s.input, &s.out));
    assert_eq!(plan.kind, OpKind::Action);

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert_eq!(
        ran.report.status(),
        RunStatus::Completed,
        "{:?}",
        ran.report
    );
    assert_eq!(text(&s.out), "hello");
    assert_eq!(text(&s.input), "hello");
    assert_eq!(leftovers(&s.sb.work), Vec::<String>::new());
    let entry = s.j.entries().unwrap().pop().unwrap();
    assert_eq!(entry.kind, OpKind::Action);
    assert_eq!(entry.undo_steps, 1);
    assert!(entry.is_undoable());
}

#[test]
fn an_exit_code_that_is_not_allowed_publishes_nothing() {
    let s = scene();
    let call = fake("fail", &[Arg::Input(0), Arg::Out]);

    let error = failure_of(&s, by(vec![call.clone()]));

    assert!(error.contains("ended with code 3"), "{error}");
    assert!(error.contains("boom"), "{error}");

    // The same program, when the recipe says that code is fine.
    let allowed = ProducerSpec::Program {
        chain: vec![call],
        timeout_secs: 60,
        ok_exit: vec![0, 3],
        collect: Collect::Exact,
    };
    let plan = plan_of(&s.e, &recipe(vec![produce(&s.input, &s.out, allowed)]));
    let ran = run_journaled(&s.e, &s.j, &plan);
    assert!(ran.report.failed.is_empty(), "{:?}", ran.report);
    assert_eq!(text(&s.out), "hello");
}

#[test]
fn a_program_that_writes_nothing_or_an_empty_file_publishes_nothing() {
    let s = scene();
    let error = failure_of(&s, by(vec![fake("nothing", &[Arg::Out])]));
    assert!(error.contains("wrote no file"), "{error}");

    let error = failure_of(&s, by(vec![fake("empty", &[Arg::Out])]));
    assert!(error.contains("empty file"), "{error}");
}

#[test]
fn an_input_changed_since_the_plan_stops_the_program_from_starting() {
    let s = scene();
    let mark = s.sb.path("started");
    let call = fake("trace", &[lit(&mark), Arg::Input(0), Arg::Out]);
    let plan = plan_of(
        &s.e,
        &recipe(vec![produce(&s.input, &s.out, by(vec![call]))]),
    );
    s.sb.write("in.txt", "something longer");

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert_eq!(ran.report.failed.len(), 1);
    assert!(ran.report.failed[0].error.contains("changed"));
    assert!(!mark.exists(), "the program never ran");
    assert!(!s.out.exists());
    assert_eq!(leftovers(&s.sb.work), Vec::<String>::new());
}

#[test]
fn an_input_changed_while_the_program_ran_discards_what_it_wrote() {
    let s = scene();
    let error = failure_of(&s, by(vec![fake("touch", &[Arg::Input(0), Arg::Out])]));
    assert!(error.contains("changed"), "{error}");
    assert_eq!(text(&s.input), "hello, changed");
}

#[test]
fn a_name_taken_when_the_file_is_ready_is_left_alone_and_the_next_free_one_is_used() {
    let (s, ffs) = faulty_scene();
    let plan = plan_of(&s.e, &copy_request(&s.input, &s.out));
    let taken = s.out.clone();
    ffs.hook(Op::RenameStrict, "out.txt", 1, move || {
        std::fs::write(&taken, "somebody else's").unwrap();
    });

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert!(ran.report.failed.is_empty(), "{:?}", ran.report);
    assert_eq!(text(&s.out), "somebody else's");
    let published = s.sb.path("out (1).txt");
    assert_eq!(text(&published), "hello");

    // Undo removes the file under the name it got, and only that one.
    let (up, rep) = undo(&s.e, &s.j, &ran.id);
    assert!(up.blocked.is_empty() && rep.failed.is_empty());
    assert!(!published.exists());
    assert_eq!(text(&s.out), "somebody else's");
}

/// Cancels the run as soon as the program has started.
struct CancelWhenStarted {
    cancel: Cancel,
    mark: PathBuf,
}

impl ExecHandler for CancelWhenStarted {
    fn progress(&mut self, _p: &Progress) {
        if self.mark.exists() {
            self.cancel.cancel();
        }
    }
}

#[test]
fn cancelling_kills_the_program_and_publishes_nothing() {
    let s = scene();
    let mark = s.sb.path("started");
    let call = fake("slow", &[lit(&mark), Arg::Out]);
    let plan = plan_of(
        &s.e,
        &recipe(vec![produce(&s.input, &s.out, by(vec![call]))]),
    );
    let cancel = Cancel::new();
    let mut handler = CancelWhenStarted {
        cancel: cancel.clone(),
        mark: mark.clone(),
    };

    let ran =
        s.e.run_operation(&s.j, &plan, &mut handler, &cancel)
            .unwrap();

    assert!(ran.report.cancelled, "{:?}", ran.report);
    assert!(mark.exists(), "the program had started");
    assert!(!s.out.exists());
    assert_eq!(leftovers(&s.sb.work), Vec::<String>::new());
}

#[test]
fn a_program_that_takes_too_long_is_stopped_and_publishes_nothing() {
    let s = scene();
    let mark = s.sb.path("started");
    let slow = ProducerSpec::Program {
        chain: vec![fake("slow", &[lit(&mark), Arg::Out])],
        timeout_secs: 1,
        ok_exit: vec![0],
        collect: Collect::Exact,
    };

    let error = failure_of(&s, slow);

    assert!(error.contains("took more than 1 s"), "{error}");
    assert!(mark.exists());
}

#[test]
fn undo_removes_the_file_only_if_it_is_still_the_one_that_was_written() {
    let s = scene();
    let ran = run_journaled(&s.e, &s.j, &plan_of(&s.e, &copy_request(&s.input, &s.out)));
    let (up, rep) = undo(&s.e, &s.j, &ran.id);
    assert!(up.blocked.is_empty() && rep.failed.is_empty());
    assert!(!s.out.exists());
    assert_eq!(text(&s.input), "hello", "the input is never touched");

    let ran = run_journaled(&s.e, &s.j, &plan_of(&s.e, &copy_request(&s.input, &s.out)));
    std::fs::write(&s.out, "edited by the person since").unwrap();
    let (up, rep) = undo(&s.e, &s.j, &ran.id);
    assert!(
        !up.blocked.is_empty() || !rep.failed.is_empty(),
        "a changed file is not removed"
    );
    assert_eq!(text(&s.out), "edited by the person since");
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_refuse_a_taken_name_fails_the_step_and_says_why() {
    let (s, ffs) = faulty_scene();
    let plan = plan_of(&s.e, &copy_request(&s.input, &s.out));
    ffs.fail(Op::RenameStrict, "out.txt", libc::EOPNOTSUPP);

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert_eq!(ran.report.failed.len(), 1);
    let error = &ran.report.failed[0].error;
    assert!(error.contains("was not published"), "{error}");
    assert!(!s.out.exists());
    assert_eq!(leftovers(&s.sb.work), Vec::<String>::new());
}

#[test]
fn each_program_of_a_chain_reads_what_the_one_before_wrote() {
    let s = scene();
    let chain = vec![
        fake("copy", &[Arg::Input(0), Arg::Out]),
        fake("upper", &[Arg::Prev, Arg::Out]),
    ];
    let plan = plan_of(&s.e, &recipe(vec![produce(&s.input, &s.out, by(chain))]));

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert!(ran.report.failed.is_empty(), "{:?}", ran.report);
    assert_eq!(text(&s.out), "HELLO");
    assert_eq!(leftovers(&s.sb.work), Vec::<String>::new());
    let names: Vec<_> = std::fs::read_dir(&s.sb.work)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(names.len(), 2, "only the input and the output: {names:?}");
}

#[test]
fn a_chain_whose_second_program_fails_publishes_nothing() {
    let s = scene();
    let chain = vec![
        fake("copy", &[Arg::Input(0), Arg::Out]),
        fake("fail", &[Arg::Prev, Arg::Out]),
    ];
    let error = failure_of(&s, by(chain));
    assert!(error.contains("ended with code 3"), "{error}");
}

#[test]
fn a_program_that_names_its_output_itself_has_the_one_file_collected() {
    let s = scene();
    let own = ProducerSpec::Program {
        chain: vec![fake("outdir", &[Arg::OutDir, Arg::Input(0)])],
        timeout_secs: 60,
        ok_exit: vec![0],
        collect: Collect::Single { ext: "pdf".into() },
    };
    let out = s.sb.path("final.pdf");
    let plan = plan_of(&s.e, &recipe(vec![produce(&s.input, &out, own)]));

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert!(ran.report.failed.is_empty(), "{:?}", ran.report);
    assert_eq!(text(&out), "hello");
}

#[test]
fn a_checksum_is_written_the_way_sha256sum_writes_it() {
    let s = scene();
    let out = s.sb.path("in.txt.sha256");
    let sum = ProducerSpec::Checksum {
        algo: ChecksumAlgo::Sha256,
    };
    let plan = plan_of(&s.e, &recipe(vec![produce(&s.input, &out, sum)]));

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert!(ran.report.failed.is_empty(), "{:?}", ran.report);
    assert_eq!(text(&out), format!("{HELLO_SHA256}  in.txt\n"));
    assert_eq!(leftovers(&s.sb.work), Vec::<String>::new());
}

#[test]
fn a_request_in_json_becomes_the_same_plan() {
    let s = scene();
    let json = serde_json::json!({
        "op": "recipe",
        "recipe": "checksum",
        "requests": [{"produce": {
            "inputs": [s.input],
            "output": s.sb.path("sum.sha256"),
            "producer": {"by": "checksum", "algo": "sha256"},
        }}],
    });
    let req: OpRequest = serde_json::from_value(json).unwrap();

    let plan = plan_of(&s.e, &req);

    assert!(matches!(plan.steps[..], [Step::ProduceFile { .. }]));
    assert_eq!(plan.request, Some(req.clone()));
    let again: OpRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
    assert_eq!(again, req);
}

#[test]
fn shells_unknown_programs_and_rollback_are_refused_by_the_plan() {
    let s = scene();
    let with = |program: &str| {
        let call = ProgramCall {
            program: program.into(),
            args: vec![Arg::Input(0), Arg::Out],
        };
        try_plan(
            &s.e,
            &recipe(vec![produce(&s.input, &s.out, by(vec![call]))]),
        )
    };
    for shell in ["sh", "bash", "cmd", "powershell", "python3", "perl"] {
        let error = with(shell).unwrap_err().to_string();
        assert!(error.contains("never starts it"), "{shell}: {error}");
    }
    let error = with("no-such-tool").unwrap_err().to_string();
    assert!(error.contains("not a program rada knows"), "{error}");

    let OpRequest::Recipe(mut r) = copy_request(&s.input, &s.out) else {
        unreachable!()
    };
    r.failure = FailurePolicy::Rollback;
    let error = try_plan(&s.e, &OpRequest::Recipe(r))
        .unwrap_err()
        .to_string();
    assert!(error.contains("not available yet"), "{error}");
}

#[test]
fn a_program_that_is_not_installed_blocks_the_plan_and_names_it() {
    let s = scene();
    let ghost = Programs::new([("ghost".to_string(), s.sb.path("not-there"))].into());
    let e = s.sb.engine().with_programs(ghost);
    let call = ProgramCall {
        program: "ghost".into(),
        args: vec![Arg::Input(0), Arg::Out],
    };

    let plan = try_plan(&e, &recipe(vec![produce(&s.input, &s.out, by(vec![call]))])).unwrap();

    assert!(!plan.is_executable());
    let blocking: Vec<_> = plan.blocking().collect();
    assert_eq!(blocking[0].kind, WarningKind::ProgramMissing);
    assert!(
        blocking[0].message.contains("ghost"),
        "{}",
        blocking[0].message
    );
}

/// A file that can be started but never is: the table is only asked where it is.
#[cfg(unix)]
fn installed(sb: &Sandbox, name: &str) -> PathBuf {
    let p = sb.mkdir("bin").join(name);
    std::fs::write(&p, "").unwrap();
    chmod(&p, 0o755);
    p
}

#[cfg(unix)]
#[test]
fn a_name_becomes_the_file_it_stands_for_on_this_system() {
    use rada_core::ops::programs::{Os, Unresolved};
    let sb = Sandbox::new();
    let convert = installed(&sb, "convert");
    let gs = installed(&sb, "gs");
    let path = sb.path("bin").into_os_string();
    let table = Programs::default();

    let magick = table.resolve_in("magick", &path, Os::Linux).unwrap();
    assert_eq!((magick.name.as_str(), magick.path), ("magick", convert));
    assert_eq!(table.resolve_in("gs", &path, Os::Linux).unwrap().path, gs);
    // On Windows `convert` is the system's tool for disks, and Ghostscript has other names.
    for name in ["magick", "gs"] {
        let missing = table.resolve_in(name, &path, Os::Windows);
        assert_eq!(missing, Err(Unresolved::Missing(name.into())));
    }
}

#[cfg(unix)]
#[test]
fn the_configuration_names_a_file_but_never_a_shell() {
    use rada_core::ops::programs::{Os, Unresolved};
    let sb = Sandbox::new();
    let own = installed(&sb, "my-qpdf");
    let shell = installed(&sb, "bash");
    let table = Programs::new(
        [
            ("qpdf".to_string(), own.clone()),
            ("tool".to_string(), shell),
            ("relative".to_string(), PathBuf::from("bin/my-qpdf")),
        ]
        .into(),
    );
    let nowhere = std::ffi::OsStr::new("");

    assert_eq!(
        table.resolve_in("qpdf", nowhere, Os::Linux).unwrap().path,
        own
    );
    let refused = table.resolve_in("tool", nowhere, Os::Linux);
    assert_eq!(refused, Err(Unresolved::Refused("tool".into())));
    let relative = table.resolve_in("relative", nowhere, Os::Linux);
    assert_eq!(relative, Err(Unresolved::Missing("relative".into())));
}

#[cfg(unix)]
#[test]
fn only_files_can_be_read_by_an_action() {
    let s = scene();
    let link = s.sb.symlink(&s.input, "link.txt");
    let folder = s.sb.mkdir("folder");
    for input in [link, folder, s.sb.path("missing.txt")] {
        let plan = try_plan(&s.e, &copy_request(&input, &s.out)).unwrap();
        assert!(!plan.is_executable(), "{}", input.display());
    }
}

#[test]
fn a_recipe_is_one_operation_with_one_undo() {
    let s = scene();
    let second = s.sb.path("second.txt");
    let copy = |out: &Path| {
        let call = fake("copy", &[Arg::Input(0), Arg::Out]);
        produce(&s.input, out, by(vec![call]))
    };
    let folder = SubRequest::Op(OpRequest::MakeDir {
        parent: s.sb.work.clone(),
        name: "made".into(),
    });
    let plan = plan_of(&s.e, &recipe(vec![copy(&s.out), folder, copy(&second)]));
    assert_eq!(plan.steps.len(), 3);

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert_eq!(ran.report.done, 3, "{:?}", ran.report);
    assert_eq!(s.j.entries().unwrap().len(), 1);
    let (up, rep) = undo(&s.e, &s.j, &ran.id);
    assert!(up.blocked.is_empty() && rep.failed.is_empty());
    assert!(!s.out.exists() && !second.exists() && !s.sb.path("made").exists());
    assert_eq!(text(&s.input), "hello");
}

#[test]
fn requests_that_get_in_each_others_way_are_not_a_plan() {
    let s = scene();
    let copy = || {
        let call = fake("copy", &[Arg::Input(0), Arg::Out]);
        produce(&s.input, &s.out, by(vec![call]))
    };
    let twice = try_plan(&s.e, &recipe(vec![copy(), copy()])).unwrap();
    assert!(!twice.is_executable());

    let trash = SubRequest::Op(OpRequest::Trash {
        sources: vec![s.input.clone()],
    });
    let gone = try_plan(&s.e, &recipe(vec![copy(), trash])).unwrap();
    assert!(!gone.is_executable());
    assert!(
        gone.blocking()
            .any(|w| w.message.contains("another removes")),
        "{:?}",
        gone.warnings
    );
}

#[test]
fn what_was_made_stays_when_another_step_fails_and_can_be_undone() {
    let s = scene();
    let (a, b, c) = (s.sb.path("a.txt"), s.sb.path("b.txt"), s.sb.path("c.txt"));
    let with = |role: &str, out: &Path| {
        let call = fake(role, &[Arg::Input(0), Arg::Out]);
        produce(&s.input, out, by(vec![call]))
    };
    let plan = plan_of(
        &s.e,
        &recipe(vec![with("copy", &a), with("fail", &b), with("copy", &c)]),
    );

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert_eq!((ran.report.done, ran.report.failed.len()), (2, 1));
    assert_eq!(ran.report.status(), RunStatus::CompletedWithProblems);
    assert!(a.exists() && !b.exists() && c.exists());
    let (up, rep) = undo(&s.e, &s.j, &ran.id);
    assert!(up.blocked.is_empty() && rep.failed.is_empty());
    assert!(!a.exists() && !c.exists());
}

#[test]
fn a_journal_that_cannot_take_the_second_record_means_nothing_is_published() {
    let s = scene();
    let plan = plan_of(&s.e, &copy_request(&s.input, &s.out));
    // The first record is written before the program starts, the second before the name.
    s.j.fail_record(RecordKind::Pending, 2);

    let ran = run_journaled(&s.e, &s.j, &plan);

    assert!(!s.out.exists(), "{:?}", ran.report);
    assert!(ran.report.aborted);
    assert!(ran.report.journal_gap.as_ref().is_some_and(|g| g.stopped));
    assert_eq!(leftovers(&s.sb.work), Vec::<String>::new());
}

#[test]
fn rada_0_6_1_skips_what_an_action_writes_in_the_journal_and_reads_the_rest() {
    let s = scene();
    let ran = run_journaled(&s.e, &s.j, &plan_of(&s.e, &copy_request(&s.input, &s.out)));
    undo(&s.e, &s.j, &ran.id);
    let failing = by(vec![fake("fail", &[Arg::Input(0), Arg::Out])]);
    failure_of(&s, failing);

    let journal = text(s.j.path());
    let reading = journal_061::read(&journal);

    assert_eq!(reading.damaged, Vec::new(), "0.6.1 would call this damage");
    let actions = journal.matches(r#""t":"begin_action""#).count();
    assert_eq!(actions, 2);
    assert_eq!(reading.skipped, actions, "only the new record is skipped");
    assert!(reading.read > actions);
}

#[test]
fn rada_0_6_1_reads_a_start_that_says_its_undo_cannot_be_promised() {
    let sb = Sandbox::new();
    let j = sb.journal();
    let mut plan = Plan::empty(OpKind::Copy, "to a phone");
    plan.warnings.push(Warning {
        kind: WarningKind::DeviceUndo,
        severity: Severity::Warning,
        message: String::new(),
        count: 1,
        examples: Vec::new(),
    });
    let id = j.begin_plan(&plan).unwrap();
    j.end(&id, RunStatus::Completed, 0, 0, 0).unwrap();

    let journal = text(j.path());
    assert!(journal.contains(r#""undo_limit":"device""#), "{journal}");
    let reading = journal_061::read(&journal);

    assert_eq!(reading.damaged, Vec::new(), "0.6.1 would call this damage");
    assert_eq!(
        (reading.read, reading.skipped),
        (2, 0),
        "a plain begin to it"
    );
}

// ---------------------------------------------------------------------------------------------
// Killed in the middle
// ---------------------------------------------------------------------------------------------

#[cfg(unix)]
fn park(marker: &Path) -> ! {
    std::fs::write(marker, b"parked").unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

/// Not a test: the rada that is killed while it runs an action. Only does something when
/// started by [`killed_at`].
#[cfg(unix)]
#[test]
#[ignore = "the victim of the tests that kill an action"]
fn victim() {
    use rada_core::platform::{self, Dirs};
    let Ok(point) = std::env::var("RADA_ACTION_POINT") else {
        return;
    };
    let var = |name: &str| PathBuf::from(std::env::var(name).unwrap());
    let (root, input, out) = (
        var("RADA_ACTION_ROOT"),
        var("RADA_ACTION_IN"),
        var("RADA_ACTION_OUT"),
    );
    let marker = var("RADA_ACTION_MARKER");
    let dirs = Dirs::under(&root);
    let journal = Journal::open(dirs.journal_path()).unwrap();
    let ffs = FaultFs::new();
    let engine = Engine::new(ffs.clone(), platform::current(dirs)).with_programs(programs());
    let plan = plan_of(&engine, &copy_request(&input, &out));
    // After the plan is made: the plan looks at the same files.
    let m = marker.clone();
    match point.as_str() {
        // The first look at what the program wrote: it has ended, nothing is recorded yet.
        "between_the_records" => ffs.hook(Op::Lstat, "out.txt", 1, move || park(&m)),
        "before_the_name" => ffs.hook(Op::RenameStrict, "out.txt", 1, move || park(&m)),
        // The first look at the destination is the one right after it got its name.
        "after_the_name" => ffs.hook(Op::Lstat, "result.txt", 1, move || park(&m)),
        other => panic!("unknown point {other}"),
    }
    let _ = engine.run_operation(&journal, &plan, &mut SkipErrors, &Cancel::new());
    std::fs::write(&marker, b"finished").unwrap();
}

/// Kill a rada that runs an action at `point`, settle on the next start, and hand back the
/// scene with what recovery said.
#[cfg(unix)]
fn killed_at(point: &str) -> (Scene, Recovered) {
    let mut s = scene();
    s.out = s.sb.path("result.txt");
    let marker = s.sb.root.join("marker");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["victim", "--exact", "--ignored", "--nocapture"])
        .env("RADA_ACTION_POINT", point)
        .env("RADA_ACTION_ROOT", &s.sb.root)
        .env("RADA_ACTION_IN", &s.input)
        .env("RADA_ACTION_OUT", &s.out)
        .env("RADA_ACTION_MARKER", &marker)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let end = std::time::Instant::now() + Duration::from_secs(60);
    // The child makes the file before it writes the word in it: wait for the word.
    while std::fs::read(&marker).map_or(true, |b| b.is_empty()) && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    let reached = std::fs::read(&marker).unwrap_or_default();
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(reached, b"parked", "{point}: the point was never reached");

    assert_eq!(s.j.interrupted().unwrap().len(), 1, "{point}");
    let mut rec = s.e.recover_interrupted(&s.j).unwrap();
    assert_eq!(rec.len(), 1, "{point}: {rec:?}");
    assert_eq!(leftovers(&s.sb.work), Vec::<String>::new(), "{point}");
    assert!(s.e.recover_interrupted(&s.j).unwrap().is_empty());
    let entry = s.j.entries().unwrap().pop().unwrap();
    assert_eq!(entry.status, EntryStatus::Finished(RunStatus::Interrupted));
    (s, rec.remove(0))
}

#[cfg(unix)]
#[test]
fn killed_before_the_second_record_nothing_has_a_name_and_the_work_folder_goes() {
    let (s, rec) = killed_at("between_the_records");
    assert!(!s.out.exists());
    assert_eq!(rec.removed_temps.len(), 1, "{rec:?}");
    assert!(rec.adopted.is_empty() && rec.notes.is_empty(), "{rec:?}");
}

#[cfg(unix)]
#[test]
fn killed_after_the_second_record_but_before_the_name_nothing_is_published() {
    let (s, rec) = killed_at("before_the_name");
    assert!(!s.out.exists());
    assert_eq!(rec.removed_temps.len(), 1, "{rec:?}");
    assert!(rec.adopted.is_empty(), "{rec:?}");
}

#[cfg(unix)]
#[test]
fn killed_right_after_the_name_the_file_is_taken_as_made_and_undo_removes_it() {
    let (s, rec) = killed_at("after_the_name");
    assert_eq!(text(&s.out), "hello");
    assert_eq!(rec.adopted, std::slice::from_ref(&s.out));
    let id = s.j.entries().unwrap().pop().unwrap().id;
    let (up, rep) = undo(&s.e, &s.j, &id);
    assert!(up.blocked.is_empty() && rep.failed.is_empty());
    assert!(!s.out.exists());
}

#[test]
fn a_file_that_is_not_the_one_the_step_wrote_is_left_alone_by_recovery() {
    let s = scene();
    let out = s.sb.path("result.txt");
    // What a rada killed right before giving the name leaves in the journal; then somebody
    // else puts a file under that name before the next start.
    let intent = rada_core::journal::Intent::publishing(&out, 5, None);
    let totals = Totals::default();
    let id =
        s.j.begin(OpKind::Action, "cut", true, totals, None, None)
            .unwrap();
    s.j.pending(&id, intent).unwrap();
    std::fs::write(&out, "not the five bytes that were expected").unwrap();
    // The lock of the operation goes with the journal that began it, as with a dead process.
    drop(s.j);
    let j = s.sb.journal();

    let rec = s.e.recover_interrupted(&j).unwrap();

    assert_eq!(rec.len(), 1);
    assert!(rec[0].adopted.is_empty(), "{rec:?}");
    assert_eq!(rec[0].notes.len(), 1, "it says so: {rec:?}");
    assert!(out.exists());
    assert_eq!(j.entries().unwrap()[0].undo_steps, 0);
}

// ---------------------------------------------------------------------------------------------
// The stand-in
// ---------------------------------------------------------------------------------------------

/// Not a test: the program the tests above run. Without a part to play it does nothing.
#[test]
#[ignore = "the stand-in program of the other tests"]
fn stand_in() {
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|a| a.starts_with("role:")) else {
        return;
    };
    let role = args[at].trim_start_matches("role:").to_string();
    let rest: Vec<PathBuf> = args[at + 1..].iter().map(PathBuf::from).collect();
    std::process::exit(play(&role, &rest));
}

fn play(role: &str, p: &[PathBuf]) -> i32 {
    match role {
        "copy" => copy(&p[0], &p[1]),
        "upper" => {
            let text = std::fs::read_to_string(&p[0]).unwrap();
            std::fs::write(&p[1], text.to_uppercase()).unwrap();
            0
        }
        "fail" => {
            copy(&p[0], &p[1]);
            eprintln!("first line\nboom");
            3
        }
        "nothing" => 0,
        "empty" => {
            std::fs::write(&p[0], "").unwrap();
            0
        }
        "trace" => {
            std::fs::write(&p[0], "ran").unwrap();
            copy(&p[1], &p[2])
        }
        "touch" => {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&p[0])
                .unwrap();
            f.write_all(b", changed").unwrap();
            copy(&p[0], &p[1])
        }
        "slow" => {
            std::fs::write(&p[0], "started").unwrap();
            std::thread::sleep(Duration::from_secs(120));
            0
        }
        "outdir" => copy(&p[1], &p[0].join("whatever it likes.pdf")),
        other => panic!("no part called {other}"),
    }
}

fn copy(from: &Path, to: &Path) -> i32 {
    std::fs::copy(from, to).unwrap();
    0
}
