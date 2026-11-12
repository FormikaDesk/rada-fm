//! How the lists that are not a folder are filled: a search, the items of a tag, the files
//! used lately.
//!
//! Each is a [`ViewSource`]: something that finds entries and hands them, as it finds them, to
//! a [`ViewSink`]. The source knows nothing of threads, channels or screens; the sink is asked
//! now and then whether to stop. A search hands its matches over in batches while it walks;
//! the tag view and the recent files have theirs all at once and hand over one batch. What
//! could not be read is not an error of the whole list: it is a [`Problem`], and the list is
//! made of the rest.

use std::path::PathBuf;
use std::time::Duration;

use crate::ViewKind;
use crate::fs::FsEngine;
use crate::model::Entry;
use crate::platform::Platform;
use crate::search::SearchSource;
use crate::tagsearch::{self, TagQuery};
use crate::{recents, search};

/// A folder that could not be looked into, and why in a few words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub path: PathBuf,
    pub reason: String,
}

/// A bound that kept a list from being complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// This many matches were found: there may be more.
    Results(usize),
    /// This many entries were looked at.
    Visited(usize),
    /// It took longer than this.
    Time(Duration),
    /// Folders nested deeper than this were not entered.
    Depth(usize),
}

/// How far a source has got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewProgress {
    pub found: usize,
    pub visited: usize,
}

/// How a source ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewEnd {
    /// The bounds it ran into, each once.
    pub limits: Vec<Limit>,
    /// It was asked to stop: what it had found is the list.
    pub cancelled: bool,
}

/// What a source works with.
pub struct ViewCx<'a> {
    pub fs: &'a dyn FsEngine,
    pub platform: &'a dyn Platform,
    /// Whether hidden items are listed, and hidden folders entered.
    pub show_hidden: bool,
}

/// Where a source puts what it finds.
pub trait ViewSink {
    /// More entries, in the order they were found.
    fn batch(&mut self, entries: Vec<Entry>);
    fn problem(&mut self, problem: Problem);
    fn progress(&mut self, progress: ViewProgress);
    /// Whether nobody wants the list any more, or the person asked to stop.
    fn cancelled(&self) -> bool;
}

/// Something that fills a list.
pub trait ViewSource: Send {
    /// Find the entries and hand them to `sink`. An error is a list that cannot be made at
    /// all (no such folder, a pattern that cannot be read).
    fn run(&self, cx: &ViewCx<'_>, sink: &mut dyn ViewSink) -> Result<ViewEnd, String>;
}

/// The source of the list `kind` stands for.
pub fn source(kind: &ViewKind) -> Box<dyn ViewSource> {
    match kind {
        ViewKind::Recent => Box::new(RecentSource),
        ViewKind::Tag(name) => Box::new(TagSource(TagQuery::new(name))),
        ViewKind::Search { root, query } => Box::new(SearchSource {
            root: root.clone(),
            query: query.clone(),
            limits: search::SearchLimits::default(),
        }),
    }
}

/// The items that carry a tag.
struct TagSource(TagQuery);

impl ViewSource for TagSource {
    fn run(&self, cx: &ViewCx<'_>, sink: &mut dyn ViewSink) -> Result<ViewEnd, String> {
        let cancel = || sink.cancelled();
        let (entries, problems) =
            tagsearch::with_problems(|| tagsearch::entries(cx.fs, cx.platform, &self.0, &cancel));
        let entries = entries?;
        for problem in problems {
            sink.problem(problem);
        }
        hand_over(sink, entries)
    }
}

/// The files used lately.
struct RecentSource;

impl ViewSource for RecentSource {
    fn run(&self, cx: &ViewCx<'_>, sink: &mut dyn ViewSink) -> Result<ViewEnd, String> {
        let cancel = || sink.cancelled();
        let entries = recents::entries(cx.fs, cx.platform, &cancel)?;
        hand_over(sink, entries)
    }
}

/// A list that was found all at once: one batch.
fn hand_over(sink: &mut dyn ViewSink, entries: Vec<Entry>) -> Result<ViewEnd, String> {
    let cancelled = sink.cancelled();
    sink.progress(ViewProgress {
        found: entries.len(),
        visited: entries.len(),
    });
    sink.batch(entries);
    Ok(ViewEnd {
        limits: Vec::new(),
        cancelled,
    })
}

/// A sink that keeps everything: for callers that want the whole list, and for the tests.
pub struct Collected<'a> {
    pub entries: Vec<Entry>,
    /// How many batches the entries came in.
    pub batches: usize,
    pub problems: Vec<Problem>,
    pub progress: ViewProgress,
    cancel: &'a dyn Fn() -> bool,
}

impl<'a> Collected<'a> {
    pub fn new(cancel: &'a dyn Fn() -> bool) -> Collected<'a> {
        Collected {
            entries: Vec::new(),
            batches: 0,
            problems: Vec::new(),
            progress: ViewProgress::default(),
            cancel,
        }
    }
}

impl ViewSink for Collected<'_> {
    fn batch(&mut self, entries: Vec<Entry>) {
        self.batches += 1;
        self.entries.extend(entries);
    }

    fn problem(&mut self, problem: Problem) {
        self.problems.push(problem);
    }

    fn progress(&mut self, progress: ViewProgress) {
        self.progress = progress;
    }

    fn cancelled(&self) -> bool {
        (self.cancel)()
    }
}
