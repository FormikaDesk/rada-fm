//! Which recipes fit which files, their arguments, and the plans they ask for.

use std::collections::BTreeMap;

use rada_core::i18n::{Lang, with_lang};
use rada_core::ops::programs::Os;
use rada_core::ops::{Arg, Collect, OpRequest, ProducerSpec, Programs, SubRequest};
use rada_core::plugin::{
    ArgKind, ArgValue, ArgValues, Availability, Cancel, PluginHost, RecipeContext, RecipeError,
    RecipeOrigin,
};
use rada_core::testutil::Sandbox;
use rada_lua::{Builtin, LuaHost, Tools};

const PDF_TOOLS: &str = r#"
rada.recipe {
  name = "merge",
  title = { en = "Merge PDFs", it = "Unisci PDF" },
  description = "One PDF from several",
  when = { ext = "pdf", min = 2 },
  run = function(ctx)
    local first = ctx.selection[1]
    rada.run {
      program = "qpdf",
      args = { "--empty", "--pages", rada.inputs, "--", rada.out },
      inputs = ctx.selection,
      output = rada.join(rada.parent(first), rada.stem(first) .. "-merged.pdf"),
    }
    return { title = "Merge " .. #ctx.selection, notes = { "a note" } }
  end,
}

rada.recipe {
  name = "rotate",
  title = "Rotate pages",
  when = { ext = { "PDF" }, max = 1 },
  args = {
    { name = "angle", label = { en = "Angle", it = "Angolo" }, choices = { "90", "180", "270" }, default = "90" },
    { name = "pages", type = "text", default = "1-z" },
    { name = "copies", type = "int", min = 1, max = 3, default = 1 },
  },
  run = function(ctx, args)
    for i = 1, args.copies do
      rada.run {
        program = "qpdf",
        args = { rada.input(1), "--rotate=+" .. args.angle .. ":" .. args.pages, rada.out },
        inputs = ctx.selection[1],
        output = rada.join(ctx.cwd, "rotated-" .. i .. ".pdf"),
        timeout = 30,
      }
    end
  end,
}

rada.recipe {
  name = "office",
  when = { ext = { "docx", "odt" } },
  available = function(ctx)
    if rada.has("not-a-known-program") then return true end
    return false, "LibreOffice is not installed"
  end,
  run = function(ctx)
    rada.run {
      program = "soffice",
      args = { "--headless", "--convert-to", "pdf", "--outdir", rada.outdir, rada.profile, rada.input(1) },
      inputs = ctx.selection[1],
      output = rada.join(ctx.cwd, "doc.pdf"),
      collect = { single = "pdf" },
    }
  end,
}

rada.recipe {
  name = "hidden",
  when = { ext = "pdf" },
  available = function() return false end,
  run = function() end,
}

rada.recipe {
  name = "folder",
  title = "Index the folder",
  when = { dir = true, kind = "folder" },
  run = function(ctx)
    rada.checksum { input = rada.join(ctx.cwd, "a.pdf"), output = rada.join(ctx.cwd, "a.pdf.sha256") }
  end,
}
"#;

fn setup() -> (Sandbox, LuaHost) {
    let sb = Sandbox::new();
    let dir = sb.mkdir("recipes");
    std::fs::write(dir.join("pdf.lua"), PDF_TOOLS).unwrap();
    sb.write("a.pdf", "%PDF-1.4 a");
    sb.write("b.PDF", "%PDF-1.4 b");
    sb.write("c.docx", "doc");
    sb.write("d.txt", "text");
    let host = LuaHost::new(Tools {
        programs: Programs::default(),
        meta: None,
    })
    .with_user_dir(dir);
    assert_eq!(host.load(), Vec::new());
    (sb, host)
}

fn ctx(sb: &Sandbox, names: &[&str]) -> RecipeContext {
    RecipeContext {
        selection: names.iter().map(|n| sb.path(n)).collect(),
        marked: Vec::new(),
        cwd: sb.work.clone(),
        os: Os::Linux,
    }
}

fn states(host: &LuaHost, ctx: &RecipeContext) -> BTreeMap<String, Availability> {
    host.recipes(ctx)
        .into_iter()
        .map(|r| (r.name, r.state))
        .collect()
}

#[test]
fn only_the_recipes_for_the_files_chosen_are_there() {
    let (sb, host) = setup();
    with_lang(Lang::En, || {
        let one_pdf = states(&host, &ctx(&sb, &["a.pdf"]));
        assert_eq!(one_pdf.keys().collect::<Vec<_>>(), ["merge", "rotate"]);
        assert_eq!(
            one_pdf["merge"],
            Availability::Disabled("choose at least 2 items".into())
        );
        assert_eq!(one_pdf["rotate"], Availability::Available);

        let two = states(&host, &ctx(&sb, &["a.pdf", "b.PDF"]));
        assert_eq!(two["merge"], Availability::Available);
        assert_eq!(
            two["rotate"],
            Availability::Disabled("for one file at a time".into())
        );

        let doc = states(&host, &ctx(&sb, &["c.docx"]));
        assert_eq!(
            doc["office"],
            Availability::Disabled("LibreOffice is not installed".into())
        );
        assert!(states(&host, &ctx(&sb, &["a.pdf", "d.txt"])).is_empty());
        assert!(states(&host, &ctx(&sb, &["missing.pdf"])).is_empty());
        let nothing = states(&host, &ctx(&sb, &[]));
        assert_eq!(nothing.keys().collect::<Vec<_>>(), ["folder"]);
    });
    with_lang(Lang::It, || {
        let infos = host.recipes(&ctx(&sb, &["a.pdf"]));
        let merge = infos.iter().find(|r| r.name == "merge").unwrap();
        assert_eq!(merge.title, "Unisci PDF");
        assert_eq!(
            merge.state,
            Availability::Disabled("scegli almeno 2 elementi".into())
        );
        assert!(matches!(merge.origin, RecipeOrigin::User(_)));
        let rotate = infos.iter().find(|r| r.name == "rotate").unwrap();
        assert_eq!(rotate.args[0].label, "Angolo");
        assert_eq!(
            rotate.args[0].kind,
            ArgKind::Choice(vec!["90".into(), "180".into(), "270".into()])
        );
        assert_eq!(rotate.args[2].kind, ArgKind::Int { min: 1, max: 3 });
    });
}

#[test]
fn a_run_asks_for_programs_with_the_words_of_the_plan() {
    let (sb, host) = setup();
    let ctx = ctx(&sb, &["a.pdf", "b.PDF"]);
    let plan = host
        .run("merge", &ctx, &ArgValues::new(), &Cancel::new())
        .unwrap();
    assert_eq!(plan.recipe, "merge");
    assert_eq!(plan.title.as_deref(), Some("Merge 2"));
    assert_eq!(plan.notes, ["a note"]);
    let [SubRequest::Produce(spec)] = plan.requests.as_slice() else {
        panic!("{plan:?}");
    };
    assert_eq!(spec.inputs, ctx.selection);
    assert_eq!(spec.output, sb.path("a-merged.pdf"));
    let ProducerSpec::Program {
        chain,
        timeout_secs,
        ok_exit,
        collect,
    } = &spec.producer
    else {
        panic!();
    };
    assert_eq!(chain[0].program, "qpdf");
    let lit = |s: &str| Arg::Lit(s.into());
    assert_eq!(
        chain[0].args,
        [
            lit("--empty"),
            lit("--pages"),
            Arg::Inputs,
            lit("--"),
            Arg::Out
        ]
    );
    assert_eq!(
        (*timeout_secs, ok_exit.as_slice(), collect),
        (600, &[0][..], &Collect::Exact)
    );
}

#[test]
fn arguments_take_their_defaults_and_only_the_values_they_allow() {
    let (sb, host) = setup();
    let ctx = ctx(&sb, &["a.pdf"]);
    let mut args = ArgValues::new();
    args.insert("angle".into(), ArgValue::Text("180".into()));
    args.insert("copies".into(), ArgValue::Int(2));
    let plan = host.run("rotate", &ctx, &args, &Cancel::new()).unwrap();
    assert_eq!(plan.requests.len(), 2);
    let SubRequest::Produce(spec) = &plan.requests[1] else {
        panic!()
    };
    let ProducerSpec::Program {
        chain,
        timeout_secs,
        ..
    } = &spec.producer
    else {
        panic!()
    };
    assert_eq!(
        chain[0].args,
        [
            Arg::Input(0),
            Arg::Lit("--rotate=+180:1-z".into()),
            Arg::Out
        ]
    );
    assert_eq!(*timeout_secs, 30);
    assert_eq!(plan.title.as_deref(), Some("Rotate pages"));

    for (name, value) in [
        ("angle", ArgValue::Text("45".into())),
        ("copies", ArgValue::Int(9)),
        ("pages", ArgValue::Int(1)),
        ("unknown", ArgValue::Flag(true)),
    ] {
        let mut args = ArgValues::new();
        args.insert(name.into(), value);
        let err = host.run("rotate", &ctx, &args, &Cancel::new()).unwrap_err();
        assert!(
            matches!(&err, RecipeError::BadArgument { name: n, .. } if n == name),
            "{err:?}"
        );
    }
}

#[test]
fn a_recipe_that_does_not_fit_does_not_run() {
    let (sb, host) = setup();
    let none = ArgValues::new();
    let err = host
        .run("merge", &ctx(&sb, &["a.pdf"]), &none, &Cancel::new())
        .unwrap_err();
    assert!(matches!(err, RecipeError::NotAvailable(_)), "{err:?}");
    let err = host
        .run("office", &ctx(&sb, &["c.docx"]), &none, &Cancel::new())
        .unwrap_err();
    assert_eq!(
        err,
        RecipeError::NotAvailable("LibreOffice is not installed".into())
    );
    let err = host
        .run("hidden", &ctx(&sb, &["a.pdf"]), &none, &Cancel::new())
        .unwrap_err();
    assert!(matches!(err, RecipeError::NotAvailable(_)), "{err:?}");
    let err = host
        .run("nope", &ctx(&sb, &["a.pdf"]), &none, &Cancel::new())
        .unwrap_err();
    assert_eq!(err, RecipeError::Unknown("nope".into()));
}

#[test]
fn the_plan_of_a_recipe_is_planned_like_any_other() {
    let (sb, host) = setup();
    let plan = host
        .run("folder", &ctx(&sb, &[]), &ArgValues::new(), &Cancel::new())
        .unwrap();
    let engine = sb.engine();
    let mut progress = |_: &rada_core::ops::ScanProgress| {};
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let ctl = rada_core::ops::ScanControl {
        cancel: &cancel,
        progress: &mut progress,
    };
    let planned = engine
        .plan_request(&OpRequest::Recipe(plan), None, ctl)
        .unwrap();
    assert_eq!(
        planned.plan.blocking().count(),
        0,
        "{:?}",
        planned.plan.warnings
    );
    assert_eq!(planned.plan.items[0].path, sb.path("a.pdf.sha256"));
    assert!(!sb.path("a.pdf.sha256").exists());
}

#[test]
fn recipes_built_in_come_first_and_keep_their_names() {
    let sb = Sandbox::new();
    let dir = sb.mkdir("recipes");
    let mine = "rada.recipe { name = \"same\", when = { dir = true }, run = function(ctx) rada.mkdir(rada.join(ctx.cwd, \"mine\")) end }";
    std::fs::write(dir.join("same.lua"), mine).unwrap();
    let builtin = Builtin {
        file: "same.lua",
        text: "rada.recipe { name = \"same\", when = { dir = true }, run = function(ctx) rada.mkdir(rada.join(ctx.cwd, \"built\")) end }",
    };
    let host = LuaHost::new(Tools::default())
        .with_builtins(&[builtin])
        .with_user_dir(dir);
    let problems = host.load();
    assert_eq!(problems.len(), 1);
    assert!(problems[0].file.ends_with("same.lua"));
    let infos = host.recipes(&ctx(&sb, &[]));
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].origin, RecipeOrigin::Builtin);
    let plan = host
        .run("same", &ctx(&sb, &[]), &ArgValues::new(), &Cancel::new())
        .unwrap();
    assert!(format!("{:?}", plan.requests).contains("built"));
}

#[test]
fn a_recipe_runs_in_the_background_and_says_when_it_is_done() {
    let (sb, host) = setup();
    let host: std::sync::Arc<dyn PluginHost> = std::sync::Arc::new(host);
    let (tx, rx) = crossbeam_channel::unbounded();
    let ctx = ctx(&sb, &["a.pdf", "b.PDF"]);
    let _cancel =
        rada_core::plugin::run_in_background(host, 7, "merge".into(), ctx, ArgValues::new(), tx);
    let event = rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap();
    let rada_core::events::CoreEvent::Recipe(done) = event else {
        panic!()
    };
    assert_eq!((done.id, done.recipe.as_str()), (7, "merge"));
    assert!(done.result.is_ok());
}

#[test]
fn what_a_recipe_declares_wrong_is_said() {
    let sb = Sandbox::new();
    let dir = sb.mkdir("recipes");
    let cases = [
        ("Bad Name", "name = \"Bad Name\", run = function() end"),
        ("run", "name = \"norun\""),
        (
            "max",
            "name = \"minmax\", when = { min = 3, max = 1 }, run = function() end",
        ),
        (
            "choices",
            "name = \"nochoice\", args = { { name = \"q\", type = \"choice\" } }, run = function() end",
        ),
        (
            "default",
            "name = \"baddefault\", args = { { name = \"q\", choices = { \"a\" }, default = \"b\" } }, run = function() end",
        ),
    ];
    for (i, (_, body)) in cases.iter().enumerate() {
        std::fs::write(
            dir.join(format!("{i}.lua")),
            format!("rada.recipe {{ {body} }}"),
        )
        .unwrap();
    }
    let host = LuaHost::new(Tools::default()).with_user_dir(dir);
    let problems = host.load();
    assert_eq!(problems.len(), cases.len(), "{problems:?}");
    for ((word, _), problem) in cases.iter().zip(&problems) {
        assert!(problem.message.contains(word), "{word}: {problem}");
    }
}
