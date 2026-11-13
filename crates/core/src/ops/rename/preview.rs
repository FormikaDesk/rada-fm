//! The preview of a bulk rename, made on a thread of its own: a pattern that names files by
//! what they say about themselves has to read them, which can take a while (a video asks
//! `ffprobe`), so it is never done while the window is drawn. The reading says how far it is,
//! and a newer question, or none, stops it.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

use super::{Facts, Pattern, Preview};
use crate::events::CoreEvent;
use crate::meta::MetaReader;
use crate::platform::PathRules;

/// What the preview worker says.
#[derive(Clone, Debug)]
pub enum RenamePreview {
    /// `done` of `total` files have been read.
    Reading {
        epoch: u64,
        done: usize,
        total: usize,
    },
    /// The new name of every item, in order, or why it cannot have one.
    Ready { epoch: u64, rows: Vec<Preview> },
}

struct Ask {
    epoch: u64,
    pattern: Pattern,
    items: Arc<Vec<PathBuf>>,
    rules: PathRules,
    /// The language of whoever asked, for the reasons an item cannot be renamed.
    lang: crate::i18n::Lang,
}

/// Makes previews one after the other, always of the newest question.
#[derive(Clone)]
pub struct RenamePreviewer {
    tx: Sender<Ask>,
    newest: Arc<AtomicU64>,
}

/// How often the reading says how far it is.
const TICK: Duration = Duration::from_millis(100);

impl RenamePreviewer {
    pub fn spawn(reader: MetaReader, out: Sender<CoreEvent>) -> RenamePreviewer {
        let (tx, rx) = unbounded::<Ask>();
        let newest = Arc::new(AtomicU64::new(0));
        let current = newest.clone();
        std::thread::Builder::new()
            .name("rada-rename-preview".into())
            .spawn(move || serve(&rx, &reader, &current, &out))
            .expect("spawn rename preview worker");
        RenamePreviewer { tx, newest }
    }

    /// Preview `pattern` on `items`; the answers carry the epoch returned. Any question asked
    /// before is given up.
    pub fn ask(&self, pattern: Pattern, items: Arc<Vec<PathBuf>>, rules: PathRules) -> u64 {
        let epoch = self.newest.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.tx.send(Ask {
            epoch,
            pattern,
            items,
            rules,
            lang: crate::i18n::lang(),
        });
        epoch
    }

    /// Give up the question being answered (the window was closed).
    pub fn stop(&self) {
        self.newest.fetch_add(1, Ordering::SeqCst);
    }
}

fn serve(rx: &Receiver<Ask>, reader: &MetaReader, newest: &AtomicU64, out: &Sender<CoreEvent>) {
    while let Ok(mut ask) = rx.recv() {
        // Only the last of the questions that piled up is worth answering.
        while let Ok(later) = rx.try_recv() {
            ask = later;
        }
        let stale = || newest.load(Ordering::SeqCst) != ask.epoch;
        if stale() {
            continue;
        }
        let made = crate::i18n::with_lang(ask.lang, || rows(&ask, reader, &stale, out));
        let Some(rows) = made else {
            continue;
        };
        let ready = RenamePreview::Ready {
            epoch: ask.epoch,
            rows,
        };
        if out.send(CoreEvent::RenamePreview(ready)).is_err() {
            return;
        }
    }
}

/// Every row of the preview; `None` when a newer question came first.
fn rows(
    ask: &Ask,
    reader: &MetaReader,
    stale: &dyn Fn() -> bool,
    out: &Sender<CoreEvent>,
) -> Option<Vec<Preview>> {
    let (meta, mtime) = (ask.pattern.needs_meta(), ask.pattern.needs_mtime());
    let total = ask.items.len();
    let mut said = Instant::now();
    let mut rows = Vec::with_capacity(total);
    for (i, path) in ask.items.iter().enumerate() {
        if stale() {
            return None;
        }
        let facts = Facts {
            meta: meta.then(|| reader.read(path, stale).1),
            mtime: if mtime {
                std::fs::symlink_metadata(path)
                    .and_then(|m| m.modified())
                    .ok()
            } else {
                None
            },
        };
        rows.push(ask.pattern.preview_one(i, path, &facts, &ask.rules));
        if meta && said.elapsed() >= TICK {
            said = Instant::now();
            let reading = RenamePreview::Reading {
                epoch: ask.epoch,
                done: i + 1,
                total,
            };
            let _ = out.send(CoreEvent::RenamePreview(reading));
        }
    }
    (!stale()).then_some(rows)
}
