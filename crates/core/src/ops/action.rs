//! Actions: steps that make a new file by running another program, or by reading files.
//!
//! An action is the one kind of step in which rada is not the one that writes. So it is fenced
//! in: the program is named by a whole path fixed when the plan is made, it writes in a folder
//! of its own next to the destination, and only one file leaves that folder, under a name that
//! was free. What the plan read is checked before the program starts and again after it ends.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fs::Fingerprint;
use crate::pathcodec;

/// A file an action reads, as the plan saw it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputStamp {
    #[serde(with = "pathcodec::path")]
    #[schemars(with = "String")]
    pub path: PathBuf,
    pub expect: Fingerprint,
}

/// What writes the file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum Producer {
    /// One program, or several run one after the other, each reading what the one before
    /// wrote. Only what the last one writes is kept.
    Program {
        chain: Vec<Command>,
        /// The time each program of the chain is given.
        timeout_secs: u64,
        /// The exit codes that mean success.
        ok_exit: Vec<i32>,
        collect: Collect,
    },
    /// The checksum of the one input, in the format of `sha256sum`.
    Checksum { algo: ChecksumAlgo },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChecksumAlgo {
    Sha256,
}

/// One program and what it is given.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Command {
    pub program: ResolvedProgram,
    pub args: Vec<Arg>,
}

/// A program by the name recipes call it and by the file that name stood for when the plan
/// was made: the file is what runs, whatever `PATH` says later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedProgram {
    pub name: String,
    #[serde(with = "pathcodec::path")]
    #[schemars(with = "String")]
    pub path: PathBuf,
}

/// One argument of a program. Paths are filled in when the step runs; each is one argument,
/// whatever characters it holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Arg {
    /// Given as it is.
    Lit(String),
    /// The path of the input with this number, from 0.
    Input(usize),
    /// Every input, one argument each.
    Inputs,
    /// What the program before this one in the chain wrote.
    Prev,
    /// The file this program is to write.
    Out,
    /// The folder the programs work in, for one that names its output by itself.
    OutDir,
    /// LibreOffice's option for a profile of its own inside the work folder, so that it
    /// neither waits for nor changes the person's own.
    ProfileUrl,
}

/// How the file the last program wrote is found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Collect {
    /// It is where [`Arg::Out`] said.
    #[default]
    Exact,
    /// It is the only file with this extension in the work folder ([`Arg::OutDir`]).
    Single { ext: String },
}

/// What becomes of the steps that worked when another of the same plan fails.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    /// What was made stays, and can be undone.
    #[default]
    Keep,
    /// What was made is taken back.
    Rollback,
}

impl Producer {
    /// The programs by the names recipes call them, in the order they run.
    pub fn program_names(&self) -> Vec<&str> {
        match self {
            Producer::Program { chain, .. } => {
                chain.iter().map(|c| c.program.name.as_str()).collect()
            }
            Producer::Checksum { .. } => Vec::new(),
        }
    }
}

/// The folder a step works in: `.rada-work-<tag>` next to `dst`. The tag is the step's own, so
/// that settling an interrupted run removes this folder and no other.
pub fn work_dir(dst: &Path, tag: &str) -> PathBuf {
    dst.with_file_name(format!(".rada-work-{tag}"))
}

/// What is wrong with a chain of programs, if anything: checked when the plan is made, and
/// again when a plan read from elsewhere is run.
pub fn check_chain(chain: &[Command], collect: &Collect, inputs: usize) -> Result<(), String> {
    if chain.is_empty() {
        return Err("no program to run".into());
    }
    for (i, command) in chain.iter().enumerate() {
        let last = i + 1 == chain.len();
        check_command(command, i, last, collect, inputs)?;
    }
    Ok(())
}

fn check_command(
    command: &Command,
    index: usize,
    last: bool,
    collect: &Collect,
    inputs: usize,
) -> Result<(), String> {
    let name = &command.program.name;
    let has = |arg: &Arg| command.args.contains(arg);
    if let Some(n) = command.args.iter().find_map(|a| match a {
        Arg::Input(n) if *n >= inputs => Some(*n),
        _ => None,
    }) {
        return Err(format!("{name} is given input {n}, which does not exist"));
    }
    if index == 0 && has(&Arg::Prev) {
        return Err(format!("{name} runs first: nothing was written before it"));
    }
    let by_itself = last && matches!(collect, Collect::Single { .. });
    if by_itself && !has(&Arg::OutDir) {
        return Err(format!("{name} is not told the folder to write in"));
    }
    if !by_itself && !has(&Arg::Out) {
        return Err(format!("{name} is not told the file to write"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(args: Vec<Arg>) -> Command {
        Command {
            program: ResolvedProgram {
                name: "tool".into(),
                path: PathBuf::from("/usr/bin/tool"),
            },
            args,
        }
    }

    #[test]
    fn a_chain_must_say_where_each_program_writes() {
        let ok = [
            command(vec![Arg::Input(0), Arg::Out]),
            command(vec![Arg::Prev, Arg::Out]),
        ];
        assert!(check_chain(&ok, &Collect::Exact, 1).is_ok());
        assert!(check_chain(&[], &Collect::Exact, 1).is_err());
        let no_out = [command(vec![Arg::Input(0)])];
        assert!(check_chain(&no_out, &Collect::Exact, 1).is_err());
        let single = Collect::Single { ext: "pdf".into() };
        assert!(check_chain(&no_out, &single, 1).is_err());
        let dir = [command(vec![Arg::OutDir, Arg::Input(0)])];
        assert!(check_chain(&dir, &single, 1).is_ok());
    }

    #[test]
    fn a_chain_cannot_read_what_is_not_there() {
        let first_prev = [command(vec![Arg::Prev, Arg::Out])];
        assert!(check_chain(&first_prev, &Collect::Exact, 1).is_err());
        let no_input = [command(vec![Arg::Input(2), Arg::Out])];
        assert!(check_chain(&no_input, &Collect::Exact, 2).is_err());
    }

    #[test]
    fn the_work_folder_is_next_to_the_file_and_named_by_the_step() {
        assert_eq!(
            work_dir(Path::new("/a/b/out.pdf"), "17-3"),
            PathBuf::from("/a/b/.rada-work-17-3")
        );
    }

    #[test]
    fn arguments_are_written_as_short_json() {
        let args = vec![Arg::Lit("-o".into()), Arg::Out, Arg::Input(0), Arg::Inputs];
        assert_eq!(
            serde_json::to_string(&args).unwrap(),
            r#"[{"lit":"-o"},"out",{"input":0},"inputs"]"#
        );
    }
}
