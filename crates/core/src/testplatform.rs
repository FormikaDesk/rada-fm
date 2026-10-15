//! A platform that behaves like macOS where it matters to the interface, built on top of the
//! sandboxed platform of the machine running the tests: packages, localized names, iCloud
//! Drive, the system asking for consent, ejecting and Quick Look. It does nothing real:
//! what the interface asks it to do is written to a log the tests read.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::Result;
use crate::fs::FsMeta;
use crate::platform::{
    AccessDenied, Attrs, Dirs, Fidelity, FileAttributes, Opener, PathRules, PlaceKind, Platform,
    TagStore, TrashBackend, UserDirs, Volume, VolumeLister,
};

/// What the stand-in says and has.
#[derive(Clone, Default)]
pub struct MacLikeSpec {
    /// Folders whose name ends like this are packages.
    pub package_suffixes: Vec<&'static str>,
    /// The names the "Finder" shows.
    pub names: Vec<(PathBuf, String)>,
    /// A folder standing in for the system's tables of folder names: with it, the standard
    /// folders of the home folder are named in the language of the interface, as on a Mac.
    pub folder_tables: Option<PathBuf>,
    /// The files "Spotlight" says were used lately; `None`: the stand-in has no such list.
    pub recents: Option<Vec<(PathBuf, std::time::SystemTime)>>,
    pub extra_places: Vec<(PlaceKind, PathBuf)>,
    /// Folders the system refuses until the person allows it, with the explanation.
    pub denied: Vec<AccessDenied>,
    /// Disks the lister reports instead of the machine's own; empty keeps the real list.
    pub volumes: Vec<Volume>,
    /// What an eject answers: `None` works, `Some(text)` fails with that text.
    pub eject_error: Option<String>,
    pub quick_look: bool,
}

/// Everything the interface asked of the stand-in, in order: `open_url <url>`,
/// `quick_look <path>`, `eject <mount point>`, `open <path>`.
pub type CallLog = Arc<Mutex<Vec<String>>>;

pub struct MacLike {
    inner: Arc<dyn Platform>,
    spec: MacLikeSpec,
    attrs: Attrs2,
    opener: Opener2,
    volumes: Volumes2,
    pub log: CallLog,
}

struct Attrs2 {
    inner: Arc<dyn Platform>,
    suffixes: Vec<&'static str>,
    folder_tables: Option<PathBuf>,
}

impl FileAttributes for Attrs2 {
    fn attrs(&self, name: &OsStr, meta: &FsMeta) -> Attrs {
        let mut a = self.inner.attributes().attrs(name, meta);
        // The Finder hides dot files, whatever the machine the stand-in runs on does.
        a.hidden |= name.to_string_lossy().starts_with('.');
        a
    }
    fn shown_name(&self, path: &Path, name: &OsStr, meta: &FsMeta) -> Option<std::ffi::OsString> {
        // The Finder's rules, as the real Mac attributes have them.
        crate::platform::macos::localized_folder_name(name, meta.is_dir()).or_else(|| {
            let tables = self.folder_tables.as_deref()?;
            meta.is_dir()
                .then(|| {
                    crate::platform::foldernames::FolderNames::current_with(tables)
                        .shown(path, &self.inner.dirs().home)
                })
                .flatten()
                .map(std::ffi::OsString::from)
        })
    }
    fn is_package(&self, path: &Path, meta: &FsMeta) -> bool {
        meta.is_dir()
            && self
                .suffixes
                .iter()
                .any(|s| path.to_string_lossy().ends_with(s))
    }
}

struct Opener2 {
    inner: Arc<dyn Platform>,
    log: CallLog,
    quick_look: bool,
}

impl Opener for Opener2 {
    fn open(&self, path: &Path) -> Result<()> {
        self.log
            .lock()
            .unwrap()
            .push(format!("open {}", path.display()));
        Ok(())
    }
    fn reveal(&self, path: &Path) -> Result<()> {
        self.inner.opener().reveal(path)
    }
    fn open_url(&self, url: &str) -> Result<()> {
        self.log.lock().unwrap().push(format!("open_url {url}"));
        Ok(())
    }
    fn can_quick_look(&self) -> bool {
        self.quick_look
    }
    fn quick_look(&self, path: &Path) -> Result<()> {
        self.log
            .lock()
            .unwrap()
            .push(format!("quick_look {}", path.display()));
        Ok(())
    }
}

struct Volumes2 {
    inner: Arc<dyn Platform>,
    fixed: Vec<Volume>,
    eject_error: Option<String>,
    log: CallLog,
}

impl VolumeLister for Volumes2 {
    fn list(&self) -> Result<Vec<Volume>> {
        if self.fixed.is_empty() {
            self.inner.volumes().list()
        } else {
            Ok(self.fixed.clone())
        }
    }
    fn wait_for_change(&self, timeout: Duration) -> bool {
        std::thread::sleep(timeout.min(Duration::from_millis(200)));
        false
    }
    fn can_eject(&self, v: &Volume) -> bool {
        v.kind == crate::platform::VolumeKind::Removable
    }
    fn eject(&self, v: &Volume) -> Result<()> {
        self.log
            .lock()
            .unwrap()
            .push(format!("eject {}", v.mount_point.display()));
        match &self.eject_error {
            None => Ok(()),
            Some(text) => Err(crate::Error::Invalid(text.clone())),
        }
    }
}

impl MacLike {
    pub fn new(inner: Arc<dyn Platform>, spec: MacLikeSpec) -> MacLike {
        let log: CallLog = Arc::default();
        MacLike {
            attrs: Attrs2 {
                inner: inner.clone(),
                suffixes: spec.package_suffixes.clone(),
                folder_tables: spec.folder_tables.clone(),
            },
            opener: Opener2 {
                inner: inner.clone(),
                log: log.clone(),
                quick_look: spec.quick_look,
            },
            volumes: Volumes2 {
                inner: inner.clone(),
                fixed: spec.volumes.clone(),
                eject_error: spec.eject_error.clone(),
                log: log.clone(),
            },
            inner,
            spec,
            log,
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl Platform for MacLike {
    fn name(&self) -> &'static str {
        "macos-like"
    }
    fn dirs(&self) -> &Dirs {
        self.inner.dirs()
    }
    fn trash(&self) -> &dyn TrashBackend {
        self.inner.trash()
    }
    fn volumes(&self) -> &dyn VolumeLister {
        &self.volumes
    }
    fn attributes(&self) -> &dyn FileAttributes {
        &self.attrs
    }
    fn opener(&self) -> &dyn Opener {
        &self.opener
    }
    fn path_rules(&self) -> PathRules {
        self.inner.path_rules()
    }
    fn fidelity(&self) -> &dyn Fidelity {
        self.inner.fidelity()
    }
    fn user_dirs(&self) -> UserDirs {
        self.inner.user_dirs()
    }
    fn display_names(&self, places: &[PathBuf]) -> Vec<(PathBuf, String)> {
        let mut out = self.spec.names.clone();
        if let Some(tables) = &self.spec.folder_tables {
            let home = &self.inner.dirs().home;
            let names = crate::platform::foldernames::FolderNames::current_with(tables);
            for p in places {
                if let Some(n) = names.shown(p, home) {
                    out.push((p.clone(), n));
                }
            }
        }
        out
    }
    fn extra_places(&self) -> Vec<(PlaceKind, PathBuf)> {
        self.spec.extra_places.clone()
    }
    fn recent_available(&self) -> bool {
        self.spec.recents.is_some()
    }
    fn recent_files(
        &self,
        _cancel: &dyn Fn() -> bool,
    ) -> Option<Vec<(PathBuf, std::time::SystemTime)>> {
        self.spec.recents.clone()
    }
    fn explain_denied(&self, path: &Path, err: &std::io::Error) -> Option<AccessDenied> {
        if err.raw_os_error() != Some(1) {
            return None;
        }
        self.spec
            .denied
            .iter()
            .find(|d| path.starts_with(&d.folder))
            .cloned()
    }
}

/// The sandbox's platform: the machine's own, except that nothing is ever opened. What the
/// program asks the system to open (a file, a folder in the file manager, a program with a
/// file, a terminal, an address) is written to the log, and nothing is started: a test must
/// not open a window on the desktop of whoever runs it.
pub struct Recording {
    inner: Arc<dyn Platform>,
    opener: Recorder,
}

struct Recorder {
    log: CallLog,
    /// What can open a file is the platform's own to say; launching it is not done.
    inner: Arc<dyn Platform>,
}

impl Recorder {
    fn note(&self, what: String) -> Result<()> {
        self.log.lock().unwrap().push(what);
        Ok(())
    }
}

impl Opener for Recorder {
    fn open(&self, path: &Path) -> Result<()> {
        self.note(format!("open {}", path.display()))
    }
    fn reveal(&self, path: &Path) -> Result<()> {
        self.note(format!("reveal {}", path.display()))
    }
    fn open_with(&self, path: &Path, program: &str) -> Result<()> {
        self.note(format!("open_with {program} {}", path.display()))
    }
    fn kind_of(&self, path: &Path) -> Option<String> {
        self.inner.opener().kind_of(path)
    }
    fn apps_for(&self, path: &Path) -> crate::platform::OpenChoices {
        // Only the Linux list is read from folders the tests own; the other systems' lists are
        // asked of the system itself (Launch Services, the shell), which a test leaves alone.
        if self.inner.name() == "linux" {
            self.inner.opener().apps_for(path)
        } else {
            crate::platform::OpenChoices::default()
        }
    }
    fn open_with_app(&self, path: &Path, app: &crate::platform::OpenApp) -> Result<()> {
        self.note(format!("open_with_app {} {}", app.id, path.display()))
    }
    fn open_terminal(&self, dir: &Path) -> Result<()> {
        self.note(format!("open_terminal {}", dir.display()))
    }
    fn open_url(&self, url: &str) -> Result<()> {
        self.note(format!("open_url {url}"))
    }
}

impl Recording {
    pub fn new(inner: Arc<dyn Platform>, log: CallLog) -> Recording {
        Recording {
            opener: Recorder {
                log,
                inner: inner.clone(),
            },
            inner,
        }
    }
}

impl Platform for Recording {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn dirs(&self) -> &Dirs {
        self.inner.dirs()
    }
    fn trash(&self) -> &dyn TrashBackend {
        self.inner.trash()
    }
    fn volumes(&self) -> &dyn VolumeLister {
        self.inner.volumes()
    }
    fn attributes(&self) -> &dyn FileAttributes {
        self.inner.attributes()
    }
    fn opener(&self) -> &dyn Opener {
        &self.opener
    }
    fn path_rules(&self) -> PathRules {
        self.inner.path_rules()
    }
    fn fidelity(&self) -> &dyn Fidelity {
        self.inner.fidelity()
    }
    fn user_dirs(&self) -> UserDirs {
        self.inner.user_dirs()
    }
    fn display_names(&self, places: &[PathBuf]) -> Vec<(PathBuf, String)> {
        self.inner.display_names(places)
    }
    fn extra_places(&self) -> Vec<(PlaceKind, PathBuf)> {
        self.inner.extra_places()
    }
    fn tags(&self) -> Option<&dyn TagStore> {
        self.inner.tags()
    }
    fn recent_available(&self) -> bool {
        self.inner.recent_available()
    }
    fn recent_files(
        &self,
        cancel: &dyn Fn() -> bool,
    ) -> Option<Vec<(PathBuf, std::time::SystemTime)>> {
        self.inner.recent_files(cancel)
    }
    fn explain_denied(&self, path: &Path, err: &std::io::Error) -> Option<AccessDenied> {
        self.inner.explain_denied(path, err)
    }
}

/// The platform of the machine running the tests, but with other rules for names: to plan as
/// if the destination were a volume where `Foo` and `foo` are the same file (macOS, Windows).
pub struct WithPathRules {
    inner: Arc<dyn Platform>,
    rules: PathRules,
}

impl WithPathRules {
    pub fn new(inner: Arc<dyn Platform>, rules: PathRules) -> Self {
        Self { inner, rules }
    }

    /// `inner`, with names that ignore case.
    pub fn case_insensitive(inner: Arc<dyn Platform>) -> Arc<dyn Platform> {
        let rules = PathRules {
            case_insensitive: true,
            ..inner.path_rules()
        };
        Arc::new(Self::new(inner, rules))
    }
}

impl Platform for WithPathRules {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn dirs(&self) -> &Dirs {
        self.inner.dirs()
    }
    fn trash(&self) -> &dyn TrashBackend {
        self.inner.trash()
    }
    fn volumes(&self) -> &dyn VolumeLister {
        self.inner.volumes()
    }
    fn attributes(&self) -> &dyn FileAttributes {
        self.inner.attributes()
    }
    fn opener(&self) -> &dyn Opener {
        self.inner.opener()
    }
    fn path_rules(&self) -> PathRules {
        self.rules.clone()
    }
    fn fidelity(&self) -> &dyn Fidelity {
        self.inner.fidelity()
    }
    fn user_dirs(&self) -> UserDirs {
        self.inner.user_dirs()
    }
    fn display_names(&self, places: &[PathBuf]) -> Vec<(PathBuf, String)> {
        self.inner.display_names(places)
    }
    fn extra_places(&self) -> Vec<(PlaceKind, PathBuf)> {
        self.inner.extra_places()
    }
    fn tags(&self) -> Option<&dyn TagStore> {
        self.inner.tags()
    }
    fn recent_available(&self) -> bool {
        self.inner.recent_available()
    }
    fn recent_files(
        &self,
        cancel: &dyn Fn() -> bool,
    ) -> Option<Vec<(PathBuf, std::time::SystemTime)>> {
        self.inner.recent_files(cancel)
    }
    fn explain_denied(&self, path: &Path, err: &std::io::Error) -> Option<AccessDenied> {
        self.inner.explain_denied(path, err)
    }
}
