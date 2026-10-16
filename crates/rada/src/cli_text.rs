//! The words of `rada --help` and of each command's help, in the language in use.
//!
//! clap builds the help from the doc comments of `main.rs`, which are English. After it has
//! built the command, this puts the texts of the table in their place: the description of the
//! program and of every command and option, the headings of the sections and the template of
//! the page itself. What clap says on its own when the line typed is wrong stays English.

use clap::{Arg, Command};
use rada_core::i18n::Key;

/// The text of an option, by the command it belongs to and its name.
fn arg_text(command: &str, arg: &str) -> Option<Key> {
    Some(match (command, arg) {
        ("rada", "path") => Key::CliArgPath,
        ("rada", "spawn_terminal") => Key::CliArgSpawn,
        ("rada", "icons") => Key::CliArgIcons,
        ("rada", "profile") => Key::CliArgProfile,
        ("rada", "images") => Key::CliArgImages,
        ("rada", "theme") => Key::CliArgTheme,
        ("rada", "appearance") => Key::CliArgAppearance,
        ("rada", "no_mouse") => Key::CliArgNoMouse,
        ("rada", "keymap") => Key::CliArgKeymap,
        ("rada", "request") => Key::CliArgRequest,
        ("rada", "schema") => Key::CliArgSchema,
        ("rada", "hidden") => Key::CliArgHidden,
        ("rada", "cwd_file") => Key::CliArgCwdFile,
        ("rada", "init") => Key::CliArgInit,
        ("rada", "chooser_file") => Key::CliArgChooserFile,
        ("rada", "chooser_multiple") => Key::CliArgChooserMultiple,
        ("rada", "chooser_dir") => Key::CliArgChooserDir,
        ("rada", "chooser_save") => Key::CliArgChooserSave,
        ("update", "check") => Key::CliArgCheck,
        ("update", "rollback") => Key::CliArgRollback,
        ("update", "yes") => Key::CliArgYes,
        ("desktop", "remove") => Key::CliArgRemoveDesktop,
        ("desktop", "default_file_manager") => Key::CliArgDefaultFm,
        ("desktop", "desktop") => Key::CliArgDesktopLauncher,
        ("font", "remove") => Key::CliArgRemoveFont,
        ("file-picker", "remove") => Key::CliArgRemoveFilePicker,
        ("file-picker", "yes") => Key::CliArgFilePickerYes,
        (_, "help") => Key::CliHelpFlag,
        (_, "version") => Key::CliVersionFlag,
        _ => return None,
    })
}

/// What a command is about, by its name.
fn about_text(command: &str) -> Option<Key> {
    Some(match command {
        "rada" => Key::CliAbout,
        "setup" => Key::CliCmdSetup,
        "update" => Key::CliCmdUpdate,
        "doctor" => Key::CliCmdDoctor,
        "desktop" => Key::CliSetupDesktop,
        "font" => Key::CliSetupFont,
        "file-picker" => Key::CliSetupFilePicker,
        "help" => Key::CliHelpSubcommand,
        _ => return None,
    })
}

/// The line of "Usage:", with the words of the placeholders in the language in use.
fn usage_text(command: &str) -> Option<String> {
    let options = Key::CliPlaceholderOptions.tr();
    let sub = Key::CliPlaceholderCommand.tr();
    let path = Key::CliPlaceholderPath.tr();
    Some(match command {
        "rada" => format!("rada [{options}] [{path}]...\n       rada <{sub}>"),
        "setup" => format!("rada setup <{sub}>"),
        "update" => format!("rada update [{options}]"),
        "desktop" => format!("rada setup desktop [{options}]"),
        "font" => format!("rada setup font [{options}]"),
        "file-picker" => format!("rada setup file-picker [{options}]"),
        _ => return None,
    })
}

/// Put the texts of the table into a command and, one by one, into its subcommands. `cmd` must
/// have been built (`Command::build`) so that clap's own `help` is there to be translated.
pub fn localise(cmd: Command) -> Command {
    let name = cmd.get_name().to_string();
    let heading_commands = Key::CliHeadingCommands.tr();
    let heading_options = Key::CliHeadingOptions.tr();
    let heading_args = Key::CliHeadingArguments.tr();
    let mut cmd = cmd
        .help_template(format!(
            "{{about-with-newline}}\n{} {{usage}}\n\n{{all-args}}{{after-help}}",
            Key::CliUsage.tr()
        ))
        .subcommand_help_heading(heading_commands)
        .mut_args(move |a: Arg| {
            let id = a.get_id().to_string();
            let a = match arg_text(&name, &id) {
                Some(k) => a.help(k.tr()),
                None => a,
            };
            if a.is_positional() {
                a.help_heading(heading_args)
            } else {
                a.help_heading(heading_options)
            }
        });
    if let Some(k) = about_text(cmd.get_name()) {
        cmd = cmd.about(k.tr());
    }
    if let Some(usage) = usage_text(cmd.get_name()) {
        cmd = cmd.override_usage(usage);
    }
    let subs: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    for sub in subs {
        cmd = cmd.mut_subcommand(sub, localise);
    }
    cmd
}

/// The command line of rada, parsed in the language in use.
pub fn command<C: clap::CommandFactory>() -> Command {
    let mut cmd = C::command();
    cmd.build();
    localise(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser, Subcommand};
    use rada_core::i18n::{Lang, with_lang};

    #[derive(Parser)]
    #[command(name = "rada", version)]
    struct Probe {
        #[command(subcommand)]
        command: Option<Sub>,
        /// Folder to open.
        path: Vec<std::path::PathBuf>,
        /// Show hidden files at startup.
        #[arg(long)]
        hidden: bool,
    }

    #[derive(Subcommand)]
    enum Sub {
        /// Say what rada found out.
        Doctor,
    }

    fn help(l: Lang) -> String {
        with_lang(l, || {
            let mut cmd = command::<Probe>();
            cmd.render_help().to_string()
        })
    }

    #[test]
    fn the_help_page_speaks_the_language_in_use() {
        let it = help(Lang::It);
        for want in [
            "Uso:",
            "Comandi:",
            "Argomenti:",
            "Opzioni:",
            "Mostra i file nascosti all'avvio.",
            "Cartella da aprire",
            "Mostra l'aiuto",
            "Mostra la versione",
            "Dice che cosa rada ha scoperto",
        ] {
            assert!(it.contains(want), "{want:?} missing:\n{it}");
        }
        for english in [
            "Usage:",
            "Options:",
            "Arguments:",
            "Commands:",
            "Print help",
        ] {
            assert!(!it.contains(english), "{english:?} left in:\n{it}");
        }
        let en = help(Lang::En);
        assert!(en.contains("Usage:") && en.contains("Options:") && en.contains("Print help"));
        let _ = Probe::command();
    }
}
