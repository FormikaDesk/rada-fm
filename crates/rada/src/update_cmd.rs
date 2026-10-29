//! `rada update`: look for a newer version and install it, from the command line.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Result, bail};
use rada_core::display;
use rada_core::i18n::{Key, tf};
use rada_core::update::{self, Check, Ctx, Mode};

/// The release notes as they are written on GitHub, shortened and indented. They are written
/// by somebody else: what could act on the terminal is made visible.
fn notes_lines(notes: &str) -> Vec<String> {
    let notes = display::block(notes);
    let notes = notes.trim();
    if notes.is_empty() {
        return vec![format!("  {}", Key::UNoNotes.tr())];
    }
    const MAX: usize = 60;
    let mut out: Vec<String> = notes.lines().take(MAX).map(|l| format!("  {l}")).collect();
    if notes.lines().count() > MAX {
        out.push("  …".into());
    }
    out
}

fn what_will_happen(how: &Result<Mode, String>) -> String {
    match how {
        Ok(Mode::Binary) => Key::UBinary.tr().to_string(),
        Ok(Mode::Source { dir }) => tf(Key::USource, &[&display::path(dir)]),
        Ok(Mode::Package { hint }) => display::text(hint),
        Err(e) => display::text(e),
    }
}

/// How an update that failed ends, on the error output: what went wrong, which came from the
/// network or from a program, and where the log is.
fn failure_lines(error: &str, log: &std::path::Path) -> [String; 2] {
    [
        tf(Key::UFailed, &[&display::text(error)]),
        tf(Key::UUntouched, &[&display::path(log)]),
    ]
}

fn confirm(question: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("{}", Key::UNotTerminal.tr());
    }
    print!("{}", tf(Key::UPrompt, &[&question]));
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_lowercase().as_str(),
        "y" | "yes" | "s" | "si" | "s\u{ec}"
    ))
}

pub fn run(ctx: Ctx, check_only: bool, rollback: bool, yes: bool) -> Result<()> {
    if rollback {
        match update::install::read_backup(&ctx.update_dir().join("previous")) {
            Some((_, info)) => println!(
                "{}",
                tf(
                    Key::UGoingBack,
                    &[&ctx.current, &display::text(&info.version)]
                )
            ),
            None => bail!("{}", Key::UpNoEarlier.tr()),
        }
        let back = update::rollback(&ctx).map_err(anyhow::Error::msg)?;
        println!("{}", tf(Key::UDoneBack, &[&display::text(&back)]));
        return Ok(());
    }

    println!("{}", tf(Key::ULooking, &[&ctx.current]));
    let (found, release) = update::check_full(&ctx, true, update::now_secs());
    let notice = match found {
        Check::UpToDate => {
            println!("{}", tf(Key::UNewest, &[&ctx.current]));
            return Ok(());
        }
        Check::Failed(e) => bail!("{e}"),
        Check::Newer(n) => n,
    };
    let how = ctx.mode_for(Some(&notice.version));
    println!(
        "{}",
        tf(
            Key::UAvailable,
            &[
                &display::text(&notice.version),
                &if notice.prerelease {
                    Key::UPreRelease.tr()
                } else {
                    ""
                },
                &ctx.current,
            ],
        )
    );
    for line in notes_lines(&notice.notes) {
        println!("{line}");
    }
    println!("\n{}", what_will_happen(&how));
    if check_only {
        return Ok(());
    }
    match how {
        Ok(Mode::Package { .. }) => return Ok(()),
        Err(e) => bail!("{e}"),
        Ok(_) => {}
    }
    let Some(release) = release else {
        bail!("{}", Key::UReleaseGone.tr());
    };
    if !yes && !confirm(Key::UUpdateNow.tr())? {
        println!("{}", Key::ULeft.tr());
        return Ok(());
    }
    let mut last = String::new();
    let result = update::apply(&ctx, &release, &mut |step| {
        if step != last {
            println!("  {}", display::text(&step));
            last = step;
        }
    });
    match result {
        Ok(done) => {
            println!("\n{}", display::block(&done.note));
            if done.restart {
                println!("{}", tf(Key::URestart, &[&display::text(&done.version)]));
            }
            Ok(())
        }
        Err(e) => {
            for line in failure_lines(&e, &ctx.log_path()) {
                eprintln!("{line}");
            }
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet(lines: &[String]) {
        for l in lines {
            // (A line break is the message's own, or the notes'.)
            assert!(
                !l.chars().any(|c| c != '\n' && display::is_dangerous(c)),
                "{l:?}"
            );
        }
    }

    #[test]
    fn release_notes_cannot_act_on_the_terminal() {
        let notes =
            "Fixes\n\x1b[31mred\x1b[0m\r\n\x1b]52;c;aGk=\x07 copied\n\u{202e}gnp.exe\n\u{9b}2J end";
        let lines = notes_lines(notes);
        quiet(&lines);
        let all = lines.join("\n");
        assert!(all.contains("\\e[31mred"), "the escape is shown: {all}");
        assert!(all.contains("\\e]52;c;aGk=\\u{7} copied"), "{all}");
        assert!(all.contains("\\u{202e}gnp.exe"), "{all}");
        assert!(all.contains("\\u{9b}2J end"), "{all}");
        assert_eq!(lines.len(), 5, "the line breaks stay: {all}");
        assert!(lines.iter().all(|l| l.starts_with("  ")));
    }

    #[test]
    fn long_notes_are_cut_and_empty_ones_say_so() {
        let long = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>();
        let lines = notes_lines(&long.join("\n"));
        assert_eq!(lines.len(), 61);
        assert_eq!(lines[59], "  line 59");
        assert_eq!(lines[60], "  …");
        assert_eq!(notes_lines("  \n ").len(), 1);
        assert!(notes_lines("").concat().contains(Key::UNoNotes.tr()));
    }

    #[test]
    fn the_failure_of_an_update_is_told_without_what_the_network_could_add() {
        let lines = failure_lines(
            "the server said \x1b[2J\x1b[H gone\nand \u{202e}more",
            std::path::Path::new("/state/up\x1bdate.log"),
        );
        quiet(&lines);
        assert!(
            lines[0].contains("\\e[2J\\e[H gone\\nand \\u{202e}more"),
            "{lines:?}"
        );
        assert!(lines[1].contains("up\\edate.log"), "{lines:?}");
    }

    #[test]
    fn what_will_happen_is_clean_whatever_the_package_says() {
        let how = Ok(Mode::Package {
            hint: "run \x1b[1mpacman -Syu\x1b[0m".into(),
        });
        quiet(&[what_will_happen(&how)]);
        quiet(&[what_will_happen(&Err("no\x07".into()))]);
    }
}
