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
    let notes = clean_markdown(&notes);
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

fn clean_markdown(notes: &str) -> String {
    let mut out = Vec::new();
    let mut blank = false;
    for line in notes.lines() {
        let heading = line.trim().starts_with("### ") || line.trim().starts_with("## ");
        if heading && !out.is_empty() && !blank {
            out.push(String::new());
        }
        let line = clean_markdown_line(line.trim());
        if line.is_empty() {
            if !blank {
                out.push(String::new());
            }
            blank = true;
        } else {
            out.push(line);
            blank = false;
        }
    }
    out.join("\n")
}

fn clean_markdown_line(line: &str) -> String {
    let heading = line
        .strip_prefix("### ")
        .or_else(|| line.strip_prefix("## "));
    if let Some(title) = heading {
        return format!("{title}:");
    }
    let bullet = line.strip_prefix("- ").or_else(|| line.strip_prefix("* "));
    let (prefix, body) = bullet.map_or(("", line), |body| ("• ", body));
    format!("{prefix}{}", clean_inline(body))
}

fn clean_inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '*'
            && chars.get(i + 1) == Some(&'*')
            && copy_double_emphasis(&chars, &mut i, &mut out)
        {
            continue;
        }
        if chars[i] == '`' && copy_delimited(&chars, &mut i, '`', &mut out) {
            continue;
        }
        if chars[i] == '*' && copy_delimited(&chars, &mut i, '*', &mut out) {
            continue;
        }
        if chars[i] == '[' && copy_link(&chars, &mut i, &mut out) {
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn copy_double_emphasis(chars: &[char], i: &mut usize, out: &mut String) -> bool {
    if chars.get(*i + 2).is_none_or(|c| c.is_whitespace()) {
        return false;
    }
    let Some(end) = chars[*i + 2..]
        .windows(2)
        .enumerate()
        .find(|(offset, pair)| *pair == ['*', '*'] && !chars[*i + 1 + offset].is_whitespace())
        .map(|(offset, _)| offset + *i + 2)
    else {
        return false;
    };
    if end == *i + 2 || chars[end - 1].is_whitespace() {
        return false;
    }
    out.extend(&chars[*i + 2..end]);
    *i = end + 2;
    true
}

fn copy_delimited(chars: &[char], i: &mut usize, delimiter: char, out: &mut String) -> bool {
    if delimiter == '*' && chars.get(*i + 1).is_none_or(|c| c.is_whitespace()) {
        return false;
    }
    let Some(end) = chars[*i + 1..]
        .iter()
        .enumerate()
        .find(|(offset, c)| {
            **c == delimiter && (delimiter != '*' || !chars[*i + *offset].is_whitespace())
        })
        .map(|(offset, _)| offset)
        .map(|n| n + *i + 1)
    else {
        return false;
    };
    if end == *i + 1 {
        return false;
    }
    out.extend(&chars[*i + 1..end]);
    *i = end + 1;
    true
}

fn copy_link(chars: &[char], i: &mut usize, out: &mut String) -> bool {
    let Some(close) = chars[*i + 1..]
        .iter()
        .position(|c| *c == ']')
        .map(|n| n + *i + 1)
    else {
        return false;
    };
    if chars.get(close + 1) != Some(&'(') {
        return false;
    }
    let Some(end) = chars[close + 2..]
        .iter()
        .position(|c| *c == ')')
        .map(|n| n + close + 2)
    else {
        return false;
    };
    if close == *i + 1 {
        return false;
    }
    out.extend(&chars[*i + 1..close]);
    out.push_str(" (");
    out.extend(&chars[close + 2..end]);
    out.push(')');
    *i = end + 1;
    true
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
    fn markdown_notes_are_plain_text() {
        let lines = notes_lines(
            "Intro\n### Changed\n- *A new icon.* use `rada update` [guide](https://example.test)\n* second\n\n\n## Fixed\n* last",
        );
        let text = lines.join("\n");
        assert!(text.contains("  Intro\n  \n  Changed:\n  • A new icon. use rada update guide (https://example.test)\n  • second\n  \n  Fixed:\n  • last"), "{text}");
        assert!(!text.contains('*'));
        assert!(!text.contains('`'));
    }

    #[test]
    fn release_060_notes_are_readable() {
        let notes = "### Changed\n- **A new icon.** rada has a new logo, still the boat in its bay. The launcher entry on Linux, the application on macOS and the program file on Windows use it, and the README opens with it. On Linux the icon is installed in more sizes (48, 64, 128, 256 and 512 pixels) next to the flat SVG, and `rada setup desktop --remove` takes them all away; the small sizes (16, 24 and 32 pixels) keep the flat drawing, which is sharper there. Nothing in how rada works changes.";
        let text = notes_lines(notes).join("\n");
        assert!(
            text.starts_with("  Changed:\n  • A new icon. rada has a new logo"),
            "{text}"
        );
        assert!(
            text.contains("rada setup desktop --remove takes them all away"),
            "{text}"
        );
    }

    #[test]
    fn isolated_markdown_punctuation_is_preserved() {
        assert_eq!(notes_lines("*\n`"), ["  *", "  `"]);
    }

    #[test]
    fn literal_asterisks_are_preserved() {
        assert_eq!(
            notes_lines("2 * 3 * 4\ncopia *.pdf e *.png"),
            ["  2 * 3 * 4", "  copia *.pdf e *.png"]
        );
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
