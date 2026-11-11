//! Requests for actions: one new file made by a program or computed from files, and several
//! requests that are to be one operation.
//!
//! Like every request these say what is wanted and nothing about how. A program is named the
//! way recipes name it (`qpdf`); the plan fixes the file that name stands for, the files that
//! are read as they are now, and the name of the one file that will be written.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::action::{
    self, Arg, ChecksumAlgo, Collect, Command, FailurePolicy, InputStamp, Producer,
};
use super::engine::Engine;
use super::names;
use super::plan::*;
use super::programs::Unresolved;
use super::request::{OpRequest, Planned};
use super::scan::ScanControl;
use crate::fs::FileKind;
use crate::i18n::{Key, tf};
use crate::{Error, Result, archive, display, pathcodec};

/// The longest a program of a chain may be given.
const MAX_TIMEOUT_SECS: u64 = 24 * 3600;

fn default_timeout() -> u64 {
    600
}

fn default_ok_exit() -> Vec<i32> {
    vec![0]
}

/// One new file to make.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProduceSpec {
    /// The files that are read. They are never changed.
    #[serde(with = "pathcodec::paths")]
    #[schemars(with = "Vec<String>")]
    pub inputs: Vec<PathBuf>,
    /// The file to write (a whole path). If the name is taken when the file is ready, it
    /// gets the next free one (`name (1).ext`): nothing is ever replaced.
    #[serde(with = "pathcodec::path")]
    #[schemars(with = "String")]
    pub output: PathBuf,
    pub producer: ProducerSpec,
    /// About how many bytes the file will be, to check the free space.
    #[serde(default)]
    pub size_hint: Option<u64>,
}

/// What is to write the file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "by", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProducerSpec {
    /// One program, or several run one after the other.
    Program {
        chain: Vec<ProgramCall>,
        /// Seconds each program is given (default 600).
        #[serde(default = "default_timeout")]
        timeout_secs: u64,
        /// Exit codes that mean success (default 0).
        #[serde(default = "default_ok_exit")]
        ok_exit: Vec<i32>,
        #[serde(default)]
        collect: Collect,
    },
    /// The checksum of the one input, as `sha256sum` writes it.
    Checksum { algo: ChecksumAlgo },
}

/// A program by the name recipes call it (`qpdf`, `gs`, `magick`, `ffmpeg`, `soffice`…, or
/// one the configuration names), and its arguments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProgramCall {
    pub program: String,
    pub args: Vec<Arg>,
}

/// Several requests as one operation: one plan, one entry in the journal, one undo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecipePlan {
    /// The name of the recipe that asks.
    pub recipe: String,
    pub requests: Vec<SubRequest>,
    /// The title of the plan; without one it is made from the requests.
    #[serde(default)]
    pub title: Option<String>,
    /// Lines the recipe wants shown with the plan.
    #[serde(default)]
    pub notes: Vec<String>,
    /// What becomes of what was made when a step fails.
    #[serde(default)]
    pub failure: FailurePolicy,
}

/// One of the requests of a recipe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubRequest {
    /// Any request but an undo or another recipe.
    Op(OpRequest),
    Produce(ProduceSpec),
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(message.into()))
}

impl ProduceSpec {
    /// Cheap checks that need no filesystem access.
    pub fn validate(&self) -> Result<()> {
        if !self.output.is_absolute() || self.output.file_name().is_none() {
            return invalid("the output must be the whole path of a file");
        }
        if self.inputs.is_empty() {
            return invalid("no inputs given");
        }
        if let Some(p) = self.inputs.iter().find(|p| !p.is_absolute()) {
            let p = display::path(p);
            return invalid(format!("inputs must be absolute paths, got {p}"));
        }
        match &self.producer {
            ProducerSpec::Checksum { .. } if self.inputs.len() != 1 => {
                invalid("a checksum is of one file")
            }
            ProducerSpec::Checksum { .. } => Ok(()),
            ProducerSpec::Program {
                timeout_secs,
                ok_exit,
                collect,
                ..
            } => validate_program(*timeout_secs, ok_exit, collect),
        }
    }
}

fn validate_program(timeout_secs: u64, ok_exit: &[i32], collect: &Collect) -> Result<()> {
    if timeout_secs == 0 || timeout_secs > MAX_TIMEOUT_SECS {
        return invalid(format!(
            "timeout_secs must be between 1 and {MAX_TIMEOUT_SECS}"
        ));
    }
    if ok_exit.is_empty() {
        return invalid("ok_exit names no exit code");
    }
    if let Collect::Single { ext } = collect
        && (ext.is_empty() || !ext.chars().all(|c| c.is_ascii_alphanumeric()))
    {
        return invalid("collect.single.ext must be an extension without its dot");
    }
    Ok(())
}

impl RecipePlan {
    /// Cheap checks that need no filesystem access.
    pub fn validate(&self) -> Result<()> {
        if self.requests.is_empty() {
            return invalid("the recipe asks for nothing");
        }
        if self.failure == FailurePolicy::Rollback {
            return invalid(
                "failure: \"rollback\" is not available yet; with \"keep\" what was made stays and can be undone",
            );
        }
        for request in &self.requests {
            match request {
                SubRequest::Op(OpRequest::Undo { .. }) => {
                    return invalid("a recipe cannot undo");
                }
                SubRequest::Op(OpRequest::Recipe(_)) => {
                    return invalid("a recipe cannot hold another");
                }
                SubRequest::Op(op) => op.validate()?,
                SubRequest::Produce(spec) => spec.validate()?,
            }
        }
        Ok(())
    }
}

/// What a plan for one new file is built from.
struct Draft<'a> {
    engine: &'a Engine,
    spec: &'a ProduceSpec,
    ws: WarningSet,
}

impl Draft<'_> {
    fn block(&mut self, kind: WarningKind, path: &Path) {
        self.ws.add(kind, Severity::Blocking, Some(path));
    }

    fn block_with(&mut self, kind: WarningKind, path: Option<&Path>, detail: String) {
        self.ws
            .add_with(kind, Severity::Blocking, path, Some(detail));
    }

    /// Where the file goes: a name that can be made, in a folder that can be written, not on
    /// a phone or a camera.
    fn check_place(&mut self) {
        let (engine, output) = (self.engine, &self.spec.output);
        let name = output.file_name().unwrap_or_default();
        if let Err(why) = names::validate(&engine.platform.path_rules(), name) {
            self.block_with(WarningKind::InvalidName, Some(output), why);
        }
        let on_device = |p: &PathBuf| engine.platform.on_device(p);
        if on_device(output) || self.spec.inputs.iter().any(on_device) {
            self.block(WarningKind::DeviceAction, output);
        }
        let Some(parent) = output.parent() else {
            return;
        };
        match engine.fs.lstat(parent) {
            Ok(m) if m.is_dir() => {
                if !engine.fs.can_write(parent) {
                    self.block(WarningKind::NotWritable, parent);
                }
            }
            _ => self.block(WarningKind::Missing, parent),
        }
        self.check_space(parent);
    }

    fn check_space(&mut self, parent: &Path) {
        if let Some(need) = self.spec.size_hint
            && let Ok(free) = self.engine.fs.available_space(parent)
            && need > free
        {
            let detail = format!(
                "need {}, {} free",
                display::bytes(need),
                display::bytes(free)
            );
            self.ws.add_with(
                WarningKind::LowSpace,
                Severity::Warning,
                Some(parent),
                Some(detail),
            );
        }
    }

    /// The inputs as they are now. Only files: what a link points to, or what is in a folder,
    /// could change without the plan seeing it.
    fn stamps(&mut self) -> Vec<InputStamp> {
        let mut stamps = Vec::new();
        for path in &self.spec.inputs {
            match self.engine.fs.lstat(path) {
                Ok(m) if m.kind == FileKind::File => stamps.push(InputStamp {
                    path: path.clone(),
                    expect: m.fingerprint(),
                }),
                Ok(_) => self.block(WarningKind::SpecialFile, path),
                Err(_) => self.block(WarningKind::Missing, path),
            }
        }
        stamps
    }

    /// The producer with every program turned into a file; `None` when one is not installed
    /// (the plan says which).
    fn producer(&mut self, inputs: usize) -> Result<Option<Producer>> {
        let (chain, timeout_secs, ok_exit, collect) = match &self.spec.producer {
            ProducerSpec::Checksum { algo } => {
                return Ok(Some(Producer::Checksum { algo: *algo }));
            }
            ProducerSpec::Program {
                chain,
                timeout_secs,
                ok_exit,
                collect,
            } => (chain, *timeout_secs, ok_exit.clone(), collect.clone()),
        };
        let mut resolved = Vec::new();
        for call in chain {
            match self.engine.programs.resolve(&call.program) {
                Ok(program) => resolved.push(Command {
                    program,
                    args: call.args.clone(),
                }),
                Err(Unresolved::Missing(name)) => {
                    self.block_with(WarningKind::ProgramMissing, None, display::text(&name));
                }
                Err(other) => return invalid(other.to_string()),
            }
        }
        if resolved.len() != chain.len() {
            return Ok(None);
        }
        action::check_chain(&resolved, &collect, inputs).map_err(Error::Invalid)?;
        self.ws
            .add(WarningKind::ActionProgram, Severity::Info, None);
        Ok(Some(Producer::Program {
            chain: resolved,
            timeout_secs,
            ok_exit,
            collect,
        }))
    }
}

/// The title of the plan for one new file.
fn produce_title(spec: &ProduceSpec) -> String {
    let out = spec
        .output
        .file_name()
        .map(display::name)
        .unwrap_or_default();
    match &spec.producer {
        ProducerSpec::Checksum { .. } => {
            let of = spec.inputs.first().and_then(|p| p.file_name());
            tf(
                Key::TitleChecksum,
                &[&of.map(display::name).unwrap_or_default()],
            )
        }
        ProducerSpec::Program { chain, .. } => {
            let programs: Vec<&str> = chain.iter().map(|c| c.program.as_str()).collect();
            tf(
                Key::TitleProduce,
                &[&out, &display::text(&programs.join(", "))],
            )
        }
    }
}

impl Engine {
    /// The plan that makes one new file. Reads the filesystem, changes nothing.
    pub fn plan_produce(&self, spec: &ProduceSpec) -> Result<Plan> {
        spec.validate()?;
        let inside = |p: &PathBuf| archive::is_inside(&*self.fs, p);
        if let Some(p) = std::iter::once(&spec.output)
            .chain(&spec.inputs)
            .find(|p| inside(p))
        {
            let p = display::path(p);
            return invalid(format!(
                "{p} is inside an archive: an action reads and writes files in folders"
            ));
        }
        let mut plan = Plan::empty(OpKind::Action, produce_title(spec));
        let mut draft = Draft {
            engine: self,
            spec,
            ws: WarningSet::default(),
        };
        draft.check_place();
        let inputs = draft.stamps();
        let producer = draft.producer(spec.inputs.len())?;
        plan.destination = spec.output.parent().map(Path::to_path_buf);
        plan.totals.items = 1;
        plan.totals.files = 1;
        plan.items.push(produced_item(spec));
        if let Some(producer) = producer
            && inputs.len() == spec.inputs.len()
        {
            plan.steps.push(Step::ProduceFile {
                dst: spec.output.clone(),
                producer,
                inputs,
                size_hint: spec.size_hint,
            });
        }
        plan.warnings = draft.ws.finish();
        Ok(plan)
    }

    /// The one plan of a recipe: each request planned by itself, then merged.
    pub fn plan_recipe(&self, recipe: &RecipePlan, ctl: ScanControl<'_>) -> Result<Planned> {
        recipe.validate()?;
        let mut parts = Vec::new();
        for request in &recipe.requests {
            parts.push(match request {
                SubRequest::Produce(spec) => self.plan_produce(spec)?,
                SubRequest::Op(op) => {
                    let ctl = ScanControl {
                        cancel: ctl.cancel,
                        progress: &mut *ctl.progress,
                    };
                    self.plan_request(op, None, ctl)?.plan
                }
            });
        }
        let title = match (&recipe.title, parts.as_slice()) {
            (Some(title), _) => display::text(title),
            (None, [only]) => only.title.clone(),
            (None, _) => tf(Key::TitleRecipe, &[&display::text(&recipe.recipe)]),
        };
        let mut plan = Plan::merge(parts, title);
        plan.notes
            .extend(recipe.notes.iter().map(|n| display::text(n)));
        plan.failure = recipe.failure;
        Ok(Planned {
            plan,
            scan: None,
            undo: None,
        })
    }
}

/// The line of the plan for the file that will be written.
fn produced_item(spec: &ProduceSpec) -> ItemSummary {
    ItemSummary {
        path: spec.output.clone(),
        kind: FileKind::File,
        action: ItemAction::Create,
        files: 1,
        dirs: 0,
        symlinks: 0,
        bytes: spec.size_hint.unwrap_or(0),
        target: None,
    }
}
