//! The closed state: what a recipe has, what it does not, and its limits.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rada_core::ops::programs::Os;
use rada_core::plugin::{
    ArgValues, Cancel, Limit, LoadProblem, PluginHost, RecipeContext, RecipeError,
};
use rada_core::testutil::Sandbox;
use rada_lua::{Budget, Limits, LuaHost, Tools};

fn load(sb: &Sandbox, files: &[(&str, &str)]) -> (LuaHost, Vec<LoadProblem>) {
    host_with(sb, files, Limits::default())
}

fn host_with(sb: &Sandbox, files: &[(&str, &str)], limits: Limits) -> (LuaHost, Vec<LoadProblem>) {
    let dir = sb.mkdir("recipes");
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let host = LuaHost::new(Tools::default())
        .with_user_dir(dir)
        .with_limits(limits);
    let problems = host.load();
    (host, problems)
}

fn ctx(sb: &Sandbox, selection: Vec<PathBuf>) -> RecipeContext {
    RecipeContext {
        selection,
        marked: Vec::new(),
        cwd: sb.work.clone(),
        os: Os::current(),
    }
}

fn run(
    host: &LuaHost,
    name: &str,
    ctx: &RecipeContext,
) -> Result<rada_core::ops::RecipePlan, RecipeError> {
    host.run(name, ctx, &ArgValues::new(), &Cancel::new())
}

/// A recipe for any file whose `run` is `body`.
fn recipe(body: &str) -> String {
    format!(
        "rada.recipe {{\n  name = \"probe\",\n  when = {{ kind = \"any\", dir = true }},\n  run = function(ctx, args)\n{body}\n  end,\n}}\n"
    )
}

fn script_error(e: RecipeError) -> (Option<u32>, String) {
    match e {
        RecipeError::Script { line, message, .. } => (line, message),
        other => panic!("expected an error of the script, got {other:?}"),
    }
}

#[test]
fn what_is_forbidden_is_not_there() {
    let sb = Sandbox::new();
    let absent = [
        "io",
        "os",
        "package",
        "require",
        "debug",
        "load",
        "loadfile",
        "dofile",
        "collectgarbage",
        "print",
        "coroutine",
        "_G",
        "loadstring",
        "module",
        "xpcall",
        "string.find",
        "string.match",
        "string.gmatch",
        "string.gsub",
        "string.dump",
        "math.random",
        "math.randomseed",
    ];
    let mut body = String::from("local out = {}\n");
    for name in absent {
        let (first, rest) = name
            .split_once('.')
            .map_or((name, None), |(a, b)| (a, Some(b)));
        let expr = match rest {
            Some(r) => format!("{first}.{r}"),
            None => first.to_string(),
        };
        body.push_str(&format!(
            "if {expr} ~= nil then error(\"{name} is there\") end\n"
        ));
    }
    body.push_str("if (\"x\").find ~= nil or (\"x\").gsub ~= nil or (\"x\").dump ~= nil then error(\"through the metatable\") end\n");
    body.push_str(
        "if getmetatable(\"\") ~= false then error(\"the string metatable is reachable\") end\n",
    );
    body.push_str("assert(string.upper(\"a\") == \"A\" and (\"b\"):upper() == \"B\")\n");
    body.push_str(
        "assert(math.floor and table.concat and utf8.char and pairs and pcall and setmetatable)\n",
    );
    body.push_str("assert(tostring and tonumber and select and rawget and rawset and rawlen and rawequal and next and ipairs and type and assert and error)\n");
    body.push_str("rada.mkdir(rada.join(ctx.cwd, \"ok\"))\n");
    let (host, problems) = load(&sb, &[("probe.lua", &recipe(&body))]);
    assert!(problems.is_empty(), "{problems:?}");
    let plan = run(&host, "probe", &ctx(&sb, vec![])).expect("the probe runs");
    assert_eq!(plan.requests.len(), 1);
}

#[test]
fn a_loop_without_end_is_stopped_even_inside_pcall() {
    let sb = Sandbox::new();
    let limits = Limits {
        run: Budget {
            instructions: 2_000_000,
            time: Duration::from_secs(60),
        },
        ..Limits::default()
    };
    let body = "while true do pcall(function() while true do end end) end";
    let (host, _) = host_with(&sb, &[("probe.lua", &recipe(body))], limits);
    let start = Instant::now();
    assert_eq!(
        run(&host, "probe", &ctx(&sb, vec![])),
        Err(RecipeError::Limit(Limit::Instructions))
    );
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_recipe_out_of_time_is_stopped() {
    let sb = Sandbox::new();
    let limits = Limits {
        run: Budget {
            instructions: u64::MAX,
            time: Duration::from_millis(300),
        },
        ..Limits::default()
    };
    let (host, _) = host_with(&sb, &[("probe.lua", &recipe("while true do end"))], limits);
    let start = Instant::now();
    assert_eq!(
        run(&host, "probe", &ctx(&sb, vec![])),
        Err(RecipeError::Limit(Limit::Time))
    );
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn memory_over_the_limit_stops_the_recipe() {
    let sb = Sandbox::new();
    let limits = Limits {
        memory: 4 << 20,
        ..Limits::default()
    };
    let grow = "local t = {}\nfor i = 1, 1e9 do t[i] = string.rep(\"x\", 64) .. i end";
    let rep = "local s = string.rep(\"x\", 1 << 30)";
    let caught =
        "for i = 1, 100 do pcall(function() local t = {} for j = 1, 1e9 do t[j] = j end end) end";
    let files = [
        ("a.lua", recipe(grow)),
        ("b.lua", recipe(rep).replace("probe", "rep")),
        ("c.lua", recipe(caught).replace("probe", "caught")),
    ];
    let files: Vec<(&str, &str)> = files.iter().map(|(n, t)| (*n, t.as_str())).collect();
    let (host, problems) = host_with(&sb, &files, limits);
    assert!(problems.is_empty(), "{problems:?}");
    for name in ["probe", "rep", "caught"] {
        assert_eq!(
            run(&host, name, &ctx(&sb, vec![])),
            Err(RecipeError::Limit(Limit::Memory)),
            "{name}"
        );
    }
}

#[test]
fn a_slow_pattern_cannot_be_written() {
    let sb = Sandbox::new();
    let body = r#"
local ok = pcall(function() return ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"):find(("a*"):rep(12) .. "c") end)
assert(not ok)
local text = string.rep("a", 20000)
assert(rada.match(text, "(a*)*(a*)*(a*)*(a*)*c") == nil)
local y, m = rada.match("photo-2024-05.jpg", "(\\d{4})-(\\d{2})")
assert(y == "2024" and m == "05")
assert(rada.replace("a b  c", " +", "_") == "a_b_c")
assert(#rada.split("a,b,,c", ",") == 4)
rada.mkdir(rada.join(ctx.cwd, "done"))
"#;
    let (host, _) = load(&sb, &[("probe.lua", &recipe(body))]);
    let start = Instant::now();
    run(&host, "probe", &ctx(&sb, vec![])).expect("runs");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn files_that_are_too_big_compiled_or_not_text_are_refused() {
    let sb = Sandbox::new();
    let big = format!(
        "{}\n{}",
        "-- padding\n".repeat(7000),
        recipe("rada.mkdir('/x')")
    );
    let bytecode = "\x1bLuaT\0\x19\u{93}\r\n";
    let (host, problems) = load(&sb, &[("big.lua", &big), ("code.lua", bytecode)]);
    std::fs::write(sb.path("recipes/latin1.lua"), b"-- caff\xe8\n").unwrap();
    let problems2 = host.load();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems
            .iter()
            .any(|p| p.file.ends_with("big.lua") && p.message.contains("64 KiB"))
    );
    assert!(
        problems
            .iter()
            .any(|p| p.file.ends_with("code.lua") && p.message.contains("compiled"))
    );
    assert!(
        problems2
            .iter()
            .any(|p| p.file.ends_with("latin1.lua") && p.message.contains("UTF-8"))
    );
    assert!(host.recipes(&ctx(&sb, vec![])).is_empty());
}

#[test]
fn errors_say_the_file_and_the_line() {
    let sb = Sandbox::new();
    let broken =
        "rada.recipe {\n  name = \"broken\",\n  run = function() end,\n}\nlocal x = nil\nx.y = 1\n";
    let syntax = "rada.recipe {\n  name = \"syntax\"\n  run = 3\n}\n";
    let (_, problems) = load(&sb, &[("broken.lua", broken), ("syntax.lua", syntax)]);
    let shown: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
    let broken = problems
        .iter()
        .find(|p| p.file.ends_with("broken.lua"))
        .expect("{shown:?}");
    assert_eq!(broken.line, Some(6), "{shown:?}");
    let syntax = problems
        .iter()
        .find(|p| p.file.ends_with("syntax.lua"))
        .expect("{shown:?}");
    assert_eq!(syntax.line, Some(3), "{shown:?}");
    assert!(
        shown.iter().any(|s| s.contains("broken.lua:6:")),
        "{shown:?}"
    );

    let sb = Sandbox::new();
    let body = "    local a = 1\n    rada.copy({\"relative/name\"}, ctx.cwd)";
    let (host, _) = load(&sb, &[("probe.lua", &recipe(body))]);
    let (line, message) = script_error(run(&host, "probe", &ctx(&sb, vec![])).unwrap_err());
    assert_eq!(line, Some(6), "{message}");
    assert!(message.contains("not a whole path"), "{message}");
    let (host, _) = host_with(
        &sb,
        &[("probe.lua", &recipe("    error(\"no\")"))],
        Limits::default(),
    );
    assert_eq!(
        script_error(run(&host, "probe", &ctx(&sb, vec![])).unwrap_err()),
        (Some(5), "no".into())
    );
}

#[test]
fn two_runs_give_the_same_plan() {
    let sb = Sandbox::new();
    let body = r#"
local names = {}
for i = 1, 40 do names["k" .. i] = i end
for k, v in pairs(names) do
  rada.mkdir(rada.join(ctx.cwd, k .. "-" .. v))
end
"#;
    let (host, _) = load(&sb, &[("probe.lua", &recipe(body))]);
    let first = run(&host, "probe", &ctx(&sb, vec![])).unwrap();
    for _ in 0..5 {
        assert_eq!(run(&host, "probe", &ctx(&sb, vec![])).unwrap(), first);
    }
    assert_eq!(first.requests.len(), 40);
}

#[test]
fn a_recipe_has_no_way_to_write_a_file() {
    let sb = Sandbox::new();
    let target = sb.path("written.txt");
    let t = target.display().to_string().replace('\\', "\\\\");
    let attempts = [
        format!("io.open(\"{t}\", \"w\")"),
        format!("os.execute(\"touch {t}\")"),
        "require(\"io\")".to_string(),
        "load(\"return 1\")()".to_string(),
        "string.dump(function() end)".to_string(),
        "debug.getregistry()".to_string(),
        "setmetatable({}, { __gc = function() end })".to_string(),
        "getmetatable(\"\").__index = {}".to_string(),
        "collectgarbage()".to_string(),
        "rada.write(\"x\")".to_string(),
    ];
    let mut body = String::from("local failed = 0\n");
    for a in &attempts {
        body.push_str(&format!(
            "if not pcall(function() {a} end) then failed = failed + 1 end\n"
        ));
    }
    body.push_str(&format!(
        "if failed ~= {} then error(\"one attempt worked\") end\n",
        attempts.len()
    ));
    body.push_str("rada.mkdir(rada.join(ctx.cwd, \"asked\"))\n");
    let (host, _) = load(&sb, &[("probe.lua", &recipe(&body))]);
    let plan = run(&host, "probe", &ctx(&sb, vec![])).expect("runs");
    assert!(!target.exists());
    assert!(!sb.path("asked").exists(), "a request is only a request");
    assert_eq!(plan.requests.len(), 1);
}

#[test]
fn plans_are_only_asked_for_in_run() {
    let sb = Sandbox::new();
    let at_load = format!(
        "rada.mkdir(\"{}\")\n{}",
        sb.path("x").display().to_string().replace('\\', "\\\\"),
        recipe("rada.trash(ctx.cwd)")
    );
    let (_, problems) = load(&sb, &[("probe.lua", &at_load)]);
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].message.contains("asked for in run"),
        "{problems:?}"
    );
    assert_eq!(problems[0].line, Some(1));
}

#[test]
fn a_recipe_can_be_stopped_from_outside() {
    let sb = Sandbox::new();
    let limits = Limits {
        run: Budget {
            instructions: u64::MAX,
            time: Duration::from_secs(120),
        },
        ..Limits::default()
    };
    let (host, _) = host_with(&sb, &[("probe.lua", &recipe("while true do end"))], limits);
    let cancel = Cancel::new();
    let stopper = cancel.clone();
    let start = Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        stopper.cancel();
    });
    let result = host.run("probe", &ctx(&sb, vec![]), &ArgValues::new(), &cancel);
    assert_eq!(result, Err(RecipeError::Cancelled));
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_file_with_a_mistake_leaves_the_others() {
    let sb = Sandbox::new();
    let good = recipe("rada.mkdir(rada.join(ctx.cwd, \"a\"))").replace("probe", "good");
    let (host, problems) = load(&sb, &[("a.lua", "this is not lua"), ("b.lua", &good)]);
    assert_eq!(problems.len(), 1);
    let names: Vec<String> = host
        .recipes(&ctx(&sb, vec![]))
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(names, ["good"]);
}

#[test]
fn the_same_name_twice_keeps_the_first() {
    let sb = Sandbox::new();
    let one = recipe("rada.mkdir(rada.join(ctx.cwd, \"one\"))");
    let two = recipe("rada.mkdir(rada.join(ctx.cwd, \"two\"))");
    let (host, problems) = load(&sb, &[("1.lua", &one), ("2.lua", &two)]);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].file.ends_with("2.lua"));
    let plan = run(&host, "probe", &ctx(&sb, vec![])).unwrap();
    assert!(format!("{:?}", plan.requests).contains("one"));
}

/// Where a recipe looks: a path of the sandbox, never one outside.
fn path_text(p: &Path) -> String {
    p.display().to_string().replace('\\', "\\\\")
}

#[test]
fn looking_changes_nothing_and_says_what_is_there() {
    let sb = Sandbox::new();
    sb.write("folder/b.txt", "bb");
    sb.write("folder/a.txt", "a");
    let dir = path_text(&sb.path("folder"));
    let body = format!(
        r#"
local list, more = rada.list("{dir}")
assert(#list == 2 and not more)
assert(list[1].name == "a.txt" and list[1].kind == "file" and list[1].size == 1)
local s = rada.stat(list[2].path)
assert(s.size == 2 and s.mtime > 0)
assert(rada.stat("{dir}/none") == nil)
assert(rada.has("surely-not-a-program") == false)
local m = rada.meta(list[1].path)
assert(m.exif.date == nil and m.audio.title == nil)
assert(rada.name(list[1].path) == "a.txt" and rada.stem(list[1].path) == "a" and rada.ext(list[1].path) == "txt")
rada.trash(list[1].path)
"#
    );
    let (host, _) = load(&sb, &[("probe.lua", &recipe(&body))]);
    run(&host, "probe", &ctx(&sb, vec![])).expect("runs");
    assert!(sb.path("folder/a.txt").exists());
}
