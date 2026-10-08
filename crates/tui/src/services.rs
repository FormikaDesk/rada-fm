//! The set of background workers the UI talks to, wired to a single event channel.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, unbounded};
use vela_core::events::CoreEvent;
use vela_core::fs::FsEngine;
use vela_core::jobs::Jobs;
use vela_core::journal::Journal;
use vela_core::ops::Engine;
use vela_core::platform::Platform;
use vela_core::watch::DirWatcher;
use vela_core::workers::{DirLoader, PreviewWorker, VolumesWorker};

pub struct Services {
    pub jobs: Jobs,
    pub loader: DirLoader,
    pub previewer: PreviewWorker,
    pub watcher: DirWatcher,
    pub volumes: VolumesWorker,
    pub platform: Arc<dyn Platform>,
    pub events: Receiver<CoreEvent>,
    pub home: PathBuf,
    pub journal_ok: bool,
}

impl Services {
    pub fn start(
        fs: Arc<dyn FsEngine>,
        platform: Arc<dyn Platform>,
        journal: Option<Journal>,
    ) -> Services {
        let (tx, rx) = unbounded();
        let engine = Engine::new(fs.clone(), platform.clone());
        let journal_ok = journal.is_some();
        Services {
            jobs: Jobs::new(engine, journal.map(Arc::new), tx.clone()),
            loader: DirLoader::spawn(fs.clone(), platform.clone(), tx.clone()),
            previewer: PreviewWorker::spawn(fs, tx.clone()),
            watcher: DirWatcher::spawn(tx.clone()),
            volumes: VolumesWorker::spawn(platform.clone(), Duration::from_secs(5), tx),
            home: platform.dirs().home.clone(),
            platform,
            events: rx,
            journal_ok,
        }
    }
}
