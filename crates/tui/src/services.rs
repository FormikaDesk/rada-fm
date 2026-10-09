//! The set of background workers the UI talks to, wired to a single event channel.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, unbounded};
use rada_core::events::CoreEvent;
use rada_core::fs::FsEngine;
use rada_core::jobs::Jobs;
use rada_core::journal::Journal;
use rada_core::ops::Engine;
use rada_core::places::{PlacesStore, standard_places};
use rada_core::platform::Platform;
use rada_core::watch::DirWatcher;
use rada_core::workers::{DirLoader, PreviewWorker, VolumesWorker};

pub struct Services {
    pub jobs: Jobs,
    pub loader: DirLoader,
    pub previewer: PreviewWorker,
    pub watcher: DirWatcher,
    pub volumes: VolumesWorker,
    pub places: PlacesStore,
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
        let jobs = Jobs::new(engine, journal.map(Arc::new), tx.clone());
        // Operations a killed process left half done are settled before anything new runs.
        jobs.recover_on_start();
        Services {
            jobs,
            loader: DirLoader::spawn(fs.clone(), platform.clone(), tx.clone()),
            previewer: PreviewWorker::spawn(fs, tx.clone()),
            watcher: DirWatcher::spawn(tx.clone()),
            places: PlacesStore::spawn(
                platform.dirs().rada_state(),
                standard_places(
                    &platform.dirs().home,
                    &platform.user_dirs(),
                    &platform.dirs().home_trash().join("files"),
                ),
                tx.clone(),
            ),
            volumes: VolumesWorker::spawn(platform.clone(), Duration::from_secs(5), tx),
            home: platform.dirs().home.clone(),
            platform,
            events: rx,
            journal_ok,
        }
    }
}
