//! The nine recipes built into rada, run to the end with programs that are not real.
//!
//! This file has no test harness of its own: it is also the stand-in for qpdf, gs, magick,
//! convert, sips, ffmpeg and soffice. Each test links this binary into its sandbox under those
//! names; started under one of them, it writes down the arguments it was given and writes the
//! file it was asked for. Started under its own name, it runs the tests below.

use std::ffi::OsString;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rada_core::ops::programs::Os;
use rada_core::ops::{Engine, OpRequest, Plan, Programs, ScanControl, Step};
use rada_core::plugin::{
    ArgValue, ArgValues, Availability, Cancel, PluginHost, RecipeContext, RecipeError,
};
use rada_core::testutil::{Sandbox, run_journaled, undo};
use rada_lua::{BUILTINS, LuaHost, Tools};

const FAKES: &[&str] = &[
    "qpdf", "gs", "magick", "convert", "sips", "ffmpeg", "soffice",
];

fn main() {
    let mut args = std::env::args_os();
    let me = PathBuf::from(args.next().unwrap_or_default());
    let name = me
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if FAKES.contains(&name.as_str()) {
        std::process::exit(fake(&me, &name, args.collect()));
    }
    run_tests(std::env::args().skip(1).collect());
}

// ---------------------------------------------------------------------------------------------
// The stand-in
// ---------------------------------------------------------------------------------------------

/// Play `name`: note the call in `calls.log` beside this file, write the output.
fn fake(me: &Path, name: &str, args: Vec<OsString>) -> i32 {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let log = me.parent().unwrap().join("calls.log");
    let line = format!("{name}\u{1f}{}\n", args.join("\u{1f}"));
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .unwrap();
    f.write_all(line.as_bytes()).unwrap();
    let after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .map(|i| PathBuf::from(&args[i + 1]))
    };
    let out = match name {
        "gs" => args
            .iter()
            .find_map(|a| a.strip_prefix("-sOutputFile=").map(PathBuf::from)),
        "sips" => after("--out"),
        "soffice" => {
            let stem = Path::new(args.last().unwrap())
                .file_stem()
                .unwrap()
                .to_owned();
            after("--outdir").map(|d| d.join(stem).with_extension("pdf"))
        }
        _ => args.last().map(PathBuf::from),
    };
    std::fs::write(out.expect("an output"), format!("made by {name}\n")).unwrap();
    0
}

// ---------------------------------------------------------------------------------------------
// A small runner, since this file has no harness
// ---------------------------------------------------------------------------------------------

type Test = (&'static str, fn());

const TESTS: &[Test] = &[
    ("merge_pdf_joins_the_pdfs_in_the_order_chosen", merge_pdf),
    ("extract_pages_takes_only_page_ranges", extract_pages),
    ("rotate_pages_turns_all_pages_unless_told", rotate_pages),
    (
        "compress_pdf_asks_ghostscript_for_the_quality",
        compress_pdf,
    ),
    ("convert_image_uses_imagemagick", convert_image),
    (
        "convert_image_uses_sips_on_a_mac_without_imagemagick",
        convert_image_sips,
    ),
    (
        "resize_image_makes_one_file_per_photo_in_one_plan",
        resize_images,
    ),
    ("resize_image_with_sips", resize_image_sips),
    ("extract_audio_writes_an_m4a", extract_audio),
    (
        "document_to_pdf_collects_what_libreoffice_names",
        document_to_pdf,
    ),
    ("sha256_runs_no_program", sha256),
    (
        "an_action_whose_program_is_missing_is_not_there",
        hidden_without_programs,
    ),
    ("the_output_never_replaces_a_file", never_replaces),
    #[cfg(target_os = "linux")]
    ("convert_stands_in_for_magick_on_linux", convert_fallback),
];

fn run_tests(args: Vec<String>) {
    let filter = args.iter().find(|a| !a.starts_with('-'));
    if args.iter().any(|a| a == "--list") {
        for (name, _) in TESTS {
            println!("{name}: test");
        }
        return;
    }
    let start = Instant::now();
    let (mut passed, mut failed, mut out) = (0, Vec::new(), 0);
    for (name, test) in TESTS {
        if filter.is_some_and(|f| !name.contains(f.as_str())) {
            out += 1;
            continue;
        }
        match std::panic::catch_unwind(AssertUnwindSafe(test)) {
            Ok(()) => {
                println!("test {name} ... ok");
                passed += 1;
            }
            Err(_) => {
                println!("test {name} ... FAILED");
                failed.push(*name);
            }
        }
    }
    let result = if failed.is_empty() { "ok" } else { "FAILED" };
    println!(
        "\ntest result: {result}. {passed} passed; {} failed; 0 ignored; 0 measured; {out} filtered out; finished in {:.2}s",
        failed.len(),
        start.elapsed().as_secs_f64()
    );
    if !failed.is_empty() {
        std::process::exit(101);
    }
}

// ---------------------------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------------------------

struct Scene {
    sb: Sandbox,
    bin: PathBuf,
    programs: Programs,
}

impl Scene {
    /// A sandbox where only the programs `fakes` exist, as stand-ins.
    fn with(fakes: &[&str]) -> Scene {
        let sb = Sandbox::new();
        let bin = sb.mkdir("bin");
        let empty = sb.mkdir("nothing-installed");
        let me = std::env::current_exe().unwrap();
        let mut overrides = std::collections::BTreeMap::new();
        for name in fakes {
            let file = bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            if std::fs::hard_link(&me, &file).is_err() {
                std::fs::copy(&me, &file).unwrap();
            }
            overrides.insert(name.to_string(), file);
        }
        let programs = Programs::new(overrides).with_path(empty.into_os_string());
        Scene { sb, bin, programs }
    }

    fn host(&self) -> LuaHost {
        let tools = Tools {
            programs: self.programs.clone(),
            meta: None,
        };
        let host = LuaHost::new(tools).with_builtins(BUILTINS);
        assert_eq!(host.load(), Vec::new());
        host
    }

    fn engine(&self) -> Engine {
        self.sb.engine().with_programs(self.programs.clone())
    }

    fn file(&self, name: &str) -> PathBuf {
        self.sb.write(name, format!("the bytes of {name}"))
    }

    fn ctx(&self, files: &[&PathBuf], os: Os) -> RecipeContext {
        RecipeContext {
            selection: files.iter().map(|p| p.to_path_buf()).collect(),
            marked: Vec::new(),
            cwd: self.sb.work.clone(),
            os,
        }
    }

    /// The calls the stand-ins got, in order: the name, then the arguments.
    fn calls(&self) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(self.bin.join("calls.log")).unwrap_or_default();
        text.lines()
            .map(|l| l.split('\u{1f}').map(str::to_string).collect())
            .collect()
    }

    /// Run the recipe on `ctx` with `args`, plan it, run the plan: the plan, as run.
    fn run(&self, recipe: &str, ctx: &RecipeContext, args: &[(&str, &str)]) -> (Plan, String) {
        let values: ArgValues = args
            .iter()
            .map(|(k, v)| (k.to_string(), ArgValue::Text(v.to_string())))
            .collect();
        let host = self.host();
        let infos = host.recipes(ctx);
        let info = infos
            .iter()
            .find(|r| r.name == recipe)
            .expect("the recipe is offered");
        assert_eq!(info.state, Availability::Available);
        let asked = host
            .run(recipe, ctx, &values, &Cancel::new())
            .expect("the recipe runs");
        let engine = self.engine();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let ctl = ScanControl {
            cancel: &cancel,
            progress: &mut |_| {},
        };
        let plan = engine
            .plan_request(&OpRequest::Recipe(asked), None, ctl)
            .unwrap()
            .plan;
        assert_eq!(plan.blocking().count(), 0, "{:?}", plan.warnings);
        let journal = self.sb.journal();
        let outcome = run_journaled(&engine, &journal, &plan);
        assert!(
            outcome.report.failed.is_empty(),
            "{:?}",
            outcome.report.failed
        );
        (plan, outcome.id)
    }
}

/// What an argument must be.
enum A<'a> {
    Lit(&'a str),
    In(&'a Path),
    /// The file the program writes, in its work folder, with this extension.
    Out(&'a str),
    OutAfter(&'a str, &'a str),
    OutDir,
    Profile,
}

fn in_work(p: &Path) -> bool {
    p.parent()
        .and_then(|d| d.file_name())
        .is_some_and(|n| n.to_string_lossy().starts_with(".rada-work-"))
}

fn check_call(call: &[String], program: &str, expect: &[A]) {
    assert_eq!(call[0], program, "{call:?}");
    let args = &call[1..];
    assert_eq!(args.len(), expect.len(), "{call:?}");
    for (got, want) in args.iter().zip(expect) {
        let ok = match want {
            A::Lit(s) => got == s,
            A::In(p) => Path::new(got) == *p,
            A::Out(ext) => {
                let p = Path::new(got);
                in_work(p) && p.file_name().unwrap().to_string_lossy() == format!("out.{ext}")
            }
            A::OutAfter(prefix, ext) => got.strip_prefix(prefix).is_some_and(|rest| {
                let p = Path::new(rest);
                in_work(p) && p.file_name().unwrap().to_string_lossy() == format!("out.{ext}")
            }),
            A::OutDir => Path::new(got)
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(".rada-work-")),
            A::Profile => {
                got.starts_with("-env:UserInstallation=file://") && got.ends_with("/profile")
            }
        };
        assert!(ok, "argument {got:?} in {call:?}");
    }
}

fn made_by(path: &Path, program: &str) {
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        format!("made by {program}\n"),
        "{}",
        path.display()
    );
}

fn produced(plan: &Plan) -> usize {
    plan.steps
        .iter()
        .filter(|s| matches!(s, Step::ProduceFile { .. }))
        .count()
}

// ---------------------------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------------------------

fn merge_pdf() {
    let s = Scene::with(&["qpdf"]);
    let (b, a) = (s.file("b.pdf"), s.file("a.pdf"));
    s.run("merge-pdf", &s.ctx(&[&b, &a], Os::Linux), &[]);
    let calls = s.calls();
    assert_eq!(calls.len(), 1);
    use A::*;
    check_call(
        &calls[0],
        "qpdf",
        &[
            Lit("--empty"),
            Lit("--pages"),
            In(&b),
            In(&a),
            Lit("--"),
            Out("pdf"),
        ],
    );
    made_by(&s.sb.path("b-merged.pdf"), "qpdf");
    let host = s.host();
    let one = host.recipes(&s.ctx(&[&a], Os::Linux));
    let merge = one.iter().find(|r| r.name == "merge-pdf").unwrap();
    assert!(matches!(merge.state, Availability::Disabled(_)));
}

fn extract_pages() {
    let s = Scene::with(&["qpdf"]);
    let a = s.file("a.pdf");
    let ctx = s.ctx(&[&a], Os::Linux);
    s.run("extract-pages", &ctx, &[("pages", "1-3, 7,z")]);
    use A::*;
    check_call(
        &s.calls()[0],
        "qpdf",
        &[
            In(&a),
            Lit("--pages"),
            Lit("."),
            Lit("1-3,7,z"),
            Lit("--"),
            Out("pdf"),
        ],
    );
    made_by(&s.sb.path("a-pages.pdf"), "qpdf");
    let host = s.host();
    for bad in ["--version", "1;2", "", "1-3,", "all"] {
        let args: ArgValues = [("pages".to_string(), ArgValue::Text(bad.into()))].into();
        let err = host
            .run("extract-pages", &ctx, &args, &Cancel::new())
            .unwrap_err();
        let RecipeError::Script { line, message, .. } = err else {
            panic!("{bad}: {err:?}")
        };
        assert_eq!(line, None, "{message}");
        assert!(message.contains("not pages"), "{message}");
    }
    let err = host
        .run("extract-pages", &ctx, &ArgValues::new(), &Cancel::new())
        .unwrap_err();
    assert!(matches!(err, RecipeError::BadArgument { .. }), "{err:?}");
}

fn rotate_pages() {
    let s = Scene::with(&["qpdf"]);
    let a = s.file("a.pdf");
    let ctx = s.ctx(&[&a], Os::Linux);
    s.run("rotate-pages", &ctx, &[]);
    use A::*;
    check_call(
        &s.calls()[0],
        "qpdf",
        &[In(&a), Lit("--rotate=+90:1-z"), Out("pdf")],
    );
    made_by(&s.sb.path("a-rotated.pdf"), "qpdf");
    s.run(
        "rotate-pages",
        &ctx,
        &[("angle", "270"), ("pages", "2,4-5")],
    );
    check_call(
        &s.calls()[1],
        "qpdf",
        &[In(&a), Lit("--rotate=+270:2,4-5"), Out("pdf")],
    );
    assert!(s.sb.path("a-rotated (1).pdf").exists());
}

fn compress_pdf() {
    let s = Scene::with(&["gs"]);
    let a = s.file("a.pdf");
    s.run(
        "compress-pdf",
        &s.ctx(&[&a], Os::Linux),
        &[("quality", "screen")],
    );
    use A::*;
    check_call(
        &s.calls()[0],
        "gs",
        &[
            Lit("-sDEVICE=pdfwrite"),
            Lit("-dPDFSETTINGS=/screen"),
            Lit("-dNOPAUSE"),
            Lit("-dBATCH"),
            Lit("-dSAFER"),
            OutAfter("-sOutputFile=", "pdf"),
            In(&a),
        ],
    );
    made_by(&s.sb.path("a-screen.pdf"), "gs");
}

fn convert_image() {
    let s = Scene::with(&["magick"]);
    let (photo, png) = (s.file("photo.jpeg"), s.file("icon.png"));
    s.run(
        "convert-image",
        &s.ctx(&[&photo, &png], Os::Linux),
        &[("format", "png")],
    );
    let calls = s.calls();
    use A::*;
    check_call(&calls[0], "magick", &[In(&photo), Out("png")]);
    check_call(&calls[1], "magick", &[In(&png), Out("png")]);
    made_by(&s.sb.path("photo.png"), "magick");
    made_by(&s.sb.path("icon-converted.png"), "magick");
    assert_eq!(
        std::fs::read_to_string(&png).unwrap(),
        "the bytes of icon.png"
    );
}

fn convert_image_sips() {
    let s = Scene::with(&["sips"]);
    let photo = s.file("photo.heic");
    let mac = s.ctx(&[&photo], Os::MacOs);
    s.run("convert-image", &mac, &[("format", "jpg")]);
    use A::*;
    check_call(
        &s.calls()[0],
        "sips",
        &[
            Lit("-s"),
            Lit("format"),
            Lit("jpeg"),
            In(&photo),
            Lit("--out"),
            Out("jpg"),
        ],
    );
    made_by(&s.sb.path("photo.jpg"), "sips");
    let host = s.host();
    let webp: ArgValues = [("format".to_string(), ArgValue::Text("webp".into()))].into();
    let err = host
        .run("convert-image", &mac, &webp, &Cancel::new())
        .unwrap_err();
    assert!(err.to_string().contains("WebP"), "{err}");
    let linux = host.recipes(&s.ctx(&[&photo], Os::Linux));
    assert!(
        linux.iter().all(|r| r.name != "convert-image"),
        "sips is only for a Mac"
    );
}

fn resize_images() {
    let s = Scene::with(&["magick"]);
    let photos: Vec<PathBuf> = (1..=3).map(|n| s.file(&format!("IMG_{n}.JPG"))).collect();
    let refs: Vec<&PathBuf> = photos.iter().collect();
    let (plan, id) = s.run(
        "resize-image",
        &s.ctx(&refs, Os::Linux),
        &[("size", "1080")],
    );
    assert_eq!(produced(&plan), 3, "one plan, a file for each photo");
    let calls = s.calls();
    use A::*;
    for (call, photo) in calls.iter().zip(&photos) {
        check_call(
            call,
            "magick",
            &[In(photo), Lit("-resize"), Lit("1080x1080>"), Out("jpg")],
        );
    }
    let made: Vec<PathBuf> = (1..=3)
        .map(|n| s.sb.path(format!("IMG_{n}-1080.jpg")))
        .collect();
    made.iter().for_each(|p| made_by(p, "magick"));
    let (_, report) = undo(&s.engine(), &s.sb.journal(), &id);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(
        made.iter().all(|p| !p.exists()),
        "one undo takes the three back"
    );
    assert!(photos.iter().all(|p| p.exists()));
}

fn resize_image_sips() {
    let s = Scene::with(&["sips"]);
    let photo = s.file("p.png");
    s.run("resize-image", &s.ctx(&[&photo], Os::MacOs), &[]);
    use A::*;
    check_call(
        &s.calls()[0],
        "sips",
        &[Lit("-Z"), Lit("1600"), In(&photo), Lit("--out"), Out("png")],
    );
    made_by(&s.sb.path("p-1600.png"), "sips");
}

fn extract_audio() {
    let s = Scene::with(&["ffmpeg"]);
    let clip = s.file("holiday.MOV");
    s.run("extract-audio", &s.ctx(&[&clip], Os::Linux), &[]);
    use A::*;
    check_call(
        &s.calls()[0],
        "ffmpeg",
        &[
            Lit("-nostdin"),
            Lit("-n"),
            Lit("-i"),
            In(&clip),
            Lit("-vn"),
            Lit("-c:a"),
            Lit("aac"),
            Lit("-b:a"),
            Lit("192k"),
            Out("m4a"),
        ],
    );
    made_by(&s.sb.path("holiday.m4a"), "ffmpeg");
}

fn document_to_pdf() {
    let s = Scene::with(&["soffice"]);
    let doc = s.file("letter.docx");
    s.run("document-to-pdf", &s.ctx(&[&doc], Os::Linux), &[]);
    use A::*;
    check_call(
        &s.calls()[0],
        "soffice",
        &[
            Lit("--headless"),
            Lit("--convert-to"),
            Lit("pdf"),
            Lit("--outdir"),
            OutDir,
            Profile,
            In(&doc),
        ],
    );
    made_by(&s.sb.path("letter.pdf"), "soffice");
}

fn sha256() {
    let s = Scene::with(&[]);
    let file = s.sb.write("hello.txt", "hello");
    s.run("sha256", &s.ctx(&[&file], Os::Linux), &[]);
    assert!(s.calls().is_empty());
    let line = std::fs::read_to_string(s.sb.path("hello.txt.sha256")).unwrap();
    assert_eq!(
        line,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  hello.txt\n"
    );
}

fn hidden_without_programs() {
    let s = Scene::with(&[]);
    let files = ["a.pdf", "b.pdf", "c.png", "d.mp4", "e.odt"].map(|n| s.file(n));
    let host = s.host();
    for os in [Os::Linux, Os::MacOs, Os::Windows] {
        for f in &files {
            let names: Vec<String> = host
                .recipes(&s.ctx(&[f], os))
                .into_iter()
                .map(|r| r.name)
                .collect();
            assert_eq!(names, ["sha256"], "{} on {os:?}", f.display());
        }
        let both: Vec<String> = host
            .recipes(&s.ctx(&[&files[0], &files[1]], os))
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(both, ["sha256"]);
    }
    let with = Scene::with(&["qpdf", "gs", "magick", "ffmpeg", "soffice"]);
    let host = with.host();
    let pdf = with.file("a.pdf");
    let names: Vec<String> = host
        .recipes(&with.ctx(&[&pdf], Os::Linux))
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(
        names,
        [
            "compress-pdf",
            "extract-pages",
            "merge-pdf",
            "rotate-pages",
            "sha256"
        ]
    );
}

fn never_replaces() {
    let s = Scene::with(&["qpdf"]);
    let (a, b) = (s.file("a.pdf"), s.file("b.pdf"));
    let taken = s.sb.write("a-merged.pdf", "mine");
    s.run("merge-pdf", &s.ctx(&[&a, &b], Os::Linux), &[]);
    assert_eq!(std::fs::read_to_string(&taken).unwrap(), "mine");
    made_by(&s.sb.path("a-merged (1).pdf"), "qpdf");
}

#[cfg(target_os = "linux")]
fn convert_fallback() {
    let s = Scene::with(&[]);
    let bin = s.sb.mkdir("path");
    let me = std::env::current_exe().unwrap();
    std::fs::hard_link(&me, bin.join("convert"))
        .or_else(|_| std::fs::copy(&me, bin.join("convert")).map(|_| ()))
        .unwrap();
    let s = Scene {
        programs: Programs::default().with_path(bin.clone().into_os_string()),
        bin,
        sb: s.sb,
    };
    let photo = s.file("p.gif");
    s.run(
        "convert-image",
        &s.ctx(&[&photo], Os::Linux),
        &[("format", "webp")],
    );
    use A::*;
    check_call(&s.calls()[0], "convert", &[In(&photo), Out("webp")]);
    made_by(&s.sb.path("p.webp"), "convert");
}
