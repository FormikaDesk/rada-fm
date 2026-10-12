//! `rada update`: look for a newer version and install it, from the command line.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Result, bail};
use rada_core::update::{self, Check, Ctx, Mode};

/// The release notes as they are written on GitHub, shortened and indented.
fn show_notes(notes: &str) {
    let notes = notes.trim();
    if notes.is_empty() {
        println!("  (no release notes)");
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
        Ok(Mode::Binary) => "It will download the program for this system, check it against the release's SHA256SUMS, and replace this one (the old one is kept for `rada update --rollback`).".into(),
        Ok(Mode::Source { dir }) => format!(
            "It will update the git checkout in {} (`git pull --ff-only`, on branch main with no uncommitted changes), build it (`cargo build --release --locked`), put the new program in place of this one and run `cargo clean`.",
            dir.display()
        ),
        Ok(Mode::Package { hint }) => hint.clone(),
        Err(e) => e.clone(),
    }
}

fn confirm(question: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("not a terminal: add --yes to update without being asked");
    }
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

pub fn run(ctx: Ctx, check_only: bool, rollback: bool, yes: bool) -> Result<()> {
    if rollback {
        match update::install::read_backup(&ctx.update_dir().join("previous")) {
            Some((_, info)) => println!(
                "Going back from rada {} to rada {}…",
                ctx.current, info.version
            ),
            None => bail!("there is no earlier version saved to go back to"),
        }
        let back = update::rollback(&ctx).map_err(anyhow::Error::msg)?;
        println!("Done: this is rada {back} now. Start rada again to use it.");
        return Ok(());
    }

    println!("rada {} — looking for a newer version…", ctx.current);
    let (found, release) = update::check_full(&ctx, true, update::now_secs());
    let notice = match found {
        Check::UpToDate => {
            println!(
                "rada {} is the newest version for this channel.",
                ctx.current
            );
            return Ok(());
        }
        Check::Failed(e) => bail!("{e}"),
        Check::Newer(n) => n,
    };
    let how = ctx.mode();
    println!(
        "\nrada {} is available{} (this is {}).\n\nWhat is new:",
        notice.version,
        if notice.prerelease {
            " (a pre-release)"
        } else {
            ""
        },
        ctx.current
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
        bail!("the release could not be read again; try once more");
    };
    if !yes && !confirm("\nUpdate now?")? {
        println!("Left as it is.");
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
                println!("Start rada again to use rada {}.", done.version);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("\nThe update did not work: {e}");
            eprintln!(
                "The program that was running is untouched. Log: {}",
                ctx.log_path().display()
            );
            std::process::exit(1);
        }
    }
}
