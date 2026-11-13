//! The set of background workers the UI talks to, wired to a single event channel.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, unbounded};
use rada_core::archive::ArchiveLimits;
use rada_core::events::CoreEvent;
use rada_core::foldersize::{SizeMode, SizeSettings, SizeWorker};
use rada_core::fs::FsEngine;
use rada_core::git::Git;
use rada_core::i18n::{Key, tf};
use rada_core::jobs::Jobs;
use rada_core::journal::Journal;
use rada_core::meta::{MetaLimits, MetaReader, MetaTools, MetaWorker};
use rada_core::ops::{Engine, RenamePreviewer};
use rada_core::places::{PlacesStore, standard_places, with_extra_places};
use rada_core::platform::Platform;
use rada_core::watch::DirWatcher;
use rada_core::workers::{DirLoader, PreviewWorker, VolumesWorker};
use rada_core::zoxide::Zoxide;

pub struct Services {
    pub jobs: Jobs,
    pub loader: DirLoader,
    /// Reads the folder above the one shown, for the first of the Columns view's columns. Its
    /// own worker: the answers to the loader above must not be mistaken for these.
    pub parent_loader: DirLoader,
    pub previewer: PreviewWorker,
    /// Counts the size of the folders on screen.
    pub sizes: SizeWorker,
    /// Reads what the file under the cursor says about itself (EXIF, tags, a video's length).
    /// It runs no program until [`with_meta`](Services::with_meta) says which.
    pub meta: MetaWorker,
    /// Makes the preview of a bulk rename, reading the files through the same cache as
    /// [`meta`](Services::meta) and the plans.
    pub renames: RenamePreviewer,
    /// The reader behind [`meta`](Services::meta) and [`renames`](Services::renames), for
    /// whatever else reads metadata through the same cache (the recipes' `rada.meta`).
    pub meta_reader: MetaReader,
    /// The recipes: none until [`with_plugins`](Services::with_plugins) gives a host.
    pub plugins: Option<Arc<dyn rada_core::plugin::PluginHost>>,
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
    /// How zoxide is called: never, until [`with_zoxide`](Services::with_zoxide) says.
    pub zoxide: Zoxide,
    /// How git is called, for the state of a folder in the panels view: never, until
    /// [`with_git`](Services::with_git) says.
    pub git: Git,
    fs: Arc<dyn FsEngine>,
}

impl Services {
    /// Ask zoxide for the folders that match `text`; the answer arrives as `CoreEvent::Zoxide`.
    pub fn zoxide_query(&self, text: &str) {
        self.zoxide.query(text, self.notes.clone());
    }

    /// Ask git about `dir`; the answer arrives as `CoreEvent::Git`.
    pub fn git_status(&self, dir: &std::path::Path) {
        self.git.status(dir, self.notes.clone());
    }

    /// Tell zoxide that `dir` was entered.
    pub fn zoxide_add(&self, dir: &std::path::Path) {
        self.zoxide.add(dir);
    }

    /// Ask for the subfolders of `dir` with the number of items in each; the answer arrives as
    /// `CoreEvent::Tree` (the NAV box of the panels view).
    pub fn tree_dirs(&self, dir: PathBuf, hidden: bool) {
        rada_core::workers::tree_dirs(self.fs.clone(), dir, hidden, self.notes.clone());
    }

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
        let programs = rada_core::ops::Programs::default();
        Services::start_for(fs, platform, journal, archive_limits, programs)
    }

    /// [`start_with`](Self::start_with) and the programs the configuration names for actions.
    pub fn start_for(
        fs: Arc<dyn FsEngine>,
        platform: Arc<dyn Platform>,
        journal: Option<Journal>,
        archive_limits: ArchiveLimits,
        programs: rada_core::ops::Programs,
    ) -> Services {
        let (tx, rx) = unbounded();
        let engine = Engine::new(fs.clone(), platform.clone())
            .with_archive_limits(archive_limits)
            .with_programs(programs);
        let journal_ok = journal.is_some();
        let reader = MetaReader::new(MetaTools::none(), MetaLimits::default());
        let jobs = Jobs::new(engine, journal.map(Arc::new), tx.clone()).with_meta(reader.clone());
        // Operations a killed process left half done are settled before anything new runs.
        jobs.recover_on_start();
        Services {
            jobs,
            loader: DirLoader::spawn(fs.clone(), platform.clone(), tx.clone()),
            parent_loader: DirLoader::spawn(fs.clone(), platform.clone(), tx.clone()),
            previewer: PreviewWorker::spawn_with(fs.clone(), Some(platform.clone()), tx.clone()),
            sizes: SizeWorker::spawn(fs.clone(), tx.clone()),
            meta: MetaWorker::spawn_with(reader.clone(), tx.clone()),
            renames: RenamePreviewer::spawn(reader.clone(), tx.clone()),
            meta_reader: reader,
            plugins: None,
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
                let for_recent = platform.clone();
                PlacesStore::spawn_with_names(
                    platform.dirs().rada_state(),
                    standard,
                    Box::new(move || {
                        (
                            for_names.display_names(&candidates),
                            for_recent.recent_available(),
                        )
                    }),
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
            zoxide: Zoxide::off(),
            git: Git::off(),
            fs,
        }
    }

    /// Use git as the configuration says.
    pub fn with_git(mut self, git: Git) -> Services {
        self.git = git;
        self
    }

    /// Use zoxide as the configuration says.
    pub fn with_zoxide(mut self, zoxide: Zoxide) -> Services {
        self.zoxide = zoxide;
        self
    }

    /// Read metadata with these programs (`ffprobe`, for videos).
    pub fn with_meta(mut self, tools: MetaTools) -> Services {
        let reader = MetaReader::new(tools, MetaLimits::default());
        self.jobs = self.jobs.with_meta(reader.clone());
        self.meta = MetaWorker::spawn_with(reader.clone(), self.notes.clone());
        self.renames = RenamePreviewer::spawn(reader.clone(), self.notes.clone());
        self.meta_reader = reader;
        self
    }

    /// The recipes, and what could not be read of them: each problem is a notice with its
    /// file and line once the interface is up, and rada goes on without that file.
    pub fn with_plugins(
        mut self,
        host: Arc<dyn rada_core::plugin::PluginHost>,
        problems: &[rada_core::plugin::LoadProblem],
    ) -> Services {
        for problem in problems {
            tracing::warn!("recipe not loaded: {problem}");
        }
        let notice = match problems {
            [] => None,
            [one] => Some(tf(Key::RecipeNotLoaded, &[&one.to_string()])),
            many => Some(tf(
                Key::RecipesNotLoaded,
                &[&many.len().to_string(), &many[0].to_string()],
            )),
        };
        if let Some(text) = notice {
            let _ = self.notes.send(CoreEvent::Note { text, error: true });
        }
        self.plugins = Some(host);
        self
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

#[cfg(test)]
mod tests {
    use super::*;
    use rada_core::i18n::{Lang, with_lang};
    use rada_core::plugin::{
        ArgValues, Cancel, LoadProblem, PluginHost, RecipeContext, RecipeError, RecipeInfo,
    };

    /// A host with no recipes: the interface only knows the trait.
    struct Empty;

    impl PluginHost for Empty {
        fn load(&self) -> Vec<LoadProblem> {
            Vec::new()
        }
        fn recipes(&self, _: &RecipeContext) -> Vec<RecipeInfo> {
            Vec::new()
        }
        fn run(
            &self,
            name: &str,
            _: &RecipeContext,
            _: &ArgValues,
            _: &Cancel,
        ) -> Result<rada_core::ops::RecipePlan, RecipeError> {
            Err(RecipeError::Unknown(name.to_string()))
        }
    }

    fn note(svc: &Services) -> String {
        loop {
            match svc.events.recv_timeout(Duration::from_secs(10)) {
                Ok(CoreEvent::Note { text, error: true }) => return text,
                Ok(_) => continue,
                Err(e) => panic!("no notice: {e}"),
            }
        }
    }

    #[test]
    fn a_recipe_that_could_not_be_read_is_a_notice_with_its_file_and_line() {
        let sb = rada_core::testutil::Sandbox::new();
        let problem = |n: u32| LoadProblem {
            file: sb.path(format!("recipes/bad{n}.lua")),
            line: Some(3),
            message: "attempt to call a nil value".into(),
        };
        let start = || {
            let fs: Arc<dyn FsEngine> = Arc::new(rada_core::fs::LocalFs);
            Services::start_for(
                fs,
                sb.platform(),
                None,
                ArchiveLimits::default(),
                Default::default(),
            )
        };
        with_lang(Lang::En, || {
            let svc = start().with_plugins(Arc::new(Empty), &[problem(1)]);
            assert!(svc.plugins.is_some());
            let text = note(&svc);
            assert!(text.starts_with("Recipe not loaded: "), "{text}");
            assert!(
                text.contains("bad1.lua:3: attempt to call a nil value"),
                "{text}"
            );
        });
        with_lang(Lang::It, || {
            let svc = start().with_plugins(Arc::new(Empty), &[problem(1), problem(2)]);
            let text = note(&svc);
            assert!(text.starts_with("2 problemi nelle ricette"), "{text}");
            assert!(text.contains("bad1.lua:3:"), "{text}");
        });
    }
}
