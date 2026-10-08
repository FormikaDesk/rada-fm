//! Application state and behaviour. Pure logic: no drawing, no I/O. Everything that
//! touches the disk is a request to a worker; everything the workers say arrives as an
//! event and changes state here.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vela_core::events::{
    CoreEvent, DirEvent, FailureInfo, JobEvent, JobId, PreviewEvent, WatchEvent,
};
use vela_core::jobs::JobHandle;
use vela_core::journal::JournalEntry;
use vela_core::model::{DirListing, Entry, SortKey, SortSpec};
use vela_core::ops::{
    Cancel, ConflictPolicy, ErrorChoice, ExecReport, Pattern, Plan, Progress, Scan, TransferMode,
    TransferOptions, UndoPlan,
};
use vela_core::ops::{OpKind, Totals};
use vela_core::platform::Volume;
use vela_core::preview::{ImageInfo, ImageState, Limits, Preview};

use crate::fmt;
use crate::hits::{Hits, Target};
use crate::icons::IconSet;
use crate::images::{ImageMode, ImageUi, ResizeResult};
use crate::keymap::{Action, Keymap};
use crate::palette::{PaletteItem, PaletteKind, PaletteView};
use crate::services::Services;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Ok,
    Warn,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    until: Instant,
}

pub enum LoadState {
    Ready,
    /// Navigating: the old listing stays visible until the new one is in.
    Loading(PathBuf),
}

#[derive(Clone)]
pub struct Clip {
    pub mode: TransferMode,
    pub paths: Vec<PathBuf>,
}

pub struct PreviewState {
    pub name: String,
    pub content: Option<Preview>,
    pub scroll: usize,
    /// For image files: what to show besides the picture itself.
    pub image: Option<ImageView>,
    /// Binary files show a card; the hex dump only on request (`H`).
    pub hex: bool,
    path: Option<PathBuf>,
}

pub struct ImageView {
    pub info: ImageInfo,
    pub status: ImageStatus,
    pub note: Option<String>,
}

pub enum ImageStatus {
    /// Header read, pixels being decoded in the worker.
    Decoding,
    /// The picture is in the graphics protocol and drawn by the renderer.
    Shown,
    /// Image rendering is off (`--images off`): information only.
    NoGraphics,
    TooLarge(String),
    Failed(String),
}

pub struct Running {
    pub job: JobId,
    pub title: String,
    pub progress: Progress,
    pub started: Instant,
    pub handle: JobHandle,
    /// (time, bytes) samples for a smoothed transfer rate.
    samples: Vec<(Instant, u64)>,
    /// What it is doing, for the message when it ends.
    pub kind: OpKind,
    pub totals: Totals,
    pub reversible: bool,
}

impl Running {
    pub fn rate(&self) -> f64 {
        match (self.samples.first(), self.samples.last()) {
            (Some((t0, b0)), Some((t1, b1))) if t1 > t0 => {
                (*b1 as f64 - *b0 as f64) / t1.duration_since(*t0).as_secs_f64()
            }
            _ => 0.0,
        }
    }

    pub fn eta(&self) -> Option<Duration> {
        let rate = self.rate();
        let left = self
            .progress
            .bytes_total
            .saturating_sub(self.progress.bytes_done);
        (rate > 1.0 && left > 0).then(|| Duration::from_secs_f64(left as f64 / rate))
    }
}

#[derive(Clone)]
pub enum Replan {
    Transfer { dest: PathBuf, mode: TransferMode },
    None,
}

pub struct PlanView {
    pub plan: Plan,
    pub scan: Option<Arc<Scan>>,
    pub undo: Option<Box<UndoPlan>>,
    pub replan: Replan,
    pub scroll: usize,
    /// Typed confirmation for irreversible plans.
    pub typed: String,
    /// A new plan (other conflict policy) is being computed.
    pub replanning: Option<JobId>,
    /// Show every step instead of the per-item summary.
    pub details: bool,
}

impl PlanView {
    /// Permanent deletion is the only operation that asks for a typed confirmation
    /// (undoing something is not "irreversible" in that sense).
    pub fn needs_typed_confirmation(&self) -> bool {
        self.plan.kind == vela_core::ops::OpKind::Delete
    }

    pub fn can_run(&self) -> bool {
        self.plan.is_executable()
            && self.replanning.is_none()
            && (!self.needs_typed_confirmation() || self.typed.trim() == "yes")
    }
}

pub enum InputKind {
    Rename { from: PathBuf },
    NewDir,
    BulkRename { items: Vec<PathBuf> },
}

pub struct InputView {
    pub kind: InputKind,
    pub text: String,
    /// Cursor position in chars.
    pub cursor: usize,
    pub error: Option<String>,
}

impl InputView {
    fn new(kind: InputKind, text: String) -> Self {
        let cursor = text.chars().count();
        InputView {
            kind,
            text,
            cursor,
            error: None,
        }
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    fn insert(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
        self.error = None;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            let at = self.byte_at(self.cursor - 1);
            self.text.remove(at);
            self.cursor -= 1;
            self.error = None;
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.text.chars().count() {
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
            self.error = None;
        }
    }
}

pub struct HistoryView {
    pub entries: Vec<JournalEntry>,
    pub selected: usize,
    pub loading: bool,
}

pub struct ResultView {
    pub title: String,
    pub lines: Vec<(ToastKind, String)>,
    pub scroll: usize,
}

pub enum Modal {
    Scanning {
        job: JobId,
        what: String,
        files: u64,
        dirs: u64,
        bytes: u64,
        current: PathBuf,
        cancel: Cancel,
    },
    Plan(Box<PlanView>),
    Failure {
        job: JobId,
        info: FailureInfo,
        reply: Sender<ErrorChoice>,
    },
    Input(InputView),
    History(HistoryView),
    Palette(Box<PaletteView>),
    Result(ResultView),
    ConfirmQuit,
    Help,
    Menu(MenuView),
}

pub struct Config {
    pub start_dir: PathBuf,
    pub icons: IconSet,
    pub show_hidden: bool,
    pub sort: SortSpec,
    pub theme: Theme,
    pub image_mode: ImageMode,
    pub limits: Limits,
    /// Put the cursor on this entry of `start_dir` (when started with a file path).
    pub select: Option<OsString>,
    /// Bookmarks from the configuration file.
    pub bookmarks: Vec<PathBuf>,
    pub demo: Option<Demo>,
    pub keymap: Keymap,
    /// Mouse capture on/off (`mouse = false` in the configuration).
    pub mouse: bool,
}

/// Developer hook: put the interface in a ready-made state (for screenshots).
pub struct Demo {
    pub scene: String,
    pub dest: Option<PathBuf>,
}

mod input;

pub use input::{FilterState, MenuItem, MenuView};

pub struct App {
    pub th: Theme,
    pub icons: IconSet,
    svc: Services,

    pub cwd: PathBuf,
    pub listing: DirListing,
    pub load: LoadState,
    dir_gen: u64,
    pub visible: Vec<usize>,
    pub cursor: usize,
    pub scroll: usize,
    pub marked: BTreeSet<OsString>,
    pub show_hidden: bool,
    pub sort: SortSpec,
    remembered: HashMap<PathBuf, OsString>,

    pub preview: PreviewState,
    pub image_ui: Option<ImageUi>,
    limits: Limits,
    preview_gen: u64,
    previewed: Option<PathBuf>,

    pub volumes: Vec<Volume>,
    /// Recent folders, state bookmarks and standard places (from the places worker).
    pub paths: vela_core::places::PathLists,
    cfg_bookmarks: Vec<PathBuf>,
    demo: Option<Demo>,
    demo_auto_run: bool,
    pub clipboard: Option<Clip>,
    pub modal: Option<Modal>,
    pub running: Option<Running>,
    pub toast: Option<Toast>,
    pub watch_note: Option<String>,
    /// Number of rows the list can show; set by the renderer.
    pub view_rows: usize,
    pub should_quit: bool,
    pub dirty: bool,
    pub spinner: usize,
    plan_job: Option<JobId>,

    pub keymap: Keymap,
    pub mouse: bool,
    /// Screen rectangles of everything clickable, rebuilt every frame.
    pub hits: Hits,
    pub filter: Option<FilterState>,
    sel_anchor: Option<(usize, BTreeSet<OsString>)>,
    last_click: Option<(Instant, usize)>,
    pub help_scroll: usize,
}

impl App {
    pub fn new(cfg: Config, svc: Services, image_ui: Option<ImageUi>) -> App {
        let decode_images = image_ui.is_some();
        let mut app = App {
            th: cfg.theme,
            icons: cfg.icons,
            cwd: cfg.start_dir.clone(),
            listing: DirListing::new(cfg.start_dir.clone(), Vec::new(), cfg.sort),
            load: LoadState::Loading(cfg.start_dir.clone()),
            dir_gen: 0,
            visible: Vec::new(),
            cursor: 0,
            scroll: 0,
            marked: BTreeSet::new(),
            show_hidden: cfg.show_hidden,
            sort: cfg.sort,
            remembered: HashMap::new(),
            preview: PreviewState {
                name: String::new(),
                content: None,
                scroll: 0,
                image: None,
                hex: false,
                path: None,
            },
            image_ui,
            limits: {
                let mut l = cfg.limits;
                // No graphics, no point in decoding pictures.
                l.image.decode = l.image.decode && decode_images;
                l
            },
            preview_gen: 0,
            previewed: None,
            volumes: Vec::new(),
            paths: Default::default(),
            cfg_bookmarks: cfg.bookmarks,
            demo: cfg.demo,
            demo_auto_run: false,
            clipboard: None,
            modal: None,
            running: None,
            toast: None,
            watch_note: None,
            view_rows: 20,
            should_quit: false,
            dirty: true,
            spinner: 0,
            plan_job: None,
            keymap: cfg.keymap,
            mouse: cfg.mouse,
            hits: Hits::default(),
            filter: None,
            sel_anchor: None,
            last_click: None,
            help_scroll: 0,
            svc,
        };
        for w in std::mem::take(&mut app.keymap.warnings) {
            tracing::warn!("keymap: {w}");
        }
        if !app.svc.journal_ok {
            app.toast(
                ToastKind::Warn,
                "journal unavailable: operations cannot be undone this session",
                8,
            );
        }
        if let Some(name) = cfg.select {
            app.remembered.insert(cfg.start_dir.clone(), name);
        }
        app.svc.places.load();
        app.svc.places.visit(cfg.start_dir.clone());
        app.svc.watcher.watch(cfg.start_dir.clone());
        app.request_dir(cfg.start_dir);
        app
    }

    pub fn home(&self) -> &Path {
        &self.svc.home
    }

    pub fn events(&self) -> crossbeam_channel::Receiver<CoreEvent> {
        self.svc.events.clone()
    }

    // ------------------------------------------------------------------ small helpers

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>, secs: u64) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            until: Instant::now() + Duration::from_secs(secs),
        });
        self.dirty = true;
    }

    pub fn tick(&mut self) {
        self.spinner = self.spinner.wrapping_add(1);
        if let Some(t) = &self.toast {
            if Instant::now() >= t.until {
                self.toast = None;
                self.dirty = true;
            }
        }
        if self.running.is_some()
            || matches!(self.modal, Some(Modal::Scanning { .. }))
            || matches!(self.load, LoadState::Loading(_))
        {
            self.dirty = true; // spinner / elapsed time
        }
    }

    pub fn entry_at(&self, vis_index: usize) -> Option<&Entry> {
        self.visible
            .get(vis_index)
            .and_then(|&i| self.listing.all().get(i))
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entry_at(self.cursor)
    }

    fn rebuild_visible(&mut self) {
        let show = self.show_hidden;
        let terms: Vec<String> = self
            .filter
            .as_ref()
            .map(|f| f.text.split_whitespace().map(str::to_lowercase).collect())
            .unwrap_or_default();
        self.visible = self
            .listing
            .all()
            .iter()
            .enumerate()
            .filter(|(_, e)| show || !e.hidden)
            .filter(|(_, e)| {
                terms.is_empty() || {
                    let name = e.display.to_lowercase();
                    terms.iter().all(|t| name.contains(t))
                }
            })
            .map(|(i, _)| i)
            .collect();
    }

    /// Paths an operation applies to: the marked entries, or else the one under the cursor.
    pub fn targets(&self) -> Vec<PathBuf> {
        if !self.marked.is_empty() {
            return self
                .listing
                .all()
                .iter()
                .filter(|e| self.marked.contains(&e.name))
                .map(|e| e.path.clone())
                .collect();
        }
        self.current()
            .map(|e| vec![e.path.clone()])
            .unwrap_or_default()
    }

    pub fn is_busy(&self) -> bool {
        self.running.is_some()
    }

    fn ensure_visible(&mut self) {
        let rows = self.view_rows.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + rows {
            self.scroll = self.cursor + 1 - rows;
        }
        let max_scroll = self.visible.len().saturating_sub(rows);
        self.scroll = self.scroll.min(max_scroll);
    }

    fn set_cursor(&mut self, to: usize) {
        let max = self.visible.len().saturating_sub(1);
        let to = to.min(max);
        if to != self.cursor {
            self.cursor = to;
            self.preview.scroll = 0;
        }
        self.ensure_visible();
        self.request_preview();
        self.dirty = true;
    }

    fn cursor_name(&self) -> Option<OsString> {
        self.current().map(|e| e.name.clone())
    }

    fn restore_cursor(&mut self, name: Option<OsString>) {
        let idx = name.and_then(|n| {
            self.visible
                .iter()
                .position(|&i| self.listing.all()[i].name == n)
        });
        match idx {
            Some(i) => self.cursor = i,
            None => self.cursor = self.cursor.min(self.visible.len().saturating_sub(1)),
        }
        self.ensure_visible();
    }

    fn request_dir(&mut self, path: PathBuf) {
        if path != self.cwd {
            self.filter = None;
            self.sel_anchor = None;
        }
        self.dir_gen += 1;
        self.load = LoadState::Loading(path.clone());
        self.svc.loader.load(path, self.dir_gen);
    }

    fn request_preview(&mut self) {
        let Some(e) = self.current() else {
            self.preview.content = None;
            self.preview.name.clear();
            self.previewed = None;
            return;
        };
        let path = e.path.clone();
        if self.previewed.as_ref() == Some(&path) {
            return;
        }
        self.previewed = Some(path.clone());
        self.preview_gen += 1;
        self.svc
            .previewer
            .request(path, self.preview_gen, self.limits.clone());
    }

    // ------------------------------------------------------------------ navigation

    pub fn open_dir(&mut self, path: PathBuf) {
        if let Some(name) = self.cursor_name() {
            self.remembered.insert(self.cwd.clone(), name);
        }
        self.request_dir(path);
    }

    fn go_parent(&mut self) {
        let Some(parent) = self.cwd.parent().map(|p| p.to_path_buf()) else {
            self.toast(ToastKind::Info, "already at the top", 2);
            return;
        };
        if let Some(name) = self.cwd.file_name() {
            self.remembered.insert(parent.clone(), name.to_os_string());
        }
        self.request_dir(parent);
    }

    fn enter(&mut self) {
        let Some(e) = self.current() else { return };
        if e.is_dir() {
            let target = e.path.clone();
            self.open_dir(target);
        } else if e.error.is_none() {
            let path = e.path.clone();
            let platform = self.svc.platform.clone();
            // Spawning a process is I/O: keep it off the UI thread.
            std::thread::spawn(move || {
                if let Err(err) = platform.opener().open(&path) {
                    tracing::warn!("open {}: {err}", path.display());
                }
            });
            self.toast(
                ToastKind::Info,
                format!(
                    "opening {}",
                    self.current()
                        .map(|e| e.display.clone())
                        .unwrap_or_default()
                ),
                2,
            );
        }
    }

    // ------------------------------------------------------------------ core events

    pub fn on_core_event(&mut self, ev: CoreEvent) {
        self.dirty = true;
        match ev {
            CoreEvent::Dir(d) => self.on_dir(d),
            CoreEvent::Watch(w) => self.on_watch(w),
            CoreEvent::Preview(p) => self.on_preview(p),
            CoreEvent::Volumes(v) => {
                self.volumes = v;
                self.refresh_palette();
            }
            CoreEvent::Paths(p) => {
                self.paths = p;
                self.refresh_palette();
            }
            CoreEvent::Job(j) => self.on_job(j),
        }
    }

    fn on_dir(&mut self, ev: DirEvent) {
        match ev {
            DirEvent::Loaded {
                generation,
                path,
                result,
            } => {
                if generation != self.dir_gen {
                    return; // an answer to a request that has been superseded
                }
                match result {
                    Ok(entries) => {
                        let same_dir = path == self.cwd;
                        let keep = if same_dir {
                            // Before the first listing there is no cursor yet: use the
                            // remembered entry (the file given on the command line).
                            self.cursor_name()
                                .or_else(|| self.remembered.get(&path).cloned())
                        } else {
                            self.remembered.get(&path).cloned()
                        };
                        self.listing = DirListing::new(path.clone(), entries, self.sort);
                        if !same_dir {
                            self.cwd = path.clone();
                            self.clear_marks();
                            self.cursor = 0;
                            self.scroll = 0;
                            self.svc.places.visit(path.clone());
                            self.svc.watcher.watch(path);
                            self.previewed = None;
                        } else {
                            let existing: BTreeSet<OsString> =
                                self.listing.all().iter().map(|e| e.name.clone()).collect();
                            self.marked.retain(|n| existing.contains(n));
                        }
                        self.rebuild_visible();
                        self.restore_cursor(keep);
                        self.load = LoadState::Ready;
                        self.request_preview();
                        self.run_demo();
                    }
                    Err(msg) => {
                        self.load = LoadState::Ready;
                        self.toast(ToastKind::Error, msg, 6);
                    }
                }
            }
            DirEvent::Patched { dir, updates, .. } => {
                if dir != self.cwd {
                    return;
                }
                let keep = self.cursor_name();
                self.listing.apply(updates);
                let existing: BTreeSet<OsString> =
                    self.listing.all().iter().map(|e| e.name.clone()).collect();
                self.marked.retain(|n| existing.contains(n));
                self.rebuild_visible();
                self.restore_cursor(keep);
                self.previewed = None; // contents may have changed
                self.request_preview();
            }
        }
    }

    fn on_watch(&mut self, ev: WatchEvent) {
        match ev {
            WatchEvent::Changed { dir, paths, rescan } => {
                if dir != self.cwd {
                    return;
                }
                self.dir_gen += 1;
                if rescan {
                    self.svc.loader.load(dir, self.dir_gen);
                } else {
                    self.svc.loader.patch(dir, paths, self.dir_gen);
                }
            }
            WatchEvent::Degraded(msg) => self.watch_note = Some(msg),
            WatchEvent::Failed(msg) => {
                self.watch_note = Some(msg.clone());
                self.toast(ToastKind::Warn, format!("live updates off: {msg}"), 6);
            }
        }
    }

    fn on_preview(&mut self, ev: PreviewEvent) {
        if ev.generation != self.preview_gen {
            return;
        }
        if self.preview.path.as_ref() != Some(&ev.path) {
            self.preview.hex = false;
            self.preview.scroll = 0;
            self.preview.path = Some(ev.path.clone());
        }
        self.preview.name = vela_core::display::name(&ev.name);
        match ev.preview {
            Preview::Image(ip) => {
                let status = match ip.state {
                    ImageState::Disabled => {
                        self.clear_image();
                        ImageStatus::NoGraphics
                    }
                    ImageState::Loading => {
                        if let Some(ui) = &mut self.image_ui {
                            ui.clear();
                        }
                        ImageStatus::Decoding
                    }
                    ImageState::Ready(px) => match &mut self.image_ui {
                        Some(ui) => {
                            // The event owns the only reference: no pixel copy.
                            let img = std::sync::Arc::try_unwrap(px)
                                .unwrap_or_else(|shared| (*shared).clone());
                            ui.show(img);
                            ImageStatus::Shown
                        }
                        None => ImageStatus::NoGraphics,
                    },
                    ImageState::TooLarge(m) => {
                        self.clear_image();
                        ImageStatus::TooLarge(m)
                    }
                    ImageState::Failed(m) => {
                        self.clear_image();
                        ImageStatus::Failed(m)
                    }
                };
                self.preview.image = Some(ImageView {
                    info: ip.info,
                    status,
                    note: ip.note,
                });
                self.preview.content = None;
            }
            other => {
                self.clear_image();
                self.preview.image = None;
                self.preview.content = Some(other);
            }
        }
    }

    fn clear_image(&mut self) {
        if let Some(ui) = &mut self.image_ui {
            ui.clear();
        }
    }

    /// The encoder thread finished resizing/encoding the picture for the current area.
    pub fn on_image_resized(&mut self, r: ResizeResult) {
        if let Some(ui) = &mut self.image_ui {
            ui.on_resized(r);
        }
        self.dirty = true;
    }

    fn on_job(&mut self, ev: JobEvent) {
        match ev {
            JobEvent::Scanning {
                job,
                files,
                dirs,
                bytes,
                current,
            } => {
                if let Some(Modal::Scanning {
                    job: j,
                    files: f,
                    dirs: d,
                    bytes: b,
                    current: c,
                    ..
                }) = &mut self.modal
                {
                    if *j == job {
                        (*f, *d, *b, *c) = (files, dirs, bytes, current);
                    }
                }
            }
            JobEvent::Planned {
                job,
                plan,
                scan,
                undo,
            } => self.on_planned(job, *plan, scan, undo),
            JobEvent::PlanFailed { job, message } | JobEvent::Error { job, message } => {
                if self.plan_job == Some(job) || self.running.as_ref().is_some_and(|r| r.job == job)
                {
                    self.modal = None;
                    self.plan_job = None;
                    self.running = None;
                    self.toast(ToastKind::Error, message, 6);
                }
            }
            JobEvent::NothingToUndo { job } => {
                if self.plan_job == Some(job) {
                    self.modal = None;
                    self.plan_job = None;
                    self.toast(ToastKind::Info, "nothing to undo", 3);
                }
            }
            JobEvent::NothingToRedo { job } => {
                if self.plan_job == Some(job) {
                    self.modal = None;
                    self.plan_job = None;
                    self.toast(
                        ToastKind::Info,
                        "nothing to redo (redo only follows an undo, until another operation)",
                        4,
                    );
                }
            }
            JobEvent::Progress { job, progress } => {
                if let Some(r) = self.running.as_mut().filter(|r| r.job == job) {
                    let now = Instant::now();
                    r.samples.push((now, progress.bytes_done));
                    r.samples
                        .retain(|(t, _)| now.duration_since(*t) < Duration::from_secs(4));
                    r.progress = progress;
                }
            }
            JobEvent::AskFailure { job, info, reply } => {
                if self.running.as_ref().is_some_and(|r| r.job == job) {
                    self.modal = Some(Modal::Failure { job, info, reply });
                } else {
                    let _ = reply.send(ErrorChoice::Abort);
                }
            }
            JobEvent::Finished {
                job,
                title,
                report,
                journal_errors,
                ..
            } => {
                if self.running.as_ref().is_some_and(|r| r.job == job) {
                    let done = self.running.take();
                    self.finish(title, report, journal_errors, done);
                    // Refresh even if live updates are unavailable.
                    self.dir_gen += 1;
                    self.svc.loader.load(self.cwd.clone(), self.dir_gen);
                }
            }
            JobEvent::History { entries, .. } => {
                if let Some(Modal::History(h)) = &mut self.modal {
                    h.entries = entries;
                    h.loading = false;
                    h.selected = 0;
                }
            }
        }
    }

    fn finish(
        &mut self,
        title: String,
        report: ExecReport,
        journal_errors: u64,
        done: Option<Running>,
    ) {
        use vela_core::ops::RunStatus::*;
        let mut lines: Vec<(ToastKind, String)> = Vec::new();
        for f in &report.failed {
            lines.push((ToastKind::Error, f.error.clone()));
        }
        if report.skipped_dependent > 0 {
            lines.push((
                ToastKind::Warn,
                format!(
                    "{} more skipped because their folder failed",
                    report.skipped_dependent
                ),
            ));
        }
        for (_, note) in &report.kept {
            lines.push((ToastKind::Warn, note.clone()));
        }
        if journal_errors > 0 {
            lines.push((ToastKind::Warn, format!("{journal_errors} steps could not be written to the journal and may not be undoable")));
        }
        let (kind, headline) = match report.status() {
            Completed if report.kept.is_empty() => (
                ToastKind::Ok,
                done.as_ref()
                    .map(|r| self.done_message(r))
                    .unwrap_or_else(|| format!("{title}: done")),
            ),
            Completed => (
                ToastKind::Warn,
                format!("{title}: done, some items were kept"),
            ),
            CompletedWithProblems => (
                ToastKind::Warn,
                format!(
                    "{title}: finished with {} problem(s)",
                    report.failed.len() + report.skipped_dependent as usize
                ),
            ),
            Aborted => (
                ToastKind::Warn,
                format!("{title}: stopped; press u to undo what was done"),
            ),
            Cancelled => (
                ToastKind::Warn,
                format!("{title}: cancelled; press u to undo what was done"),
            ),
        };
        if lines.is_empty() {
            self.toast(kind, headline, 4);
        } else {
            self.modal = Some(Modal::Result(ResultView {
                title: headline,
                lines,
                scroll: 0,
            }));
        }
    }

    fn on_planned(
        &mut self,
        job: JobId,
        plan: Plan,
        scan: Option<Arc<Scan>>,
        undo: Option<Box<UndoPlan>>,
    ) {
        // Re-plan of an open plan window?
        if let Some(Modal::Plan(pv)) = &mut self.modal {
            if pv.replanning == Some(job) {
                pv.plan = plan;
                pv.scan = scan.or(pv.scan.take());
                pv.replanning = None;
                pv.scroll = 0;
                return;
            }
        }
        if self.plan_job != Some(job) {
            return; // the user cancelled while it was being planned
        }
        self.plan_job = None;
        let replan = match (&plan.kind, &plan.destination) {
            (vela_core::ops::OpKind::Copy, Some(d)) => Replan::Transfer {
                dest: d.clone(),
                mode: TransferMode::Copy,
            },
            (vela_core::ops::OpKind::Move, Some(d)) => Replan::Transfer {
                dest: d.clone(),
                mode: TransferMode::Move,
            },
            _ => Replan::None,
        };
        if self.demo_auto_run {
            self.demo_auto_run = false;
            self.modal = None;
            let pv = Box::new(PlanView {
                plan,
                scan,
                undo,
                replan,
                scroll: 0,
                typed: String::new(),
                replanning: None,
                details: false,
            });
            self.run_plan(pv);
            return;
        }
        self.modal = Some(Modal::Plan(Box::new(PlanView {
            plan,
            scan,
            undo,
            replan,
            scroll: 0,
            typed: String::new(),
            replanning: None,
            details: false,
        })));
    }

    // ------------------------------------------------------------------ starting operations

    fn begin_plan(&mut self, what: &str, handle: JobHandle) {
        self.plan_job = Some(handle.id);
        self.modal = Some(Modal::Scanning {
            job: handle.id,
            what: what.to_string(),
            files: 0,
            dirs: 0,
            bytes: 0,
            current: PathBuf::new(),
            cancel: handle.cancel,
        });
    }

    fn refuse_if_busy(&mut self) -> bool {
        if self.is_busy() {
            self.toast(
                ToastKind::Warn,
                "an operation is running; wait or press Esc to cancel it",
                3,
            );
            true
        } else {
            false
        }
    }

    fn start_paste(&mut self) {
        if self.refuse_if_busy() {
            return;
        }
        let Some(clip) = self.clipboard.clone() else {
            self.toast(ToastKind::Info, "nothing copied yet (y = copy, x = cut)", 3);
            return;
        };
        let opts = TransferOptions {
            mode: clip.mode,
            policy: ConflictPolicy::Skip,
            verify: false,
        };
        let h = self
            .svc
            .jobs
            .plan_transfer(clip.paths, self.cwd.clone(), opts);
        self.begin_plan(
            if clip.mode == TransferMode::Copy {
                "Planning copy"
            } else {
                "Planning move"
            },
            h,
        );
    }

    fn start_trash(&mut self, permanent: bool) {
        if self.refuse_if_busy() {
            return;
        }
        let t = self.targets();
        if t.is_empty() {
            return;
        }
        let h = if permanent {
            self.svc.jobs.plan_delete(t)
        } else {
            self.svc.jobs.plan_trash(t)
        };
        self.begin_plan("Planning", h);
    }

    fn start_undo(&mut self, id: Option<String>) {
        if self.refuse_if_busy() {
            return;
        }
        let h = self.svc.jobs.plan_undo(id);
        self.begin_plan("Planning undo", h);
    }

    /// Ask for the same transfer again with another way of settling name clashes.
    fn replan_with(&mut self, pv: &mut PlanView, policy: ConflictPolicy) {
        if pv.needs_typed_confirmation() || pv.replanning.is_some() || pv.plan.policy == policy {
            return;
        }
        if let (Some(scan), Replan::Transfer { dest, mode }) = (pv.scan.clone(), pv.replan.clone())
        {
            let h = self.svc.jobs.replan_transfer(
                scan,
                dest,
                TransferOptions {
                    mode,
                    policy,
                    verify: false,
                },
            );
            pv.replanning = Some(h.id);
        }
    }

    fn run_plan(&mut self, pv: Box<PlanView>) {
        let title = pv.plan.title.clone();
        let total = pv.plan.total_bytes();
        let (kind, totals, reversible) = (pv.plan.kind, pv.plan.totals, pv.plan.reversible);
        let handle = match pv.undo {
            Some(up) => self.svc.jobs.execute_undo(*up),
            None => self.svc.jobs.execute(pv.plan),
        };
        self.running = Some(Running {
            job: handle.id,
            title,
            progress: Progress {
                bytes_total: total,
                ..Default::default()
            },
            started: Instant::now(),
            handle,
            samples: Vec::new(),
            kind,
            totals,
            reversible,
        });
        self.clear_marks();
    }

    fn submit_input(&mut self, iv: InputView) {
        match &iv.kind {
            InputKind::Rename { from } => {
                let h = self
                    .svc
                    .jobs
                    .plan_rename(from.clone(), iv.text.trim_end_matches('\n').to_string());
                self.begin_plan("Planning rename", h);
            }
            InputKind::NewDir => {
                let h = self.svc.jobs.plan_mkdir(self.cwd.clone(), iv.text.clone());
                self.begin_plan("Planning", h);
            }
            InputKind::BulkRename { items } => match Pattern::parse(&iv.text) {
                Ok(_) => {
                    let h = self
                        .svc
                        .jobs
                        .plan_bulk_rename(items.clone(), iv.text.clone());
                    self.begin_plan("Planning rename", h);
                }
                Err(e) => {
                    let mut iv = iv;
                    iv.error = Some(e);
                    self.modal = Some(Modal::Input(iv));
                }
            },
        }
    }

    // ------------------------------------------------------------------ keys

    fn quit(&mut self) {
        if self.is_busy() {
            self.modal = Some(Modal::ConfirmQuit);
        } else {
            self.should_quit = true;
        }
    }

    fn clear_marks(&mut self) {
        self.marked.clear();
        self.sel_anchor = None;
    }

    fn toggle_mark(&mut self) {
        if let Some(e) = self.current() {
            let name = e.name.clone();
            if !self.marked.remove(&name) {
                self.marked.insert(name);
            }
        }
        self.set_cursor(self.cursor + 1);
    }

    fn mark_all(&mut self) {
        let all: Vec<OsString> = self
            .visible
            .iter()
            .map(|&i| self.listing.all()[i].name.clone())
            .collect();
        if self.marked.len() == all.len() {
            self.clear_marks();
        } else {
            self.marked = all.into_iter().collect();
        }
    }

    fn yank(&mut self, mode: TransferMode) {
        let paths = self.targets();
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        self.clipboard = Some(Clip { mode, paths });
        self.clear_marks();
        let verb = if mode == TransferMode::Copy {
            "copied"
        } else {
            "cut"
        };
        self.toast(
            ToastKind::Info,
            format!("{n} item(s) {verb}; go to the destination and press p"),
            3,
        );
    }

    fn set_sort(&mut self, spec: SortSpec) {
        let keep = self.cursor_name();
        self.sort = spec;
        self.listing.set_sort(spec); // immediate, in memory
        self.rebuild_visible();
        self.restore_cursor(keep);
        let arrow = if spec.reverse { "↓" } else { "↑" };
        self.toast(
            ToastKind::Info,
            format!("sorted by {} {arrow}", spec.key.label()),
            2,
        );
    }

    fn start_rename(&mut self) {
        if self.refuse_if_busy() {
            return;
        }
        if let Some(e) = self.current() {
            let text = e.name.to_string_lossy().into_owned();
            let from = e.path.clone();
            self.modal = Some(Modal::Input(InputView::new(
                InputKind::Rename { from },
                text,
            )));
        }
    }

    fn start_bulk_rename(&mut self) {
        if self.refuse_if_busy() {
            return;
        }
        let items = self.targets();
        if items.is_empty() {
            return;
        }
        self.modal = Some(Modal::Input(InputView::new(
            InputKind::BulkRename { items },
            "{name}_{n:2}.{ext}".to_string(),
        )));
    }

    fn palette_items(&self) -> Vec<PaletteItem> {
        let mut items: Vec<PaletteItem> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let home = self.svc.home.clone();
        let cwd = self.cwd.clone();
        // A mount point is listed once, as a disk (so "/" is not both "Root" and a disk).
        let mounts: std::collections::HashSet<PathBuf> =
            self.volumes.iter().map(|v| v.mount_point.clone()).collect();
        let mut add = |items: &mut Vec<PaletteItem>,
                       path: &Path,
                       kind: PaletteKind,
                       label: Option<String>,
                       detail: Option<String>| {
            if kind != PaletteKind::Disk && mounts.contains(path) {
                return;
            }
            if !seen.insert(path.to_path_buf()) {
                return;
            }
            let label = label.unwrap_or_else(|| {
                if path == home {
                    "Home".to_string()
                } else {
                    path.file_name()
                        .map(vela_core::display::name)
                        .unwrap_or_else(|| vela_core::display::path(path))
                }
            });
            let detail = detail.unwrap_or_else(|| fmt::short_path(path, &home));
            items.push(PaletteItem {
                label,
                detail,
                path: path.to_path_buf(),
                kind,
            });
        };
        let bookmarks: Vec<PathBuf> = self
            .cfg_bookmarks
            .iter()
            .chain(self.paths.bookmarks.iter())
            .cloned()
            .collect();
        for p in &bookmarks {
            add(&mut items, p, PaletteKind::Bookmark, None, None);
        }
        for p in self.paths.recents.iter().filter(|p| **p != cwd) {
            add(&mut items, p, PaletteKind::Recent, None, None);
        }
        for p in &self.paths.places {
            add(&mut items, p, PaletteKind::Place, None, None);
        }
        add(
            &mut items,
            Path::new(std::path::MAIN_SEPARATOR_STR),
            PaletteKind::Place,
            Some("Root".into()),
            Some(std::path::MAIN_SEPARATOR_STR.into()),
        );
        for v in &self.volumes {
            let label = v.label.clone().unwrap_or_else(|| {
                if v.mount_point == Path::new(std::path::MAIN_SEPARATOR_STR) {
                    "Root".to_string()
                } else {
                    vela_core::display::path(&v.mount_point)
                }
            });
            let free = v
                .available
                .map(|a| format!(" · {} free", fmt::size(a)))
                .unwrap_or_else(|| {
                    if v.responsive {
                        String::new()
                    } else {
                        " · not responding".into()
                    }
                });
            let detail = format!("{}{free}", vela_core::display::path(&v.mount_point));
            add(
                &mut items,
                &v.mount_point,
                PaletteKind::Disk,
                Some(label),
                Some(detail),
            );
        }
        for anc in cwd.ancestors().skip(1).take(4) {
            add(&mut items, anc, PaletteKind::Parent, None, None);
        }
        let dirs: Vec<PathBuf> = self
            .listing
            .all()
            .iter()
            .filter(|e| e.is_dir())
            .take(400)
            .map(|e| e.path.clone())
            .collect();
        for p in &dirs {
            add(&mut items, p, PaletteKind::Folder, None, None);
        }
        items
    }

    fn open_palette(&mut self) {
        let items = self.palette_items();
        self.modal = Some(Modal::Palette(Box::new(PaletteView::new(
            items,
            self.svc.home.clone(),
        ))));
    }

    /// New recents/volumes arrived while the palette is open: refresh its list, keep the query.
    fn refresh_palette(&mut self) {
        if matches!(self.modal, Some(Modal::Palette(_))) {
            let items = self.palette_items();
            if let Some(Modal::Palette(p)) = &mut self.modal {
                p.set_items(items);
            }
        }
    }

    /// Developer hook (`--demo`): put the interface in a ready-made state, once.
    fn run_demo(&mut self) {
        let Some(d) = self.demo.take() else { return };
        let (scene, arg) = d
            .scene
            .split_once(':')
            .map_or((d.scene.as_str(), None), |(a, b)| (a, Some(b)));
        match scene {
            "palette" => {
                self.open_palette();
                if let (Some(Modal::Palette(p)), Some(q)) = (&mut self.modal, arg) {
                    for c in q.chars() {
                        p.insert(c);
                    }
                }
            }
            "plan" | "progress" => {
                let paths: Vec<PathBuf> = self
                    .visible
                    .iter()
                    .filter_map(|&i| self.listing.all().get(i))
                    .map(|e| e.path.clone())
                    .collect();
                self.marked = self
                    .visible
                    .iter()
                    .filter_map(|&i| self.listing.all().get(i))
                    .map(|e| e.name.clone())
                    .collect();
                let Some(dest) = d.dest else { return };
                self.demo_auto_run = scene == "progress";
                let opts = TransferOptions::copy(ConflictPolicy::Skip);
                let h = self.svc.jobs.plan_transfer(paths, dest, opts);
                self.begin_plan("Planning copy", h);
            }
            "history" => {
                self.modal = Some(Modal::History(HistoryView {
                    entries: Vec::new(),
                    selected: 0,
                    loading: true,
                }));
                self.svc.jobs.load_history();
            }
            "help" => self.modal = Some(Modal::Help),
            _ => {}
        }
    }

    fn toggle_bookmark(&mut self) {
        let cwd = self.cwd.clone();
        let was = self.paths.bookmarks.contains(&cwd);
        self.svc.places.toggle_bookmark(cwd);
        self.toast(
            ToastKind::Ok,
            if was {
                "bookmark removed"
            } else {
                "folder bookmarked: Ctrl+P to jump back"
            },
            3,
        );
    }

    // ------------------------------------------------------------------ modal keys

    fn on_modal_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(modal) = self.modal.take() else {
            return;
        };
        match modal {
            Modal::Scanning {
                job,
                what,
                files,
                dirs,
                bytes,
                current,
                cancel,
            } => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    cancel.cancel();
                    self.plan_job = None;
                } else {
                    self.modal = Some(Modal::Scanning {
                        job,
                        what,
                        files,
                        dirs,
                        bytes,
                        current,
                        cancel,
                    });
                }
            }
            Modal::Plan(mut pv) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Char('q') if !pv.needs_typed_confirmation() => {}
                KeyCode::Enter => {
                    if pv.can_run() {
                        self.run_plan(pv);
                    } else {
                        self.modal = Some(Modal::Plan(pv));
                    }
                }
                KeyCode::Char('c')
                    if !pv.needs_typed_confirmation()
                        && matches!(pv.replan, Replan::Transfer { .. }) =>
                {
                    let policy = pv.plan.policy.next();
                    self.replan_with(&mut pv, policy);
                    self.modal = Some(Modal::Plan(pv));
                }
                KeyCode::Tab => {
                    pv.details = !pv.details;
                    pv.scroll = 0;
                    self.modal = Some(Modal::Plan(pv));
                }
                KeyCode::Down | KeyCode::Char('j') if !pv.needs_typed_confirmation() => {
                    pv.scroll += 1;
                    self.modal = Some(Modal::Plan(pv));
                }
                KeyCode::Up | KeyCode::Char('k') if !pv.needs_typed_confirmation() => {
                    pv.scroll = pv.scroll.saturating_sub(1);
                    self.modal = Some(Modal::Plan(pv));
                }
                KeyCode::PageDown => {
                    pv.scroll += 10;
                    self.modal = Some(Modal::Plan(pv));
                }
                KeyCode::PageUp => {
                    pv.scroll = pv.scroll.saturating_sub(10);
                    self.modal = Some(Modal::Plan(pv));
                }
                KeyCode::Backspace if pv.needs_typed_confirmation() => {
                    pv.typed.pop();
                    self.modal = Some(Modal::Plan(pv));
                }
                KeyCode::Char(c) if pv.needs_typed_confirmation() && !ctrl => {
                    pv.typed.push(c);
                    self.modal = Some(Modal::Plan(pv));
                }
                _ => self.modal = Some(Modal::Plan(pv)),
            },
            Modal::Failure { job, info, reply } => {
                let choice = match key.code {
                    KeyCode::Char('s') | KeyCode::Enter => Some(ErrorChoice::Skip),
                    KeyCode::Char('S') => Some(ErrorChoice::SkipAll),
                    KeyCode::Char('r') => Some(ErrorChoice::Retry),
                    KeyCode::Char('a') | KeyCode::Esc | KeyCode::Char('q') => {
                        Some(ErrorChoice::Abort)
                    }
                    _ => None,
                };
                match choice {
                    Some(c) => {
                        let _ = reply.send(c);
                    }
                    None => self.modal = Some(Modal::Failure { job, info, reply }),
                }
            }
            Modal::Input(mut iv) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    if iv.text.is_empty() {
                        self.modal = Some(Modal::Input(iv));
                    } else {
                        self.submit_input(iv);
                    }
                }
                KeyCode::Char(c) if !ctrl => {
                    iv.insert(c);
                    self.modal = Some(Modal::Input(iv));
                }
                KeyCode::Backspace => {
                    iv.backspace();
                    self.modal = Some(Modal::Input(iv));
                }
                KeyCode::Delete => {
                    iv.delete();
                    self.modal = Some(Modal::Input(iv));
                }
                KeyCode::Left => {
                    iv.cursor = iv.cursor.saturating_sub(1);
                    self.modal = Some(Modal::Input(iv));
                }
                KeyCode::Right => {
                    iv.cursor = (iv.cursor + 1).min(iv.text.chars().count());
                    self.modal = Some(Modal::Input(iv));
                }
                KeyCode::Home => {
                    iv.cursor = 0;
                    self.modal = Some(Modal::Input(iv));
                }
                KeyCode::End => {
                    iv.cursor = iv.text.chars().count();
                    self.modal = Some(Modal::Input(iv));
                }
                KeyCode::Char('u') if ctrl => {
                    iv.text.clear();
                    iv.cursor = 0;
                    self.modal = Some(Modal::Input(iv));
                }
                _ => self.modal = Some(Modal::Input(iv)),
            },
            Modal::History(mut h) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('U') => {}
                KeyCode::Down | KeyCode::Char('j') => {
                    h.selected = (h.selected + 1).min(h.entries.len().saturating_sub(1));
                    self.modal = Some(Modal::History(h));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    h.selected = h.selected.saturating_sub(1);
                    self.modal = Some(Modal::History(h));
                }
                KeyCode::Enter | KeyCode::Char('u') => {
                    if let Some(e) = h.entries.get(h.selected).cloned() {
                        if e.is_undoable() || e.can_retry_undo() {
                            self.start_undo(Some(e.id));
                        } else {
                            self.toast(ToastKind::Info, "this operation cannot be undone (permanent, already undone, or nothing to undo)", 4);
                            self.modal = Some(Modal::History(h));
                        }
                    }
                }
                _ => self.modal = Some(Modal::History(h)),
            },
            Modal::Palette(mut p) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    if let Some(item) = p.current() {
                        let path = item.path.clone();
                        self.open_dir(path);
                    }
                }
                KeyCode::Down => {
                    p.move_by(1);
                    self.modal = Some(Modal::Palette(p));
                }
                KeyCode::Up => {
                    p.move_by(-1);
                    self.modal = Some(Modal::Palette(p));
                }
                KeyCode::Char('n') | KeyCode::Char('j') if ctrl => {
                    p.move_by(1);
                    self.modal = Some(Modal::Palette(p));
                }
                KeyCode::Char('p') | KeyCode::Char('k') if ctrl => {
                    p.move_by(-1);
                    self.modal = Some(Modal::Palette(p));
                }
                KeyCode::PageDown => {
                    p.move_by(8);
                    self.modal = Some(Modal::Palette(p));
                }
                KeyCode::PageUp => {
                    p.move_by(-8);
                    self.modal = Some(Modal::Palette(p));
                }
                KeyCode::Char('u') if ctrl => {
                    p.clear();
                    self.modal = Some(Modal::Palette(p));
                }
                KeyCode::Backspace => {
                    p.backspace();
                    self.modal = Some(Modal::Palette(p));
                }
                KeyCode::Char(c) if !ctrl => {
                    p.insert(c);
                    self.modal = Some(Modal::Palette(p));
                }
                _ => self.modal = Some(Modal::Palette(p)),
            },
            Modal::Result(mut r) => match key.code {
                KeyCode::Char('u') => self.start_undo(None),
                KeyCode::Char('z') if ctrl => self.start_undo(None),
                KeyCode::Down | KeyCode::Char('j') => {
                    r.scroll += 1;
                    self.modal = Some(Modal::Result(r));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    r.scroll = r.scroll.saturating_sub(1);
                    self.modal = Some(Modal::Result(r));
                }
                _ => {}
            },
            Modal::ConfirmQuit => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    if let Some(r) = &self.running {
                        r.handle.cancel.cancel();
                    }
                    self.should_quit = true;
                }
                _ => {}
            },
            Modal::Help => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    self.help_scroll += 1;
                    self.modal = Some(Modal::Help);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                    self.modal = Some(Modal::Help);
                }
                KeyCode::PageDown => {
                    self.help_scroll += 10;
                    self.modal = Some(Modal::Help);
                }
                KeyCode::PageUp => {
                    self.help_scroll = self.help_scroll.saturating_sub(10);
                    self.modal = Some(Modal::Help);
                }
                KeyCode::Home => {
                    self.help_scroll = 0;
                    self.modal = Some(Modal::Help);
                }
                // Closing is deliberate: Esc, q, ? , F1, Enter or Space.
                KeyCode::Esc | KeyCode::Char('q' | '?' | ' ') | KeyCode::F(1) | KeyCode::Enter => {}
                _ => self.modal = Some(Modal::Help),
            },
            Modal::Menu(m) => self.on_menu_key(m, key),
        }
    }

    // ------------------------------------------------------------------ for the renderer

    pub fn sort_label(&self) -> String {
        let arrow = if self.sort.reverse { "↓" } else { "↑" };
        format!("{} {arrow}", self.sort.key.label())
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.load, LoadState::Loading(_))
    }

    pub fn sort_key(&self) -> SortKey {
        self.sort.key
    }
}
