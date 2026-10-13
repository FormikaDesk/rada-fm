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
}

impl FileAttributes for Attrs2 {
    fn attrs(&self, name: &OsStr, meta: &FsMeta) -> Attrs {
        let mut a = self.inner.attributes().attrs(name, meta);
        // The Finder hides dot files, whatever the machine the stand-in runs on does.
        a.hidden |= name.to_string_lossy().starts_with('.');
        a
    }
    fn shown_name(&self, name: &OsStr, meta: &FsMeta) -> Option<std::ffi::OsString> {
        // The Finder's rule, as the real Mac attributes have it.
        crate::platform::macos::localized_folder_name(name, meta.is_dir())
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
    fn display_names(&self, _places: &[PathBuf]) -> Vec<(PathBuf, String)> {
        self.spec.names.clone()
    }
    fn extra_places(&self) -> Vec<(PlaceKind, PathBuf)> {
        self.spec.extra_places.clone()
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
            inner,
            opener: Recorder { log },
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
    fn explain_denied(&self, path: &Path, err: &std::io::Error) -> Option<AccessDenied> {
        self.inner.explain_denied(path, err)
    }
}
