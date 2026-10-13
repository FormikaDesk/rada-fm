//! The set of background workers the UI talks to, wired to a single event channel.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, unbounded};
use rada_core::archive::ArchiveLimits;
use rada_core::events::CoreEvent;
use rada_core::foldersize::{SizeMode, SizeSettings, SizeWorker};
use rada_core::fs::FsEngine;
use rada_core::jobs::Jobs;
use rada_core::journal::Journal;
use rada_core::ops::Engine;
use rada_core::places::{PlacesStore, standard_places, with_extra_places};
use rada_core::platform::Platform;
use rada_core::watch::DirWatcher;
use rada_core::workers::{DirLoader, PreviewWorker, VolumesWorker};

pub struct Services {
    pub jobs: Jobs,
    pub loader: DirLoader,
    pub previewer: PreviewWorker,
    /// Counts the size of the folders on screen.
    pub sizes: SizeWorker,
    /// Whether and how far folders are counted (off until [`with_folder_sizes`] says).
    ///
    /// [`with_folder_sizes`]: Services::with_folder_sizes
    pub size_settings: SizeSettings,
    pub watcher: DirWatcher,
    pub volumes: VolumesWorker,
    pub places: PlacesStore,
    pub platform: Arc<dyn Platform>,
    pub events: Receiver<CoreEvent>,
    pub home: PathBuf,
    pub journal_ok: bool,
    /// Where background threads of the interface report back (see `CoreEvent::Note`).
    pub notes: crossbeam_channel::Sender<CoreEvent>,
    /// Looks for newer versions and installs them; `None` where updating makes no sense (tests).
    pub updater: Option<rada_core::update::Updater>,
    fs: Arc<dyn FsEngine>,
}

impl Services {
    /// Ask for the subfolders of `dir`; the answer arrives as `CoreEvent::Complete`.
    pub fn complete_dirs(&self, dir: PathBuf) {
        rada_core::workers::complete_dirs(self.fs.clone(), dir, self.notes.clone());
    }
}

impl Services {
    pub fn start(
        fs: Arc<dyn FsEngine>,
        platform: Arc<dyn Platform>,
        journal: Option<Journal>,
    ) -> Services {
        Services::start_with(fs, platform, journal, ArchiveLimits::default())
    }

    /// [`start`](Self::start) with the user's limits for extracting archives.
    pub fn start_with(
        fs: Arc<dyn FsEngine>,
        platform: Arc<dyn Platform>,
        journal: Option<Journal>,
        archive_limits: ArchiveLimits,
    ) -> Services {
        let (tx, rx) = unbounded();
        let engine = Engine::new(fs.clone(), platform.clone()).with_archive_limits(archive_limits);
        let journal_ok = journal.is_some();
        let jobs = Jobs::new(engine, journal.map(Arc::new), tx.clone());
        // Operations a killed process left half done are settled before anything new runs.
        jobs.recover_on_start();
        Services {
            jobs,
            loader: DirLoader::spawn(fs.clone(), platform.clone(), tx.clone()),
            previewer: PreviewWorker::spawn(fs.clone(), tx.clone()),
            sizes: SizeWorker::spawn(fs.clone(), tx.clone()),
            size_settings: SizeSettings {
                mode: SizeMode::Off,
                ..SizeSettings::default()
            },
            watcher: DirWatcher::spawn(tx.clone()),
            places: {
                let standard = with_extra_places(
                    standard_places(
                        &platform.dirs().home,
                        &platform.user_dirs(),
                        &platform.dirs().home_trash().join("files"),
                    ),
                    platform.extra_places(),
                );
                let candidates: Vec<PathBuf> = standard.iter().map(|p| p.path.clone()).collect();
                let for_names = platform.clone();
                PlacesStore::spawn_with_names(
                    platform.dirs().rada_state(),
                    standard,
                    Box::new(move || for_names.display_names(&candidates)),
                    tx.clone(),
                )
            },
            volumes: VolumesWorker::spawn(platform.clone(), Duration::from_secs(5), tx.clone()),
            home: platform.dirs().home.clone(),
            platform,
            events: rx,
            journal_ok,
            notes: tx,
            updater: None,
            fs,
        }
    }

    /// Count the size of folders as the configuration says.
    pub fn with_folder_sizes(mut self, settings: SizeSettings) -> Services {
        self.size_settings = settings;
        self
    }

    /// Start the update worker for this program. The daily check begins when the interface
    /// has started, not here.
    pub fn with_updates(mut self, ctx: rada_core::update::Ctx) -> Services {
        self.updater = Some(rada_core::update::Updater::spawn(ctx, self.notes.clone()));
        self
    }
}
