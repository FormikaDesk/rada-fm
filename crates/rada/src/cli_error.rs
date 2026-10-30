//! What clap says when the command line is wrong, in the language in use.
//!
//! clap writes its errors in English whatever the command is translated to. For the mistakes
//! people make most (an argument or a command that does not exist, a value that is missing or
//! not one of the allowed), the sentence is built here from what clap found out (the
//! argument, the suggestion, the valid values) and laid out the way clap lays it out. Any
//! other error, and English, is left to clap, which also decides the exit code.

use clap::Command;
use clap::builder::styling::Style;
use clap::error::{ContextKind, ContextValue, Error, ErrorKind};
use rada_core::i18n::{Key, Lang, lang, tf};

/// Print `e` and end the program with clap's exit code.
pub fn exit(cmd: &Command, e: Error) -> ! {
    if lang() != Lang::En
        && let Some(text) = localised(cmd, &e)
    {
        anstream::eprint!("{text}");
        std::process::exit(e.exit_code());
    }
    e.exit()
}

/// The error as clap would print it, in the language in use. `None` for an error not made
/// here, or one with something in it that cannot be told in this language.
fn localised(cmd: &Command, e: &Error) -> Option<String> {
    let styles = cmd.get_styles();
    let paint = Paint {
        invalid: *styles.get_invalid(),
        valid: *styles.get_valid(),
    };
    let (message, tips) = match e.kind() {
        ErrorKind::UnknownArgument => unknown_argument(e, &paint)?,
        ErrorKind::InvalidSubcommand => unknown_subcommand(e, &paint)?,
        ErrorKind::InvalidValue => invalid_value(e, &paint)?,
        _ => return None,
    };
    let error = styles.get_error();
    let mut out = format!("{error}{}{error:#} {message}\n", Key::CliErrLabel.tr());
    if !tips.is_empty() {
        out.push('\n');
        for tip in tips {
            out.push_str(&format!("  {}\n", paint.tip(&tip)));
        }
    }
    if let Some(ContextValue::StyledStr(usage)) = e.get(ContextKind::Usage) {
        out.push_str(&format!("\n{}\n", usage_text(&usage.to_string())));
    }
    let literal = styles.get_literal();
    let help = format!("{literal}--help{literal:#}");
    Some(format!("{out}\n{}\n", tf(Key::CliMoreInfo, &[&help]),))
}

/// The colours of the parts of a message.
struct Paint {
    invalid: Style,
    valid: Style,
}

impl Paint {
    fn invalid(&self, text: &str) -> String {
        format!("{}{text}{:#}", self.invalid, self.invalid)
    }

    fn valid(&self, text: &str) -> String {
        format!("{}{text}{:#}", self.valid, self.valid)
    }

    /// A list of suggestions or values: each in quotes, in the colour of what is right.
    fn quoted(&self, items: &[String]) -> String {
        let quoted: Vec<String> = items
            .iter()
            .map(|i| format!("'{}'", self.valid(i)))
            .collect();
        quoted.join(", ")
    }

    fn tip(&self, text: &str) -> String {
        format!("{} {text}", self.valid(Key::CliTipLabel.tr()))
    }
}

/// clap's own "Usage:" is in English; the lines under it are already ours.
fn usage_text(clap_usage: &str) -> String {
    let rest = clap_usage.strip_prefix("Usage:").unwrap_or(clap_usage);
    format!("{} {}", Key::CliUsage.tr(), rest.trim_start())
}

/// One value of the error's context, or the strings of a list (never empty).
fn context(e: &Error, kind: ContextKind) -> Vec<String> {
    match e.get(kind) {
        Some(ContextValue::String(s)) => vec![s.clone()],
        Some(ContextValue::Strings(v)) => v.clone(),
        _ => Vec::new(),
    }
}

fn one(e: &Error, kind: ContextKind) -> Option<String> {
    context(e, kind).into_iter().next()
}

/// "A similar … exists" for what clap found close to what was typed.
fn similar(paint: &Paint, found: &[String], one: Key, many: Key) -> Option<String> {
    let key = match found.len() {
        0 => return None,
        1 => one,
        _ => many,
    };
    Some(tf(key, &[&paint.quoted(found)]))
}

/// The tips clap wrote itself, as the text they have in English: "to pass 'x' as a value,
/// use '-- x'" and "'sub --flag' exists". `None` if there is one this does not know.
fn written_tips(e: &Error) -> Option<Vec<String>> {
    let Some(ContextValue::StyledStrs(tips)) = e.get(ContextKind::Suggested) else {
        return Some(Vec::new());
    };
    tips.iter()
        .map(|t| {
            let t = t.to_string();
            if let Some((arg, instead)) = t
                .strip_prefix("to pass '")
                .and_then(|r| r.split_once("' as a value, use '"))
                .and_then(|(a, i)| Some((a, i.strip_suffix('\'')?)))
            {
                return Some(tf(Key::CliTipPassAsValue, &[&arg, &instead]));
            }
            let name = t.strip_prefix('\'')?.strip_suffix("' exists")?;
            Some(tf(Key::CliTipExists, &[&name]))
        })
        .collect()
}

type Told = (String, Vec<String>);

fn unknown_argument(e: &Error, paint: &Paint) -> Option<Told> {
    let arg = one(e, ContextKind::InvalidArg)?;
    let found = context(e, ContextKind::SuggestedArg);
    let mut tips: Vec<String> = similar(paint, &found, Key::CliTipArgOne, Key::CliTipArgMany)
        .into_iter()
        .collect();
    tips.extend(written_tips(e)?);
    Some((tf(Key::CliErrUnknownArg, &[&paint.invalid(&arg)]), tips))
}

fn unknown_subcommand(e: &Error, paint: &Paint) -> Option<Told> {
    let sub = one(e, ContextKind::InvalidSubcommand)?;
    let found = context(e, ContextKind::SuggestedSubcommand);
    let mut tips: Vec<String> = similar(paint, &found, Key::CliTipSubOne, Key::CliTipSubMany)
        .into_iter()
        .collect();
    tips.extend(written_tips(e)?);
    Some((tf(Key::CliErrUnknownSub, &[&paint.invalid(&sub)]), tips))
}

fn invalid_value(e: &Error, paint: &Paint) -> Option<Told> {
    let arg = one(e, ContextKind::InvalidArg)?;
    let Some(ContextValue::String(value)) = e.get(ContextKind::InvalidValue) else {
        return None;
    };
    let mut message = if value.is_empty() {
        tf(Key::CliErrNoValue, &[&paint.invalid(&arg)])
    } else {
        tf(Key::CliErrBadValue, &[&paint.invalid(value), &arg])
    };
    let valid = context(e, ContextKind::ValidValue);
    if !valid.is_empty() {
        let list = valid.iter().map(|v| paint.valid(v)).collect::<Vec<_>>();
        message.push_str(&format!(
            "\n  {}",
            tf(Key::CliPossibleValues, &[&list.join(", ")])
        ));
    }
    let found = context(e, ContextKind::SuggestedValue);
    let tips = similar(paint, &found, Key::CliTipValueOne, Key::CliTipValueMany);
    Some((message, tips.into_iter().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser, Subcommand};
    use rada_core::i18n::with_lang;

    #[derive(Parser)]
    #[command(name = "rada", version)]
    struct Probe {
        #[command(subcommand)]
        command: Option<Sub>,
        /// Print the schema.
        #[arg(long, value_name = "WHAT", value_parser = ["request", "plan"])]
        schema: Option<String>,
        /// Show hidden files.
        #[arg(long)]
        hidden: bool,
        /// Folder to open.
        path: Vec<std::path::PathBuf>,
    }

    #[derive(Subcommand)]
    enum Sub {
        /// Say what was found.
        Doctor,
        /// Set things up.
        Setup,
    }

    /// What `rada <args>` says in `l`, without colours.
    fn said(l: Lang, args: &[&str]) -> Option<String> {
        said_by(Probe::command(), l, args)
    }

    fn said_by(cmd: Command, l: Lang, args: &[&str]) -> Option<String> {
        let e = cmd
            .clone()
            .try_get_matches_from(std::iter::once("rada").chain(args.iter().copied()))
            .unwrap_err();
        with_lang(l, || localised(&cmd, &e)).map(|t| strip(&t))
    }

    /// `text` without the escape sequences of colours.
    fn strip(text: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for c in text.chars() {
            match (in_escape, c) {
                (false, '\x1b') => in_escape = true,
                (true, 'm') => in_escape = false,
                (false, c) => out.push(c),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn an_argument_that_does_not_exist_is_told_with_its_suggestion() {
        let it = said(Lang::It, &["--hiden"]).unwrap();
        assert!(
            it.starts_with("errore: argomento '--hiden' non riconosciuto\n"),
            "{it}"
        );
        assert!(
            it.contains("  suggerimento: esiste un argomento simile: '--hidden'\n"),
            "{it}"
        );
        assert!(it.contains("\nUso: "), "{it}");
        assert!(
            it.ends_with("Per maggiori informazioni, prova '--help'.\n"),
            "{it}"
        );
        let en = said(Lang::En, &["--hiden"]).unwrap();
        assert!(en.contains("unexpected argument '--hiden' found"), "{en}");
        assert!(
            en.contains("tip: a similar argument exists: '--hidden'"),
            "{en}"
        );
    }

    #[test]
    fn an_argument_without_a_suggestion_says_how_to_pass_it_as_a_value() {
        let it = said(Lang::It, &["--qqq"]).unwrap();
        assert!(it.contains("argomento '--qqq' non riconosciuto"), "{it}");
        assert!(
            it.contains("suggerimento: per passare '--qqq' come valore, usa '-- --qqq'"),
            "{it}"
        );
        assert!(!it.contains("simile"), "{it}");
    }

    #[test]
    fn a_command_that_does_not_exist_is_told_with_or_without_a_suggestion() {
        // As in `rada setup <word>`: at the top of rada a word is a folder to open.
        let setup = || {
            Command::new("rada")
                .subcommand_required(true)
                .subcommand(Command::new("doctor"))
        };
        let it = said_by(setup(), Lang::It, &["docter"]).unwrap();
        assert!(
            it.starts_with("errore: comando 'docter' non riconosciuto\n"),
            "{it}"
        );
        assert!(it.contains("esiste un comando simile: 'doctor'"), "{it}");
        let it = said_by(setup(), Lang::It, &["zzzz"]).unwrap();
        assert!(it.contains("comando 'zzzz' non riconosciuto"), "{it}");
        assert!(!it.contains("simile"), "{it}");
    }

    #[test]
    fn a_value_that_is_missing_or_not_allowed_is_told_with_the_valid_ones() {
        let it = said(Lang::It, &["--schema"]).unwrap();
        assert!(
            it.starts_with("errore: serve un valore per '--schema <WHAT>', ma non è stato dato\n"),
            "{it}"
        );
        let it = said(Lang::It, &["--schema", "reqest"]).unwrap();
        assert!(
            it.contains("valore 'reqest' non valido per '--schema <WHAT>'"),
            "{it}"
        );
        assert!(
            it.contains("\n  [valori possibili: request, plan]\n"),
            "{it}"
        );
        assert!(it.contains("esiste un valore simile: 'request'"), "{it}");
        let it = said(Lang::It, &["--schema", "zzz"]).unwrap();
        assert!(!it.contains("simile"), "{it}");
    }

    #[test]
    fn what_is_not_here_is_left_to_clap() {
        assert!(said(Lang::It, &["--hidden=3"]).is_none());
        assert!(said(Lang::It, &["--help"]).is_none());
    }
}
