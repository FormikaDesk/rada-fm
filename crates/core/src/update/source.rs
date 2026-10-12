//! Updating a rada that was built from a git checkout: check the checkout is in a state where
//! a pull is safe, pull, build, put the new program in place of the running one, and clean up
//! the build folder. If any step fails the program that was running is still there, and the
//! log of what happened is kept.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use super::install;
use super::{Applied, Ctx};

/// Where the log of the last update is.
pub fn log_path(state_dir: &Path) -> PathBuf {
    state_dir.join("update").join("last.log")
}

/// The log of one update, written as it goes.
pub struct Log {
    file: Option<File>,
    pub path: PathBuf,
}

impl Log {
    pub fn create(state_dir: &Path) -> Log {
        let path = log_path(state_dir);
        let file = path
            .parent()
            .and_then(|d| fs::create_dir_all(d).ok())
            .and_then(|()| {
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&path)
                    .ok()
            });
        Log { file, path }
    }

    pub fn line(&mut self, text: &str) {
        if let Some(f) = &mut self.file {
            let _ = writeln!(f, "{text}");
        }
    }
}

/// A program by name, looked for in `PATH` and then in `extra` (a program started from the
/// desktop has a short `PATH`, without the folder `rustup` installs cargo in).
fn find_program(name: &OsString, extra: &[PathBuf]) -> OsString {
    let path = Path::new(name);
    if path.components().count() > 1 {
        return name.clone();
    }
    let candidates = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(extra.iter().cloned());
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    for dir in candidates {
        for ext in exts {
            let mut file = name.clone();
            file.push(ext);
            let full = dir.join(&file);
            if full.is_file() {
                return full.into_os_string();
            }
        }
    }
    name.clone()
}

/// Run a command, sending each line of its output to the log and to `on_line`.
fn run_logged(
    cmd: &mut Command,
    log: &mut Log,
    on_line: &mut dyn FnMut(&str),
) -> Result<ExitStatus, String> {
    let shown = format!("{:?}", cmd).replace('"', "");
    log.line(&format!("$ {shown}"));
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", cmd.get_program().to_string_lossy()))?;
    let (tx, rx) = crossbeam_channel::unbounded::<String>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        }));
    }
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            for l in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        }));
    }
    drop(tx);
    while let Ok(line) = rx.recv() {
        log.line(&line);
        on_line(&line);
    }
    for r in readers {
        let _ = r.join();
    }
    child.wait().map_err(|e| {
        format!(
            "cannot wait for {}: {e}",
            cmd.get_program().to_string_lossy()
        )
    })
}

/// Run a command and return what it printed, or its error output when it fails.
fn capture(cmd: &mut Command, log: &mut Log) -> Result<String, String> {
    log.line(&format!("$ {:?}", cmd).replace('"', ""));
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot start {}: {e}", cmd.get_program().to_string_lossy()))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    log.line(&text);
    log.line(&err);
    if out.status.success() {
        Ok(text)
    } else {
        Err(if err.is_empty() { text } else { err })
    }
}

/// The version in the checkout's workspace `Cargo.toml`.
fn checkout_version(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    text.lines().find_map(|l| {
        let rest = l
            .trim()
            .strip_prefix("version")?
            .trim_start()
            .strip_prefix('=')?;
        Some(rest.trim().trim_matches('"').to_string())
    })
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "rada.exe" } else { "rada" }
}

pub fn run(
    ctx: &Ctx,
    dir: &Path,
    log: &mut Log,
    report: &mut dyn FnMut(String),
) -> Result<Applied, String> {
    let extra = vec![ctx.home.join(".cargo").join("bin")];
    let git = find_program(&ctx.git, &extra);
    let cargo = find_program(&ctx.cargo, &extra);
    let mut path_env =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    path_env.splice(0..0, extra);
    let path_env = std::env::join_paths(path_env).unwrap_or_default();
    let git_cmd = || {
        let mut c = Command::new(&git);
        c.arg("-C")
            .arg(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("PATH", &path_env)
            .envs(ctx.extra_env.iter().map(|(k, v)| (k, v)));
        c
    };

    if !dir.is_dir() {
        return Err(format!(
            "the source folder {} does not exist (see `source_dir` in the [update] section of config.toml)",
            dir.display()
        ));
    }

    report("checking the checkout".into());
    capture(git_cmd().args(["rev-parse", "--is-inside-work-tree"]), log)
        .map_err(|e| format!("{} is not a git checkout ({e})", dir.display()))?;
    let changes = capture(
        git_cmd().args(["status", "--porcelain", "--untracked-files=no"]),
        log,
    )?;
    if !changes.is_empty() {
        let n = changes.lines().count();
        return Err(format!(
            "the checkout in {} has {n} changed file{} that are not committed: commit or stash them, then update again. Nothing was changed.",
            dir.display(),
            if n == 1 { "" } else { "s" }
        ));
    }
    let branch = capture(git_cmd().args(["rev-parse", "--abbrev-ref", "HEAD"]), log)?;
    if branch != "main" {
        return Err(format!(
            "the checkout in {} is on the branch `{branch}`, not `main`: switch it to main (`git switch main`), then update again. Nothing was changed.",
            dir.display()
        ));
    }

    // The program that runs now is kept before anything can replace it.
    let run_dir = ctx.update_dir();
    if let Err(e) = install::save_backup(
        &ctx.exe,
        &ctx.current.to_string(),
        &run_dir.join("previous"),
    ) {
        log.line(&format!("no copy of the old program kept: {e}"));
    }

    let before = capture(git_cmd().args(["rev-parse", "HEAD"]), log)?;
    report("pulling the latest changes".into());
    let status = run_logged(git_cmd().args(["pull", "--ff-only"]), log, &mut |_| {})?;
    if !status.success() {
        return Err(format!(
            "`git pull --ff-only` did not work (the checkout may have diverged from its upstream); the log has the details: {}",
            log.path.display()
        ));
    }
    let after = capture(git_cmd().args(["rev-parse", "HEAD"]), log)?;
    let new_version = checkout_version(dir).unwrap_or_else(|| ctx.current.to_string());
    if before == after && new_version == ctx.current.to_string() {
        return Ok(Applied {
            version: new_version,
            note: "The checkout is already at the latest commit and the program was built from it; nothing to build.".into(),
            restart: false,
        });
    }

    // Where the build goes. A program running from `<checkout>/target` cannot be rebuilt in
    // place where the system will not overwrite a running file (Windows), so it builds
    // elsewhere there.
    let in_target = ctx.exe.starts_with(dir.join("target"));
    let own_target = (in_target && cfg!(windows)).then(|| run_dir.join("build"));
    let target_dir = own_target.clone().unwrap_or_else(|| dir.join("target"));
    report("building (this takes a few minutes)".into());
    let mut crates = 0usize;
    let mut build = Command::new(&cargo);
    build
        .current_dir(dir)
        .args(["build", "--release", "--locked"])
        .env("CARGO_TERM_COLOR", "never")
        .env("PATH", &path_env)
        .envs(ctx.extra_env.iter().map(|(k, v)| (k, v)));
    if let Some(t) = &own_target {
        build.env("CARGO_TARGET_DIR", t);
    }
    let mut last_report = std::time::Instant::now() - Duration::from_secs(1);
    let status = run_logged(&mut build, log, &mut |line| {
        let t = line.trim_start();
        if t.starts_with("Compiling") || t.starts_with("Building") {
            crates += t.starts_with("Compiling") as usize;
            if last_report.elapsed() >= Duration::from_millis(400) {
                last_report = std::time::Instant::now();
                let what = t.split_whitespace().nth(1).unwrap_or("");
                report(format!("building: {what} ({crates} compiled)"));
            }
        }
    })?;
    if !status.success() {
        return Err(format!(
            "the build failed; the program that was running is untouched. The log has the details: {}",
            log.path.display()
        ));
    }

    let built = target_dir.join("release").join(exe_name());
    if !built.is_file() {
        return Err(format!("the build left no program at {}", built.display()));
    }
    report("checking the new program".into());
    if ctx.run_check {
        let out = install::run_version(&built, Duration::from_secs(15))?;
        log.line(&format!("new program: {out}"));
        if !out.starts_with("rada ") {
            return Err(format!("the new program says {out:?}, not its version"));
        }
    }

    // The program that was running may be the very file the build has just written.
    let same_file = fs::canonicalize(&built).ok() == fs::canonicalize(&ctx.exe).ok();
    if !same_file {
        report("putting the new program in place".into());
        let stage = install::stage_path(&ctx.exe);
        fs::copy(&built, &stage).map_err(|e| {
            format!(
                "cannot write the new program next to {}: {e}",
                ctx.exe.display()
            )
        })?;
        install::replace_exe(&stage, &ctx.exe)?;
    }

    let mut note = format!(
        "Built from {} and installed at {}.",
        dir.display(),
        ctx.exe.display()
    );
    if in_target && !cfg!(windows) {
        note.push_str(" The build folder was kept because rada runs from it.");
    } else {
        report("cleaning up the build folder".into());
        let mut clean = Command::new(&cargo);
        clean
            .current_dir(dir)
            .arg("clean")
            .env("CARGO_TERM_COLOR", "never")
            .env("PATH", &path_env)
            .envs(ctx.extra_env.iter().map(|(k, v)| (k, v)));
        if let Some(t) = &own_target {
            clean.env("CARGO_TARGET_DIR", t);
        }
        match run_logged(&mut clean, log, &mut |_| {}) {
            Ok(s) if s.success() => {}
            _ => log.line("cargo clean did not work; the build folder was left"),
        }
    }
    Ok(Applied {
        version: new_version,
        note,
        restart: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_from_the_workspace_manifest() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.7.0-rc.1\"\nedition = \"2024\"\n",
        )
        .unwrap();
        assert_eq!(checkout_version(d.path()).as_deref(), Some("0.7.0-rc.1"));
        assert_eq!(checkout_version(&d.path().join("none")), None);
    }

    #[test]
    fn a_program_is_found_in_the_extra_folders() {
        let d = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "fake-tool.exe"
        } else {
            "fake-tool"
        };
        fs::write(d.path().join(name), "").unwrap();
        let found = find_program(&OsString::from("fake-tool"), &[d.path().to_path_buf()]);
        assert_eq!(PathBuf::from(found), d.path().join(name));
        // A name that is nowhere stays as it is, so that the error names it.
        let lost = find_program(&OsString::from("no-such-tool-anywhere"), &[]);
        assert_eq!(lost, OsString::from("no-such-tool-anywhere"));
    }
}
