//! `rada update`: look for a newer version and install it, from the command line.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Result, bail};
use rada_core::i18n::{Key, tf};
use rada_core::update::{self, Check, Ctx, Mode};

/// The release notes as they are written on GitHub, shortened and indented.
fn show_notes(notes: &str) {
    let notes = notes.trim();
    if notes.is_empty() {
        println!("  {}", Key::UNoNotes.tr());
        return;
    }
    const MAX: usize = 60;
    for (i, line) in notes.lines().enumerate() {
        if i == MAX {
            println!("  …");
            break;
        }
        println!("  {line}");
    }
}

fn what_will_happen(how: &Result<Mode, String>) -> String {
    match how {
        Ok(Mode::Binary) => Key::UBinary.tr().to_string(),
        Ok(Mode::Source { dir }) => tf(Key::USource, &[&dir.display()]),
        Ok(Mode::Package { hint }) => hint.clone(),
        Err(e) => e.clone(),
    }
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
            Some((_, info)) => println!("{}", tf(Key::UGoingBack, &[&ctx.current, &info.version])),
            None => bail!("{}", Key::UpNoEarlier.tr()),
        }
        let back = update::rollback(&ctx).map_err(anyhow::Error::msg)?;
        println!("{}", tf(Key::UDoneBack, &[&back]));
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
    let how = ctx.mode();
    println!(
        "{}",
        tf(
            Key::UAvailable,
            &[
                &notice.version,
                &if notice.prerelease {
                    Key::UPreRelease.tr()
                } else {
                    ""
                },
                &ctx.current,
            ],
        )
    );
    show_notes(&notice.notes);
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
            println!("  {step}");
            last = step;
        }
    });
    match result {
        Ok(done) => {
            println!("\n{}", done.note);
            if done.restart {
                println!("{}", tf(Key::URestart, &[&done.version]));
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", tf(Key::UFailed, &[&e]));
            eprintln!("{}", tf(Key::UUntouched, &[&ctx.log_path().display()]));
            std::process::exit(1);
        }
    }
}
