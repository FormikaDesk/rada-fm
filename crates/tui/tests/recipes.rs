//! Recipes in the interface: the Actions submenu, the settings asked in the windows the
//! interface already has, the bar and the command line, and an action stopped while its
//! program runs.
//!
//! The host here is the test's own: the interface only knows the trait, never Lua. The program
//! of the action that is stopped is this very test binary, playing the part `stand_in` names.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use crossterm::event::KeyCode;
use rada_core::i18n::{Lang, with_lang};
use rada_core::ops::*;
use rada_core::plugin::*;
use rada_core::testutil::*;
use rada_tui::app::{BarItem, MenuCmd, MenuKind, Modal};

/// Three recipes for PDFs: one that needs two, one with settings, one whose program runs for
/// two minutes. They write a checksum, or start the stand-in.
#[derive(Default)]
struct Host {
    asked: Mutex<Vec<ArgValues>>,
    marker: Option<PathBuf>,
}

fn info(name: &str, title: &str, args: Vec<RecipeArg>, state: Availability) -> RecipeInfo {
    RecipeInfo {
        name: name.into(),
        title: title.into(),
        description: String::new(),
        args,
        origin: RecipeOrigin::Builtin,
        state,
    }
}

fn settings() -> Vec<RecipeArg> {
    vec![
        RecipeArg {
            name: "size".into(),
            label: "Size".into(),
            kind: ArgKind::Choice(vec!["small".into(), "big".into()]),
            default: Some(ArgValue::Text("big".into())),
        },
        RecipeArg {
            name: "label".into(),
            label: "Label".into(),
            kind: ArgKind::Text,
            default: Some(ArgValue::Text("x".into())),
        },
    ]
}

impl PluginHost for Host {
    fn load(&self) -> Vec<LoadProblem> {
        Vec::new()
    }

    fn recipes(&self, ctx: &RecipeContext) -> Vec<RecipeInfo> {
        let pdfs = ctx
            .selection
            .iter()
            .all(|p| p.extension().is_some_and(|e| e == "pdf"));
        if !pdfs {
            return Vec::new();
        }
        let merge = if ctx.selection.len() < 2 {
            Availability::Disabled("choose at least 2 items".into())
        } else {
            Availability::Available
        };
        vec![
            info("merge", "Merge PDFs", Vec::new(), merge),
            info("slow", "Slow one", Vec::new(), Availability::Available),
            info("stamp", "Stamp", settings(), Availability::Available),
        ]
    }

    fn run(
        &self,
        name: &str,
        ctx: &RecipeContext,
        args: &ArgValues,
        _: &Cancel,
    ) -> Result<RecipePlan, RecipeError> {
        self.asked.lock().unwrap().push(args.clone());
        let input = ctx.selection[0].clone();
        let spec = match name {
            "slow" => slow(&input, self.marker.as_deref().unwrap()),
            _ => ProduceSpec {
                inputs: vec![input.clone()],
                output: PathBuf::from(format!("{}.sha256", input.display())),
                producer: ProducerSpec::Checksum {
                    algo: ChecksumAlgo::Sha256,
                },
                size_hint: None,
            },
        };
        Ok(RecipePlan {
            recipe: name.into(),
            requests: vec![SubRequest::Produce(spec)],
            title: None,
            notes: Vec::new(),
            failure: FailurePolicy::Keep,
        })
    }
}

/// The stand-in playing `slow`: it says it started, then waits two minutes.
fn slow(input: &Path, marker: &Path) -> ProduceSpec {
    let lit = |s: &str| Arg::Lit(s.to_string());
    let call = ProgramCall {
        program: "fake".into(),
        args: vec![
            lit("stand_in"),
            lit("--exact"),
            lit("--ignored"),
            lit("--nocapture"),
            lit("role:slow"),
            Arg::Lit(marker.display().to_string()),
            Arg::Out,
        ],
    };
    ProduceSpec {
        inputs: vec![input.to_path_buf()],
        output: input.with_file_name("slow.pdf"),
        producer: ProducerSpec::Program {
            chain: vec![call],
            timeout_secs: 60,
            ok_exit: vec![0],
            collect: Collect::Exact,
        },
        size_hint: None,
    }
}

/// `~/papers` with two PDFs and a note, the host given, the cursor on `a.pdf`.
fn scene(w: u16, h: u16) -> (H, Arc<Host>, PathBuf) {
    let sb = Sandbox::new();
    let dir = sb.dirs.home.join("papers");
    std::fs::create_dir_all(&dir).unwrap();
    for (n, text) in [("a.pdf", "a"), ("b.pdf", "b"), ("note.txt", "n")] {
        std::fs::write(dir.join(n), text).unwrap();
    }
    let marker = sb.path("started");
    give_programs(Programs::new(
        [("fake".to_string(), std::env::current_exe().unwrap())].into(),
    ));
    let mut h = H::new(sb, dir.clone(), w, h);
    let host = Arc::new(Host {
        marker: Some(marker),
        ..Host::default()
    });
    h.app.use_plugins(Some(host.clone()));
    h.cursor_to("a.pdf");
    (h, host, dir)
}

fn labels(items: &[rada_tui::app::MenuItem]) -> Vec<(String, bool, Option<String>)> {
    items
        .iter()
        .map(|i| (i.label.clone(), i.enabled, i.note.clone()))
        .collect()
}

fn choose(h: &mut H, label: &str) {
    let Some(Modal::Menu(m)) = &h.app.windows.modal else {
        panic!("no menu:\n{}", h.screen());
    };
    let at = m.items.iter().position(|i| i.label == label).unwrap();
    let from = m.selected;
    let (key, n) = if at >= from {
        (KeyCode::Down, at - from)
    } else {
        (KeyCode::Up, from - at)
    };
    for _ in 0..n {
        h.key(key);
    }
    h.key(KeyCode::Enter);
}

/// The rows of the menu that holds `word`, from its top border: only the menu, since the rest
/// of the screen has today's dates and the system's places.
fn menu_box(screen: &str, word: &str, rows: usize) -> String {
    let lines: Vec<Vec<char>> = screen.lines().map(|l| l.chars().collect()).collect();
    let at = lines
        .iter()
        .position(|l| l.iter().collect::<String>().contains(word))
        .unwrap();
    let text: String = lines[at].iter().collect();
    let col = text[..text.find(word).unwrap()].chars().count();
    let left = (0..col).rev().find(|i| lines[at][*i] == '│').unwrap();
    let right = (col..lines[at].len())
        .find(|i| lines[at][*i] == '│')
        .unwrap();
    let top = (0..=at)
        .rev()
        .find(|r| lines[*r].get(left) == Some(&'╭'))
        .unwrap();
    let boxed: Vec<String> = lines[top..top + rows]
        .iter()
        .map(|l| l[left..=right].iter().collect())
        .collect();
    boxed.join("\n") + "\n"
}

#[test]
fn the_menu_of_an_item_has_its_actions_and_says_why_one_cannot_run() {
    let (mut h, _, _) = scene(100, 30);
    let item = h.app.context_menu(true, (10, 5));
    let entry = item
        .items
        .iter()
        .find(|i| i.label == "Actions")
        .expect("an Actions entry");
    assert_eq!(entry.cmd, MenuCmd::Sub(MenuKind::Actions));
    assert_eq!(
        labels(&h.app.menu_items(MenuKind::Actions)),
        [
            (
                "Merge PDFs".into(),
                false,
                Some("choose at least 2 items".into())
            ),
            ("Slow one".into(), true, None),
            ("Stamp".into(), true, None),
        ]
    );
    h.cursor_to("note.txt");
    let menu = h.app.context_menu(true, (10, 5));
    assert!(
        menu.items.iter().all(|i| i.label != "Actions"),
        "nothing for a note"
    );
}

#[test]
fn the_actions_submenu_on_screen() {
    let (mut h, _, _) = scene(100, 30);
    h.press("shift+f10");
    choose(&mut h, "Actions");
    let screen = h.screen();
    assert!(
        screen.contains("Merge PDFs") && screen.contains("choose at least 2 items"),
        "{screen}"
    );
    insta::assert_snapshot!(
        "recipes_actions_menu_100x30",
        menu_box(&screen, "Merge PDFs", 5)
    );
}

#[test]
fn a_recipe_asks_its_settings_with_a_menu_and_the_input_window_then_plans() {
    let (mut h, host, dir) = scene(100, 30);
    h.press("shift+f10");
    choose(&mut h, "Actions");
    choose(&mut h, "Stamp");
    // The choice: its default under the cursor.
    let Some(Modal::Menu(m)) = &h.app.windows.modal else {
        panic!("{}", h.screen());
    };
    assert_eq!(m.items[m.selected].label, "big");
    let screen = h.screen();
    insta::assert_snapshot!(
        "recipes_choice_100x30",
        menu_box(&screen, "Stamp · Size", 5)
    );
    choose(&mut h, "small");
    // The text, with its default written.
    let Some(Modal::Input(iv)) = &h.app.windows.modal else {
        panic!("{}", h.screen());
    };
    assert_eq!(iv.text, "x");
    assert!(h.screen().contains("Stamp · Label"), "{}", h.screen());
    h.key(KeyCode::Backspace);
    h.keys("cover");
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(dir.join("a.pdf.sha256").exists(), "{}", h.screen());
    let asked = host.asked.lock().unwrap().last().cloned().unwrap();
    assert_eq!(asked.get("size"), Some(&ArgValue::Text("small".into())));
    assert_eq!(asked.get("label"), Some(&ArgValue::Text("cover".into())));
}

#[test]
fn a_greyed_action_does_nothing() {
    let (mut h, host, _) = scene(100, 30);
    h.press("shift+f10");
    choose(&mut h, "Actions");
    let Some(Modal::Menu(m)) = &h.app.windows.modal else {
        panic!();
    };
    assert_eq!(
        m.items[m.selected].label, "Slow one",
        "the cursor skips the greyed one"
    );
    let err = h.app.start_recipe("merge", ArgValues::new()).unwrap_err();
    assert_eq!(err, "choose at least 2 items");
    assert!(host.asked.lock().unwrap().is_empty());
}

#[test]
fn the_bar_offers_the_actions_by_their_titles() {
    let (mut h, _, dir) = scene(100, 30);
    h.press("ctrl+p");
    h.keys("stam");
    let hits = h.app.windows.address.as_ref().unwrap().hits.clone();
    let at = hits
        .iter()
        .position(|hit| matches!(&hit.item, BarItem::Recipe { name, .. } if name == "stamp"))
        .unwrap_or_else(|| panic!("{}", h.screen()));
    assert!(h.screen().contains(":stamp"), "{}", h.screen());
    for _ in 0..=at {
        h.key(KeyCode::Down);
    }
    h.key(KeyCode::Enter);
    choose(&mut h, "big");
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(dir.join("a.pdf.sha256").exists());
}

#[test]
fn the_command_line_runs_an_action_by_its_name_with_its_settings() {
    let (mut h, host, dir) = scene(100, 30);
    h.press(":");
    h.keys("recipe stamp size=small label=y");
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    let asked = host.asked.lock().unwrap().last().cloned().unwrap();
    assert_eq!(asked.get("label"), Some(&ArgValue::Text("y".into())));
    h.key(KeyCode::Esc);

    h.press(":");
    h.keys("stamp label=z");
    h.key(KeyCode::Enter);
    choose(&mut h, "big");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(dir.join("a.pdf.sha256").exists());

    for (line, said) in [
        ("recipe nope", "No action called nope"),
        ("recipe stamp size=huge", "not one of the values it takes"),
        ("recipe stamp colour=red", "the action has no such setting"),
        ("recipe stamp loud", "a setting is name=value"),
    ] {
        h.press(":");
        h.keys(line);
        h.key(KeyCode::Enter);
        let screen = h.screen();
        assert!(screen.contains(said), "{line}: {screen}");
        h.key(KeyCode::Esc);
    }
}

#[test]
fn without_recipes_there_is_no_actions_entry_and_no_recipe_to_run() {
    let (mut h, _, _) = scene(100, 30);
    h.app.use_plugins(None);
    let menu = h.app.context_menu(true, (10, 5));
    assert!(menu.items.iter().all(|i| i.label != "Actions"));
    h.press(":");
    h.keys("recipe stamp");
    h.key(KeyCode::Enter);
    assert!(
        h.screen().to_lowercase().contains("not available here"),
        "{}",
        h.screen()
    );
}

#[test]
fn in_italian() {
    with_lang(Lang::It, || {
        let (mut h, _, _) = scene(100, 30);
        let menu = h.app.context_menu(true, (10, 5));
        assert!(menu.items.iter().any(|i| i.label == "Azioni"));
        h.press(":");
        h.keys("recipe nope");
        h.key(KeyCode::Enter);
        let screen = h.screen();
        assert!(screen.contains("Nessuna azione nope"), "{screen}");
    });
}

#[test]
fn a_long_action_shows_its_progress_and_esc_stops_its_program() {
    let (mut h, host, dir) = scene(100, 30);
    let marker = host.marker.clone().unwrap();
    h.app.start_recipe("slow", ArgValues::new()).unwrap();
    plan_modal(&mut h);
    h.key(KeyCode::Enter);
    h.wait("the program to start", |_| {
        std::fs::read_to_string(&marker).is_ok_and(|s| s == "started")
    });
    assert!(h.app.ops.running.is_some());
    let screen = h.screen();
    assert!(
        screen.contains("Esc"),
        "the card of the running action: {screen}"
    );
    h.key(KeyCode::Esc);
    h.wait("the action to stop", |a| a.ops.running.is_none());
    assert!(!dir.join("slow.pdf").exists());
    let left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".rada-work"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

// ---------------------------------------------------------------------------------------------
// The stand-in
// ---------------------------------------------------------------------------------------------

/// Not a test: the program of the action above. Without a part to play it does nothing.
#[test]
#[ignore = "the stand-in program of the other tests"]
fn stand_in() {
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|a| a == "role:slow") else {
        return;
    };
    std::fs::write(&args[at + 1], "started").unwrap();
    std::thread::sleep(Duration::from_secs(120));
}
