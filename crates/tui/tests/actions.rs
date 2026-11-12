//! The plan of an action, as the window shows it before anything runs: the file that will be
//! written, what is read, what is run and from which file, and what rada answers for.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use common::*;
use rada_core::ops::*;
use rada_core::testutil::*;

/// The clock of the pictures: the dates on the screen do not change with the day.
fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

/// A folder under the home of the sandbox with one file in it, of a fixed age.
fn folder(sb: &Sandbox) -> (PathBuf, PathBuf) {
    let dir = sb.dirs.home.join("demo/papers");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("report.pdf");
    std::fs::write(&file, "a report").unwrap();
    set_mtime(&file, fixed_now() - Duration::from_secs(3 * 3600));
    (dir, file)
}

fn recipe(input: &Path, output: PathBuf, producer: ProducerSpec) -> OpRequest {
    OpRequest::Recipe(RecipePlan {
        recipe: "test".into(),
        requests: vec![SubRequest::Produce(ProduceSpec {
            inputs: vec![input.to_path_buf()],
            output,
            producer,
            size_hint: None,
        })],
        title: None,
        notes: vec!["A note of the recipe.".into()],
        failure: FailurePolicy::Keep,
    })
}

/// A program called `fake` that is given the input and the file to write.
fn fake_program() -> ProducerSpec {
    let call = ProgramCall {
        program: "fake".into(),
        args: vec![Arg::Input(0), Arg::Out],
    };
    ProducerSpec::Program {
        chain: vec![call],
        timeout_secs: 60,
        ok_exit: vec![0],
        collect: Collect::Exact,
    }
}

fn shot(h: &mut H) -> String {
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().map(str::trim_end).collect();
    lines.join("\n") + "\n"
}

/// The window of the plan of `req`, at `w`x`h`.
fn plan_window(sb: Sandbox, dir: PathBuf, req: OpRequest, w: u16, h: u16) -> H {
    let mut h = H::with_request(sb, dir, w, h, req);
    plan_modal(&mut h);
    // The places of the navigation pane come from a worker: the picture waits for them.
    h.wait("the places", |a| !a.places.paths.places.is_empty());
    h.app.hooks.clock = Some(fixed_now());
    h
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAGS section, which the Linux snapshot has"
)]
fn the_plan_of_a_checksum_says_what_is_read_and_what_is_written() {
    let sb = Sandbox::new();
    let (dir, file) = folder(&sb);
    let sum = ProducerSpec::Checksum {
        algo: ChecksumAlgo::Sha256,
    };
    let req = recipe(&file, dir.join("report.pdf.sha256"), sum);

    // Shown from a folder with nothing in it: the panes behind the window have no dates.
    let empty = sb.dirs.home.join("demo/empty");
    std::fs::create_dir_all(&empty).unwrap();

    let mut h = plan_window(sb, empty, req, 110, 30);

    insta::assert_snapshot!("plan_action_110x30", shot(&mut h));
}

#[test]
fn the_plan_of_a_program_names_the_file_that_runs_and_what_rada_answers_for() {
    let sb = Sandbox::new();
    let (dir, file) = folder(&sb);
    // Never run: the plan is only looked at.
    let exe = std::env::current_exe().unwrap();
    give_programs(Programs::new([("fake".to_string(), exe.clone())].into()));
    let run = fake_program();
    let req = recipe(&file, dir.join("small.pdf"), run);

    let mut h = plan_window(sb, dir, req, 140, 40);
    let screen = h.screen();

    for said in [
        "Make small.pdf with fake",
        "Writes",
        "small.pdf",
        "a new file: it never replaces one",
        "Reads",
        "report.pdf",
        "Runs",
        "fake  ·  ",
        "another program does the writing: rada guarantees only",
        "A note of the recipe.",
        "If a step fails, what was already made stays and can be undone.",
    ] {
        assert!(screen.contains(said), "{said:?} is not in\n{screen}");
    }
    // The start of its path: the rest may not fit the window.
    let shown: String = rada_core::display::path(&exe).chars().take(8).collect();
    assert!(screen.contains(&shown), "the path of the program: {screen}");
}

#[test]
fn an_action_that_ran_is_one_entry_that_undo_takes_back() {
    let sb = Sandbox::new();
    let (dir, file) = folder(&sb);
    let sum = ProducerSpec::Checksum {
        algo: ChecksumAlgo::Sha256,
    };
    let out = dir.join("report.pdf.sha256");
    let mut h = plan_window(sb, dir, recipe(&file, out.clone(), sum), 120, 36);

    run_plan_and_wait(&mut h);

    assert!(out.exists(), "{}", h.screen());
    let screen = h.screen();
    assert!(
        screen.contains("New file made: report.pdf.sha256"),
        "{screen}"
    );
    h.press("ctrl+z");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(!out.exists(), "{}", h.screen());
}
