//! Application state and behaviour. Pure logic: no drawing, no I/O. Everything that
//! touches the disk is a request to a worker; everything the workers say arrives as an
//! event and changes state here.

use rada_core::i18n::{Key, tf};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::archive::ArchiveKind;
use rada_core::events::{CoreEvent, DirEvent, FailureInfo, JobEvent, JobId, WatchEvent};
use rada_core::jobs::JobHandle;
use rada_core::journal::JournalEntry;
use rada_core::model::{DirListing, SortKey, SortSpec};
use rada_core::ops::{
    Cancel, ConflictPolicy, ErrorChoice, ExecReport, Pattern, Plan, Progress, Scan, TransferMode,
    TransferOptions, UndoPlan,
};
use rada_core::ops::{ExtractInto, OpKind, OpRequest, WarningKind};
use rada_core::platform::Volume;
use rada_core::preview::Limits;

use crate::fmt::{self, DateFormat, DateStyle};
use crate::icons::IconSet;
use crate::images::{ImageMode, ImageUi};
use crate::keymap::{Action, Keymap};
use crate::nav::NavMove;
use crate::palette::{PaletteItem, PaletteKind, PaletteView};
use crate::services::Services;
use crate::theme::Theme;
use crate::view::{LayoutKind, ViewMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Ok,
    Warn,
    Error,
}

#[derive(Default)]
pub enum LoadState {
    #[default]
    Ready,
    /// Navigating: the old listing stays visible until the new one is in.
    Loading(PathBuf),
}

/// How much of the folder the area it was last drawn in can show: set by the renderer, read
/// by the keyboard (a page is as many rows as fit). It is part of a tab's own state and says
/// nothing about where on the screen the tab is, so a tab can be drawn in any area — one day
/// beside another tab in a split view.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    /// Rows of the table.
    pub rows: usize,
    /// Tiles across and down in the Icons view.
    pub grid_cols: usize,
    pub grid_rows: usize,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            rows: 20,
            grid_cols: 4,
            grid_rows: 3,
        }
    }
}

#[derive(Clone)]
pub enum Replan {
    Transfer {
        dest: PathBuf,
        mode: TransferMode,
    },
    /// Asked again from the request the plan answers (an extraction, a new archive).
    Request,
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
    /// The keyboard is on the Cancel button (Tab moves it between the two buttons).
    pub cancel_focus: bool,
}

/// Whether what was typed is the word that confirms a dangerous operation: "yes", or "sì" /
/// "si" (written the way the Italian interface asks for it).
pub fn is_confirmation(typed: &str) -> bool {
    matches!(
        typed.trim().to_lowercase().as_str(),
        "yes" | "si" | "s\u{ec}"
    )
}

impl PlanView {
    /// Permanent deletion, and the extraction of what may be an archive bomb, ask for a typed
    /// confirmation (undoing something is not "irreversible" in that sense).
    pub fn needs_typed_confirmation(&self) -> bool {
        self.plan.kind == rada_core::ops::OpKind::Delete
            || self
                .plan
                .warnings
                .iter()
                .any(|w| w.kind == WarningKind::ArchiveBomb)
    }

    pub fn can_run(&self) -> bool {
        self.plan.is_executable()
            && self.replanning.is_none()
            && (!self.needs_typed_confirmation() || is_confirmation(&self.typed))
    }
}

pub enum InputKind {
    Rename {
        from: PathBuf,
    },
    NewDir,
    /// The name of the file to save, when rada is the system's file chooser.
    SaveAs,
    BulkRename {
        items: Vec<PathBuf>,
    },
    /// The folder to extract an archive into, named after the archive unless changed.
    ExtractTo {
        archive: PathBuf,
        dest: PathBuf,
        default: String,
    },
    /// The name (and so the format) of a new archive.
    Compress {
        sources: Vec<PathBuf>,
        dir: PathBuf,
        format: ArchiveKind,
    },
    /// The program to open a file with.
    OpenWith {
        path: PathBuf,
    },
    /// The name of a new empty file.
    NewFile,
    /// The name of a tag to give to the selected items.
    NewTag {
        items: Vec<PathBuf>,
    },
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
    /// The preview of the item under the cursor, over the whole screen.
    FullPreview,
    Properties(PropsView),
    /// A newer version: its notes and the question, or why an update did not work.
    Update(UpdateView),
    /// The system has not allowed the terminal to open a folder yet.
    Access(rada_core::platform::AccessDenied),
    /// Ejecting a disk.
    Eject(EjectView),
    /// The offer to install a Nerd Font.
    Font(FontView),
    /// "Open with…": the programs that can open the file, to choose from.
    OpenWith(Box<OpenWithView>),
}

/// The offer to install a Nerd Font, made once at the first start where the icons would not
/// draw. Never made automatically: the person says yes.
#[derive(Clone)]
pub struct FontOffer {
    /// The font's name, as the terminal's font setting calls it.
    pub family: String,
    /// The size of the download, in megabytes.
    pub megabytes: f32,
    /// Download and install it (blocks: run on a thread of its own). The sentence to show
    /// when done, or why not.
    pub install: Arc<dyn Fn() -> Result<String, String> + Send + Sync>,
    /// Called when the offer is first shown, so that it is not made again.
    pub shown: Arc<dyn Fn() + Send + Sync>,
}

pub struct Config {
    pub start_dir: PathBuf,
    pub icons: IconSet,
    pub show_hidden: bool,
    pub sort: SortSpec,
    pub theme: Theme,
    /// Set when the theme is to follow the terminal's background: `theme` is then only the
    /// stand-in until the terminal has been looked at.
    pub adaptive: Option<Adaptive>,
    pub image_mode: ImageMode,
    pub limits: Limits,
    /// Put the cursor on this entry of `start_dir` (when started with a file path).
    pub select: Option<OsString>,
    /// Bookmarks from the configuration file.
    pub bookmarks: Vec<PathBuf>,
    pub demo: Option<Demo>,
    pub keymap: Keymap,
    /// Offer to install a Nerd Font at the start.
    pub font_offer: Option<FontOffer>,
    /// On a Mac, ask the terminal to report the Cmd key (`cmd_keys` in the configuration).
    pub cmd_keys: bool,
    /// Mouse capture on/off (`mouse = false` in the configuration).
    pub mouse: bool,
    /// Key hints in the bottom bar (`hints = false` hides them).
    pub show_hints: bool,
    /// Whether the sidebar is wanted at all (the configuration's value, or what was
    /// last chosen with its key).
    pub sidebar: bool,
    /// An operation handed in as data (`--request`): planned at start and shown in the
    /// usual confirmation window.
    pub request: Option<rada_core::ops::OpRequest>,
    /// `explorer` (default) or `compact`.
    pub layout: LayoutKind,
    /// Panels, the classic layout, or whichever fits the window.
    pub layout_pref: crate::panels::LayoutPref,
    /// The details pane: `None` shows it when the terminal is wide enough.
    pub details_pane: Option<bool>,
    /// How folders are shown in a new tab.
    pub view: ViewMode,
    /// Remember the open tabs between sessions.
    pub remember_tabs: bool,
    /// What was remembered from the last session (tabs, panes); written back as it changes.
    pub saved_ui: rada_core::uistate::UiState,
    /// `start_dir` was asked for (a path on the command line): it opens in a tab of its own
    /// beside the restored ones.
    pub start_explicit: bool,
    pub dates: DateStyle,
    pub date_format: DateFormat,
    /// How much room the sidebar leaves between its entries.
    pub sidebar_density: crate::sidebar::Density,
    /// What the NAV box of the panels view lists under the places.
    pub nav_mode: crate::panels::NavMode,
    /// The basic profile (ASCII, 16 colours): no rounded ends, no shades, a plain look.
    pub basic: bool,
    /// The Tag column of the list: `None` follows the remembered choice, then shows it.
    pub tags_column: Option<bool>,
    /// Set when rada was started to choose files for another program (`--chooser-file`).
    pub chooser: Option<crate::chooser::Chooser>,
}

/// A theme that follows the terminal's background: which one, and how to show it. Resolved
/// when the terminal is started, because finding out may need to ask the terminal.
pub struct Adaptive {
    pub name: String,
    pub depth: crate::theme::ColorDepth,
    /// `transparent_background`: the default dark theme leaves the terminal's background.
    pub transparent: bool,
}

/// Disks that do not exist, for screenshots that must not show the machine's own.
fn demo_volumes() -> Vec<Volume> {
    use rada_core::platform::VolumeKind;
    let gb = 1_000_000_000u64;
    let vol = |mount: &str, label: &str, kind, total: u64, free: u64| Volume {
        mount_point: PathBuf::from(mount),
        label: Some(label.into()),
        fs_type: "ext4".into(),
        device: "demo".into(),
        kind,
        drive_letter: None,
        total: Some(total * gb),
        available: Some(free * gb),
        read_only: false,
        responsive: true,
    };
    vec![
        vol("/", Key::DiskSystem.tr(), VolumeKind::Fixed, 512, 212),
        vol(
            "/run/media/user/Data",
            Key::DiskData.tr(),
            VolumeKind::Fixed,
            1000,
            640,
        ),
        vol(
            "/run/media/user/USB",
            Key::DiskUsb.tr(),
            VolumeKind::Removable,
            64,
            41,
        ),
    ]
}

/// Developer hook: put the interface in a ready-made state (for screenshots).
pub struct Demo {
    pub scene: String,
    pub dest: Option<PathBuf>,
}

/// When the screen is drawn again, and the step of the spinner that moves between frames.
#[derive(Default)]
pub struct Redraw {
    /// Something changed: the next pass of the event loop draws a frame.
    pub dirty: bool,
    /// The whole screen is drawn again from nothing at the next frame (Ctrl+L, and after a
    /// program was started): whatever something else wrote on the terminal is put right.
    pub full_redraw: bool,
    /// When to do that again after a program was started: they say things a while after.
    redraw_at: Vec<Instant>,
    pub spinner: usize,
}

impl Redraw {
    /// The first frame is yet to be drawn.
    fn new() -> Redraw {
        Redraw {
            dirty: true,
            ..Default::default()
        }
    }

    /// A program was started from here. Some of them write on the terminal they were started
    /// from (Electron editors print their logs), which spoils the screen: it is drawn again from
    /// nothing now and a few times in the seconds after.
    pub fn after_launch(&mut self) {
        let now = Instant::now();
        self.full_redraw = true;
        self.dirty = true;
        self.redraw_at.extend(
            [300u64, 1500, 4000]
                .into_iter()
                .map(|ms| now + Duration::from_millis(ms)),
        );
    }

    /// Whether the screen is to be cleared and drawn again; asking resets it.
    pub fn take_full_redraw(&mut self) -> bool {
        std::mem::take(&mut self.full_redraw)
    }
}

/// What only tests and screenshots use: a ready-made scene, made-up disks, a clock that can
/// be set and moved on.
#[derive(Default)]
pub struct TestHooks {
    demo: Option<Demo>,
    /// Show made-up disks instead of the machine's own.
    demo_volumes: bool,
    demo_auto_run: bool,
    /// Replaces the system clock when drawing relative dates.
    pub clock: Option<std::time::SystemTime>,
    /// Added to the clock the notifications read: nobody waits six seconds for a card to go.
    notice_skew: Duration,
}

impl TestHooks {
    fn new(demo: Option<Demo>) -> TestHooks {
        TestHooks {
            demo_volumes: demo.is_some(),
            demo,
            ..TestHooks::default()
        }
    }
}

mod address;
mod archives;
mod chooser;
mod columns;
mod eject;
mod font;
mod input;
mod listing;
mod menus;
mod mount;
mod notice;
mod openwith;
mod ops;
mod panels;
mod preview;
mod props;
mod selection;
mod settings;
mod side;
mod sizes;
pub use sizes::FolderSizeView;
mod tabs;
mod update;
mod windows;

pub use address::{AddressEdit, BarHit, BarItem, BarMode};
pub use eject::{EjectStage, EjectView};
pub use font::{FontStage, FontView};
pub use input::{FilterState, MenuCmd, MenuIcon, MenuItem, MenuTitle, MenuView, TagCmd};
pub use menus::MenuKind;
pub use notice::{MAX_SHOWN, Notice, NoticeAction, NoticeKind, NoticeQueue};
pub use openwith::{OpenRow, OpenWithView};
pub use ops::{Clip, Operations, Running};
pub use panels::GitView;
pub use preview::{ImageStatus, ImageView, PreviewPane};
pub use props::PropsView;
pub use tabs::TabState;
pub use update::{UpdateStatus, UpdateView, Updates};

/// The language of the system's tag names (where there are tags).
fn svc_tag_language(svc: &Services) -> String {
    svc.platform
        .tags()
        .map(|t| t.language())
        .unwrap_or_else(|| "en".to_string())
}

pub struct App {
    /// What comes from `config.toml` and the View menu.
    pub settings: settings::Settings,
    svc: Services,
    /// What only tests and screenshots use.
    pub hooks: TestHooks,

    /// The front tab: its folder, cursor, selection, order and view. The others are in
    /// `tabs.slots`.
    pub tab: TabState,

    /// The preview of the item under the cursor, its picture, and the keyboard focus in it.
    pub preview: preview::PreviewPane,

    /// Disks, sticks and phones, and the folders the system holds back.
    pub devices: mount::Devices,
    /// The places the sidebar lists.
    pub places: side::Places,
    /// The tags the sidebar and the tag menu know.
    pub tags: side::Tags,
    /// What waits on the clipboard, the plan being prepared, the operation running and the undo.
    pub ops: ops::Operations,
    /// The open window, the notification cards, the address bar and the other things put over
    /// the explorer.
    pub windows: windows::Windows,
    /// The tabs behind the front one, which one is in front, and what is remembered between
    /// sessions.
    pub tabs: tabs::Tabs,
    pub columns: columns::ColumnsView,
    /// How the session ends, and whether it is a file chooser.
    pub session: chooser::Session,
    pub redraw: Redraw,

    /// The sidebar and the NAV box: the keyboard in them, their cursors and the tree.
    pub side_nav: side::SideNav,
    /// What the last frame drew, for the keyboard, the mouse and the tests.
    pub frame: crate::ui::LastFrame,
    last_click: Option<(Instant, usize)>,

    // ---- the explorer interface
    /// What is worked out about the folder shown.
    pub folder_info: panels::FolderInfo,
    pub updates: Updates,
}

impl App {
    pub fn new(mut cfg: Config, svc: Services, image_ui: Option<ImageUi>) -> App {
        // A chooser starts with the newest first and the list in front, whatever the settings say.
        if cfg.chooser.is_some() {
            cfg.sort = crate::chooser::default_sort(cfg.sort);
            cfg.details_pane = None;
        }
        let settings = settings::Settings::from_config(&cfg);
        let (tab, tabs) = tabs::Tabs::start(&mut cfg);
        let start_dir = tab.cwd.clone();
        let mut app = App {
            settings,
            tab,
            preview: preview::PreviewPane::new(image_ui, cfg.limits.clone()),
            devices: Default::default(),
            places: side::Places::new(std::mem::take(&mut cfg.bookmarks)),
            tags: side::Tags::new(svc_tag_language(&svc)),
            ops: Default::default(),
            windows: Default::default(),
            tabs,
            columns: columns::ColumnsView::default(),
            session: chooser::Session::new(cfg.chooser.clone()),
            redraw: Redraw::new(),
            side_nav: side::SideNav::new(cfg.sidebar),
            frame: Default::default(),
            last_click: None,
            folder_info: Default::default(),
            updates: Updates::new(&svc),
            hooks: TestHooks::new(cfg.demo.take()),
            svc,
        };
        app.start(start_dir, cfg);
        app
    }

    /// What the first moments of a session do: tell what went wrong before it began, ask for
    /// the folder, look for updates, and begin the operation or the offer that was asked for.
    fn start(&mut self, start_dir: PathBuf, cfg: Config) {
        for w in std::mem::take(&mut self.settings.keymap.warnings) {
            tracing::warn!("keymap: {w}");
        }
        if !self.svc.journal_ok {
            self.toast(ToastKind::Warn, Key::TJournalDown.tr(), 8);
        }
        if let Some(name) = cfg.select {
            self.tab.remembered.insert(start_dir.clone(), name);
        }
        self.svc.places.load();
        self.svc.places.visit(start_dir.clone());
        self.svc.watcher.watch(start_dir.clone());
        self.request_dir(start_dir);
        self.svc.jobs.load_undo_label();
        // The daily check: the worker decides whether it is due, off this thread.
        if let Some(u) = &self.svc.updater {
            u.check_in_background();
        }
        if let Some(req) = cfg.request {
            let h = self.svc.jobs.plan_request(req);
            self.begin_plan(Key::PlanningGeneric.tr(), h);
        }
        self.maybe_offer_font(cfg.font_offer);
    }

    /// "Now" for relative dates.
    pub fn now(&self) -> std::time::SystemTime {
        self.hooks.clock.unwrap_or_else(std::time::SystemTime::now)
    }

    pub fn home(&self) -> &Path {
        &self.svc.home
    }

    pub fn events(&self) -> crossbeam_channel::Receiver<CoreEvent> {
        self.svc.events.clone()
    }

    pub fn tick(&mut self) {
        self.redraw.spinner = self.redraw.spinner.wrapping_add(1);
        if !self.redraw.redraw_at.is_empty() {
            let now = Instant::now();
            if self.redraw.redraw_at.iter().any(|t| *t <= now) {
                self.redraw.redraw_at.retain(|t| *t > now);
                self.redraw.full_redraw = true;
                self.redraw.dirty = true;
            }
        }
        self.tick_notices();
        if self.ops.is_busy()
            || self.windows.notices.has_clock()
            || matches!(self.windows.modal, Some(Modal::Scanning { .. }))
            || matches!(self.tab.load, LoadState::Loading(_))
        {
            self.redraw.dirty = true; // spinner / elapsed time
        }
    }

    fn request_dir(&mut self, path: PathBuf) {
        if path != self.tab.cwd {
            self.tab.filter = None;
            self.tab.sel.drop_anchor();
        }
        let generation = self.tabs.next_generation();
        self.tab.load = LoadState::Loading(path.clone());
        self.svc.loader.load(path, generation);
    }

    // ------------------------------------------------------------------ navigation

    /// Change how zoxide is called (the program starts with the configuration's choice; the
    /// tests give it a made-up one).
    pub fn use_zoxide(&mut self, zoxide: rada_core::zoxide::Zoxide) {
        self.svc.zoxide = zoxide;
    }

    /// Change how git is called (the program starts with the configuration's choice; the tests
    /// give it a repository of their own to look at and a git kept from the person's settings).
    pub fn use_git(&mut self, git: rada_core::git::Git) {
        self.svc.git = git;
        self.folder_info.forget_git();
    }

    pub fn open_dir(&mut self, path: PathBuf) {
        // A device that is plugged in and not mounted is not a folder yet: opening it mounts it.
        if let Some(id) = self.devices.unmounted_id(&path) {
            self.start_mount(&id);
            return;
        }
        if let Some(name) = self.tab.cursor_name() {
            self.tab.remembered.insert(self.tab.cwd.clone(), name);
        }
        self.request_dir(path);
    }

    /// The order of the list of recent files: the last used first, whatever the order of the
    /// folders is. It lasts as long as the list is open.
    const RECENT_SORT: SortSpec = SortSpec {
        key: SortKey::Modified,
        reverse: false,
        dirs_first: false,
        natural: false,
    };

    /// Say why nothing can be made here when the list is a tag's items, not a folder.
    pub(super) fn refuse_in_tag_view(&mut self, what: &str) -> bool {
        if self.tab.recents_view() {
            self.toast(ToastKind::Warn, tf(Key::TRecentRefuse, &[&what]), 5);
            return true;
        }
        let Some(tag) = self.tab.tag_view() else {
            return false;
        };
        self.toast(ToastKind::Warn, tf(Key::TTagViewRefuse, &[&tag, &what]), 5);
        true
    }

    /// From a list of tagged or recent items: open the folder the item under the cursor is in,
    /// with the cursor on it.
    pub(super) fn show_in_folder(&mut self) {
        if self.tab.tag_view().is_none() {
            self.toast(ToastKind::Info, Key::TShowInFolderHere.tr(), 2);
            return;
        }
        let Some(path) = self.tab.current().map(|e| e.path.clone()) else {
            return;
        };
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        self.tab
            .remembered
            .insert(dir.to_path_buf(), name.to_os_string());
        self.open_dir(dir.to_path_buf());
    }

    /// The folder to use for something made next to the items: the one the first of them is
    /// in when the list is a tag's items, the folder shown otherwise.
    pub(super) fn folder_for_new(&self, items: &[PathBuf]) -> PathBuf {
        if self.tab.tag_view().is_some() {
            if let Some(dir) = items.first().and_then(|p| p.parent()) {
                return dir.to_path_buf();
            }
            return self.svc.home.clone();
        }
        self.tab.cwd.clone()
    }

    /// Back to the previous folder of the history (or forward to the next one).
    fn go_history(&mut self, back: bool) {
        let target = if back {
            self.tab.nav.back_target()
        } else {
            self.tab.nav.forward_target()
        };
        let Some(target) = target.map(Path::to_path_buf) else {
            return;
        };
        self.tab.nav_move = Some(if back {
            NavMove::Back(target.clone())
        } else {
            NavMove::Forward(target.clone())
        });
        self.open_dir(target);
    }

    fn go_parent(&mut self) {
        // A tag's items have no parent: leave the way one came.
        if self.tab.tag_view().is_some() {
            if self.tab.nav.back_target().is_some() {
                self.go_history(true);
            } else {
                self.request_dir(self.svc.home.clone());
            }
            return;
        }
        let Some(parent) = self.tab.cwd.parent().map(|p| p.to_path_buf()) else {
            self.toast(ToastKind::Info, Key::TAtTop.tr(), 2);
            return;
        };
        if let Some(name) = self.tab.cwd.file_name() {
            self.tab
                .remembered
                .insert(parent.clone(), name.to_os_string());
        }
        self.request_dir(parent);
    }

    fn enter(&mut self) {
        if self.session.chooser.is_some() {
            return self.chooser_enter(false, false);
        }
        let Some(e) = self.tab.current() else { return };
        if e.is_navigable_dir() {
            let target = e.path.clone();
            self.open_dir(target);
        } else if self.tab.archive.is_some() {
            self.toast(
                ToastKind::Info,
                tf(
                    Key::TInArchive,
                    &[
                        &self
                            .key_for(Action::Copy)
                            .unwrap_or_else(|| Key::KeyCopyWord.tr().into()),
                        &self
                            .key_for(Action::ExtractHere)
                            .unwrap_or_else(|| Key::KeyExtractWord.tr().into()),
                    ],
                ),
                4,
            );
        } else if self.archive_candidate(e) {
            let target = e.path.clone();
            self.open_dir(target);
        } else if e.error.is_none() {
            let path = e.path.clone();
            let platform = self.svc.platform.clone();
            let notes = self.svc.notes.clone();
            let state = platform.dirs().rada_state();
            // Spawning a process is I/O: keep it off the UI thread.
            self.redraw.after_launch();
            std::thread::spawn(move || {
                // A program chosen in "Open with…" to be used always for this kind of file
                // takes the place of the system's default.
                if openwith::open_with_chosen(platform.as_ref(), &state, &path) {
                    return;
                }
                if let Err(err) = platform.opener().open(&path) {
                    tracing::warn!("open {}: {err}", path.display());
                    let _ = notes.send(CoreEvent::Note {
                        text: tf(Key::TCannotOpen, &[&path.display(), &err]),
                        error: true,
                    });
                }
            });
            self.toast(
                ToastKind::Info,
                tf(
                    Key::TOpening,
                    &[&self
                        .tab
                        .current()
                        .map(|e| e.display.clone())
                        .unwrap_or_default()],
                ),
                2,
            );
        }
    }

    /// Whether this system has a Quick Look to offer.
    pub fn quick_look_available(&self) -> bool {
        self.svc.platform.opener().can_quick_look()
    }

    /// Go inside a package (`.app`, `.photoslibrary`…) instead of opening it.
    pub(super) fn show_package(&mut self) {
        match self.tab.current() {
            Some(e) if e.package => {
                let path = e.path.clone();
                self.open_dir(path);
            }
            _ => self.toast(ToastKind::Info, Key::TNotPackage.tr(), 2),
        }
    }

    /// The system's quick look at the item under the cursor (macOS).
    pub(super) fn quick_look(&mut self) {
        let Some(path) = self.tab.current().map(|e| e.path.clone()) else {
            return;
        };
        if !self.svc.platform.opener().can_quick_look() {
            self.toast(ToastKind::Info, Key::TQuickLookMac.tr(), 3);
            return;
        }
        if self.refuse_in_archive(Key::WhatQuickLook.tr()) {
            return;
        }
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        self.redraw.after_launch();
        std::thread::spawn(move || {
            if let Err(e) = platform.opener().quick_look(&path) {
                let _ = notes.send(CoreEvent::Note {
                    text: tf(Key::TQuickLookErr, &[&e]),
                    error: true,
                });
            }
        });
    }

    // ------------------------------------------------------------------ core events

    pub fn on_core_event(&mut self, ev: CoreEvent) {
        self.redraw.dirty = true;
        match ev {
            CoreEvent::Dir(d) => self.on_dir(d),
            CoreEvent::Watch(w) => self.on_watch(w),
            CoreEvent::Preview(p) => self.on_preview(p),
            CoreEvent::Volumes(v) => {
                self.devices.volumes_seen = true;
                self.devices.volumes = if self.hooks.demo_volumes {
                    demo_volumes()
                } else {
                    v
                };
                self.folder_info
                    .sizes
                    .set_volumes(&self.devices.volumes, self.svc.platform.device_root());
                self.refresh_palette();
            }
            CoreEvent::Unmounted(list) => {
                self.devices.unmounted = if self.hooks.demo_volumes {
                    Vec::new()
                } else {
                    list
                };
            }
            CoreEvent::Mounted { id, name, result } => self.on_mounted(id, name, result),
            CoreEvent::Paths(p) => {
                self.places.paths = p;
                self.refresh_palette();
            }
            CoreEvent::Job(j) => self.on_job(j),
            CoreEvent::Complete { dir, names } => self.on_complete(dir, names),
            CoreEvent::Git { dir, status } => self.on_git(dir, status),
            CoreEvent::Tree { dir, hidden, kids } => {
                self.side_nav.tree.answer(dir, hidden, kids);
                self.redraw.dirty = true;
            }
            CoreEvent::Zoxide { query, dirs } => self.on_zoxide(query, dirs),
            CoreEvent::Size(s) => self.on_size_event(s),
            CoreEvent::Update(u) => self.on_update_event(u),
            CoreEvent::Ejected {
                mount,
                name,
                result,
            } => self.on_ejected(mount, name, result),
            CoreEvent::FontInstalled(r) => self.on_font_installed(r),
            CoreEvent::OpenChoices {
                path,
                kind,
                apps,
                always,
            } => self.on_open_choices(path, kind, apps, always),
            CoreEvent::Note { text, error } => {
                self.toast(
                    if error {
                        ToastKind::Error
                    } else {
                        ToastKind::Info
                    },
                    text,
                    5,
                );
            }
        }
    }

    fn on_dir(&mut self, ev: DirEvent) {
        match ev {
            DirEvent::Loaded {
                generation,
                path,
                result,
                archive,
                denied,
            } => {
                if generation & columns::PARENT_GEN != 0 {
                    return self.on_parent_loaded(generation, path, result);
                }
                if generation != self.tabs.generation() {
                    return; // an answer to a request that has been superseded
                }
                match result {
                    Ok(entries) => {
                        self.devices.allow(&path);
                        self.tags.note(&entries);
                        let same_dir = path == self.tab.cwd;
                        let keep = if same_dir {
                            // Before the first listing there is no cursor yet: use the
                            // remembered entry (the file given on the command line).
                            self.tab
                                .cursor_name()
                                .or_else(|| self.tab.remembered.get(&path).cloned())
                        } else {
                            self.tab.remembered.get(&path).cloned()
                        };
                        // The list of recent files has an order of its own while it is open;
                        // the folders get theirs back when it is left.
                        let into_recents = rada_core::recents::is_view(&path);
                        if into_recents != self.tab.recents_view() {
                            if into_recents {
                                self.tabs.sort_outside_recents = Some(self.tab.sort);
                                self.tab.sort = Self::RECENT_SORT;
                            } else if let Some(before) = self.tabs.sort_outside_recents.take() {
                                self.tab.sort = before;
                            }
                        }
                        self.tab.listing = DirListing::new(path.clone(), entries, self.tab.sort);
                        let virtual_dir =
                            archive.is_some() || rada_core::tagsearch::view_name(&path).is_some();
                        self.tab.archive = archive;
                        if !same_dir {
                            match self.tab.nav_move.take() {
                                Some(NavMove::Back(to)) if to == path => {
                                    self.tab.nav.stepped_back()
                                }
                                Some(NavMove::Forward(to)) if to == path => {
                                    self.tab.nav.stepped_forward()
                                }
                                _ => self.tab.nav.visit(path.clone()),
                            }
                            self.tab.cwd = path.clone();
                            self.tab.sel.clear_marks();
                            self.tab.sel.cursor = 0;
                            self.tab.sel.scroll = 0;
                            // An archive's folders are not on the disk: nothing to watch, and
                            // not a place to come back to.
                            if !virtual_dir {
                                self.svc.places.visit(path.clone());
                                self.svc.zoxide_add(&path);
                                self.svc.watcher.watch(path);
                            }
                            self.preview.invalidate();
                        } else {
                            let existing: BTreeSet<OsString> = self
                                .tab
                                .listing
                                .all()
                                .iter()
                                .map(|e| e.name.clone())
                                .collect();
                            self.tab.sel.marked.retain(|n| existing.contains(n));
                        }
                        self.sync_folder_sizes();
                        self.rebuild_visible();
                        self.tab.restore_cursor(keep);
                        self.tab.load = LoadState::Ready;
                        self.request_preview();
                        self.run_demo();
                    }
                    Err(msg) => {
                        self.tab.nav_move = None;
                        self.tab.load = LoadState::Ready;
                        match denied {
                            Some(d) => self.on_access_denied(*d),
                            None => self.toast(ToastKind::Error, msg, 6),
                        }
                    }
                }
            }
            DirEvent::Patched { dir, updates, .. } => {
                if dir != self.tab.cwd {
                    return;
                }
                let keep = self.tab.cursor_name();
                self.tags.note(updates.iter().filter_map(|u| match u {
                    rada_core::model::EntryUpdate::Upsert(e) => Some(e),
                    _ => None,
                }));
                self.tab.listing.apply(updates);
                self.sync_folder_sizes();
                let existing: BTreeSet<OsString> = self
                    .tab
                    .listing
                    .all()
                    .iter()
                    .map(|e| e.name.clone())
                    .collect();
                self.tab.sel.marked.retain(|n| existing.contains(n));
                self.rebuild_visible();
                self.tab.restore_cursor(keep);
                self.preview.invalidate(); // contents may have changed
                self.request_preview();
            }
        }
    }

    fn on_watch(&mut self, ev: WatchEvent) {
        match ev {
            WatchEvent::Changed { dir, paths, rescan } => {
                if dir != self.tab.cwd {
                    return;
                }
                self.forget_folder_sizes(&dir, &paths, rescan);
                // Going to another folder (or into an archive) is on its way: reading this one
                // again now would take the place of that request, and the person would be left
                // where they were. The folder that is opened is read as it is then.
                if matches!(&self.tab.load, LoadState::Loading(target) if *target != dir) {
                    return;
                }
                let generation = self.tabs.next_generation();
                // A full read in progress is answered only for the newest request: a patch
                // would supersede it and leave the folder "loading" for good, so a change
                // that arrives meanwhile asks for another full read instead.
                if rescan || matches!(self.tab.load, LoadState::Loading(_)) {
                    self.svc.loader.load(dir, generation);
                } else {
                    self.svc.loader.patch(dir, paths, generation);
                }
            }
            // Polling still shows the changes: there is nothing to tell.
            WatchEvent::Degraded(_) => {}
            WatchEvent::Failed(msg) => {
                self.toast(ToastKind::Warn, tf(Key::TLiveOff, &[&msg]), 6);
            }
        }
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
                }) = &mut self.windows.modal
                    && *j == job
                {
                    (*f, *d, *b, *c) = (files, dirs, bytes, current);
                }
            }
            JobEvent::Planned {
                job,
                plan,
                scan,
                undo,
            } => self.on_planned(job, *plan, scan, undo),
            JobEvent::PlanFailed { job, message } | JobEvent::Error { job, message } => {
                if self.ops.fail(job) {
                    self.windows.modal = None;
                    // A sentence that does not fit a one-line message is read in a window.
                    if message.chars().count() > 70 {
                        self.windows.modal = Some(Modal::Result(ResultView {
                            title: Key::ResultCouldNot.tr().to_string(),
                            lines: vec![(ToastKind::Error, message)],
                            scroll: 0,
                        }));
                    } else {
                        self.toast(ToastKind::Error, message, 6);
                    }
                }
            }
            JobEvent::UndoLabel { label, .. } => self.ops.undo_label = label,
            JobEvent::NothingToUndo { job } => {
                if self.ops.end_plan(job) {
                    self.windows.modal = None;
                    self.toast(ToastKind::Info, Key::TNothingUndo.tr(), 3);
                }
            }
            JobEvent::NothingToRedo { job } => {
                if self.ops.end_plan(job) {
                    self.windows.modal = None;
                    self.toast(ToastKind::Info, Key::TNothingRedo.tr(), 4);
                }
            }
            JobEvent::Progress { job, progress } => {
                self.ops.progress(job, progress);
            }
            JobEvent::AskFailure { job, info, reply } => {
                if self.ops.is_running(job) {
                    self.windows.modal = Some(Modal::Failure { job, info, reply });
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
                if let Some(done) = self.ops.finish(job) {
                    self.finish(title, report, journal_errors, Some(done));
                    // Refresh even if live updates are unavailable.
                    let generation = self.tabs.next_generation();
                    self.svc.loader.load(self.tab.cwd.clone(), generation);
                    self.svc.jobs.load_undo_label();
                }
            }
            JobEvent::Recovered { items } => {
                let mut lines: Vec<(ToastKind, String)> = Vec::new();
                for r in &items {
                    lines.push((
                        ToastKind::Warn,
                        tf(
                            Key::ResultRecovered,
                            &[&r.title, &r.removed_temps.len(), &r.adopted.len()],
                        ),
                    ));
                    for n in &r.notes {
                        lines.push((ToastKind::Warn, n.clone()));
                    }
                }
                lines.push((ToastKind::Info, Key::ResultOpenHistory.tr().to_string()));
                self.windows.modal = Some(Modal::Result(ResultView {
                    title: Key::ResultInterruptedTitle.tr().to_string(),
                    lines,
                    scroll: 0,
                }));
            }
            JobEvent::History { entries, .. } => {
                if let Some(Modal::History(h)) = &mut self.windows.modal {
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
        use rada_core::ops::RunStatus::*;
        let mut lines: Vec<(ToastKind, String)> = Vec::new();
        for f in &report.failed {
            lines.push((ToastKind::Error, f.error.clone()));
        }
        if report.skipped_dependent > 0 {
            lines.push((
                ToastKind::Warn,
                if report.skipped_dependent == 1 {
                    Key::NSkippedFolderOne.tr().to_string()
                } else {
                    tf(Key::NSkippedFolderMany, &[&report.skipped_dependent])
                },
            ));
        }
        for (_, note) in &report.kept {
            lines.push((ToastKind::Warn, note.clone()));
        }
        // What a copy could not carry over (owner, extended attributes, ACLs...), one line per
        // kind with how many items and an example.
        let mut lost: std::collections::BTreeMap<&str, Vec<&Path>> = Default::default();
        for (path, what) in &report.not_preserved {
            lost.entry(what.as_str()).or_default().push(path);
        }
        for (what, paths) in lost {
            lines.push((
                ToastKind::Warn,
                tf(
                    Key::NNotPreserved,
                    &[
                        &what,
                        &fmt::count(paths.len() as u64, Key::ItemOne, Key::ItemMany),
                        &fmt::short_path(paths[0], self.home()),
                    ],
                ),
            ));
        }
        if journal_errors > 0 {
            lines.push((
                ToastKind::Warn,
                if journal_errors == 1 {
                    Key::NJournalErrorsOne.tr().to_string()
                } else {
                    tf(Key::NJournalErrorsMany, &[&journal_errors])
                },
            ));
        }
        let (kind, headline) = match report.status() {
            Completed if report.kept.is_empty() && report.not_preserved.is_empty() => {
                // The clean ending: a card with what was done, the way back and the way to see it.
                if lines.is_empty() {
                    let notice = match done.as_ref() {
                        Some(r) => self.done_notice(r),
                        None => Notice::new(NoticeKind::Ok, tf(Key::NDoneDefault, &[&title])),
                    };
                    self.notify(notice);
                    return;
                }
                (ToastKind::Ok, tf(Key::NDoneDefault, &[&title]))
            }
            Completed if report.not_preserved.is_empty() => {
                (ToastKind::Warn, tf(Key::NDoneKept, &[&title]))
            }
            Completed => (ToastKind::Warn, tf(Key::NDoneNotKept, &[&title])),
            CompletedWithProblems => (
                ToastKind::Warn,
                tf(
                    Key::NProblems,
                    &[
                        &title,
                        &(report.failed.len() + report.skipped_dependent as usize),
                    ],
                ),
            ),
            Aborted => (ToastKind::Warn, tf(Key::NStopped, &[&title])),
            Cancelled => (ToastKind::Warn, tf(Key::NCancelled, &[&title])),
            Interrupted => (ToastKind::Warn, tf(Key::NInterrupted, &[&title])),
        };
        if lines.is_empty() {
            self.toast(kind, headline, 4);
        } else {
            self.windows.modal = Some(Modal::Result(ResultView {
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
        if let Some(Modal::Plan(pv)) = &mut self.windows.modal
            && pv.replanning == Some(job)
        {
            pv.plan = plan;
            pv.scan = scan.or(pv.scan.take());
            pv.replanning = None;
            pv.scroll = 0;
            return;
        }
        if !self.ops.end_plan(job) {
            return; // the user cancelled while it was being planned
        }
        let replan = match (&plan.kind, &plan.destination) {
            (rada_core::ops::OpKind::Copy, Some(d)) => Replan::Transfer {
                dest: d.clone(),
                mode: TransferMode::Copy,
            },
            (rada_core::ops::OpKind::Move, Some(d)) => Replan::Transfer {
                dest: d.clone(),
                mode: TransferMode::Move,
            },
            (OpKind::Extract | OpKind::Compress, _) if plan.request.is_some() => Replan::Request,
            (OpKind::Rename, _) if plan.has_conflicts() && plan.blocking().next().is_none() => {
                Replan::Request
            }
            _ => Replan::None,
        };
        if self.hooks.demo_auto_run {
            self.hooks.demo_auto_run = false;
            self.windows.modal = None;
            let pv = Box::new(PlanView {
                plan,
                scan,
                undo,
                replan,
                scroll: 0,
                typed: String::new(),
                replanning: None,
                details: false,
                cancel_focus: false,
            });
            self.run_plan(pv);
            return;
        }
        self.windows.modal = Some(Modal::Plan(Box::new(PlanView {
            plan,
            scan,
            undo,
            replan,
            scroll: 0,
            typed: String::new(),
            replanning: None,
            details: false,
            cancel_focus: false,
        })));
    }

    // ------------------------------------------------------------------ starting operations

    fn begin_plan(&mut self, what: &str, handle: JobHandle) {
        self.ops.plan_job = Some(handle.id);
        self.windows.modal = Some(Modal::Scanning {
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
        if self.ops.is_busy() {
            self.toast(ToastKind::Warn, Key::TBusy.tr(), 3);
            true
        } else {
            false
        }
    }

    fn start_paste(&mut self) {
        if self.refuse_if_busy()
            || self.refuse_in_archive(Key::WhatPasteHere.tr())
            || self.refuse_in_tag_view(Key::WhatPaste.tr())
        {
            return;
        }
        let Some(clip) = self.ops.clipboard.clone() else {
            self.toast(ToastKind::Info, Key::TNothingCopied.tr(), 3);
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
            .plan_transfer(clip.paths, self.tab.cwd.clone(), opts);
        self.begin_plan(
            if clip.mode == TransferMode::Copy {
                Key::PlanningCopy.tr()
            } else {
                Key::PlanningMove.tr()
            },
            h,
        );
    }

    fn start_trash(&mut self, permanent: bool) {
        if self.refuse_if_busy() || self.refuse_in_archive(Key::WhatDelete.tr()) {
            return;
        }
        let t = self.tab.targets();
        if t.is_empty() {
            return;
        }
        let h = if permanent {
            self.svc.jobs.plan_delete(t)
        } else {
            self.svc.jobs.plan_trash(t)
        };
        self.begin_plan(Key::PlanningGeneric.tr(), h);
    }

    fn start_undo(&mut self, id: Option<String>) {
        if self.refuse_if_busy() {
            return;
        }
        let h = self.svc.jobs.plan_undo(id);
        self.begin_plan(Key::PlanningUndo.tr(), h);
    }

    /// Ask for the same transfer again with another way of settling name clashes.
    fn replan_with(&mut self, pv: &mut PlanView, policy: ConflictPolicy) {
        if pv.needs_typed_confirmation() || pv.replanning.is_some() || pv.plan.policy == policy {
            return;
        }
        match pv.replan.clone() {
            Replan::Transfer { dest, mode } => {
                if let Some(scan) = pv.scan.clone() {
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
            Replan::Request => {
                if let Some(req) = pv
                    .plan
                    .request
                    .as_ref()
                    .and_then(|r| r.with_conflict(policy))
                {
                    let h = self.svc.jobs.plan_request(req);
                    pv.replanning = Some(h.id);
                }
            }
            Replan::None => {}
        }
    }

    fn run_plan(&mut self, pv: Box<PlanView>) {
        // A file chooser makes a folder and nothing else, whatever asked for more.
        if self.session.chooser.is_some() && pv.plan.kind != OpKind::MakeDir {
            self.toast(ToastKind::Info, Key::ChooseRefused.tr(), 3);
            return;
        }
        let title = pv.plan.title.clone();
        let total = pv.plan.total_bytes();
        let (kind, totals, reversible) = (pv.plan.kind, pv.plan.totals, pv.plan.reversible);
        let outcome = self.outcome_of(&pv.plan);
        let handle = match pv.undo {
            Some(up) => self.svc.jobs.execute_undo(*up),
            None => self.svc.jobs.execute(pv.plan),
        };
        self.ops.running = Some(Running {
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
            outcome,
        });
        self.tab.sel.clear_marks();
    }

    fn submit_input(&mut self, iv: InputView) {
        match &iv.kind {
            InputKind::Rename { from } => {
                let h = self
                    .svc
                    .jobs
                    .plan_rename(from.clone(), iv.text.trim_end_matches('\n').to_string());
                self.begin_plan(Key::PlanningRename.tr(), h);
            }
            InputKind::SaveAs => self.submit_save_as(iv),
            InputKind::NewDir => {
                let h = self
                    .svc
                    .jobs
                    .plan_mkdir(self.tab.cwd.clone(), iv.text.clone());
                self.begin_plan(Key::PlanningGeneric.tr(), h);
            }
            InputKind::ExtractTo {
                archive,
                dest,
                default,
            } => {
                let name = iv.text.trim().to_string();
                let into = if name == *default {
                    ExtractInto::Auto
                } else {
                    ExtractInto::Folder { name }
                };
                let h = self.svc.jobs.plan_request(OpRequest::Extract {
                    archive: archive.clone(),
                    destination: dest.clone(),
                    into,
                    only: Vec::new(),
                    conflict: ConflictPolicy::Skip,
                });
                self.begin_plan(Key::ReadingArchive.tr(), h);
            }
            InputKind::Compress {
                sources,
                dir,
                format,
            } => {
                let mut name = iv.text.trim().to_string();
                if name.contains(['/', '\\']) {
                    let mut iv = iv;
                    iv.error = Some("a name cannot contain a slash".into());
                    self.windows.modal = Some(Modal::Input(iv));
                    return;
                }
                let kind = match ArchiveKind::from_name(std::ffi::OsStr::new(&name)) {
                    Some(k) => k,
                    None => {
                        name.push_str(format.extension());
                        *format
                    }
                };
                let h = self.svc.jobs.plan_request(OpRequest::Compress {
                    sources: sources.clone(),
                    archive: dir.join(&name),
                    format: kind,
                    conflict: ConflictPolicy::Skip,
                });
                self.begin_plan(Key::PlanningCompress.tr(), h);
            }
            InputKind::NewFile => {
                let h = self
                    .svc
                    .jobs
                    .plan_mkfile(self.tab.cwd.clone(), iv.text.clone());
                self.begin_plan(Key::PlanningGeneric.tr(), h);
            }
            InputKind::NewTag { items } => match rada_core::platform::tags::validate_name(&iv.text)
            {
                Ok(name) => {
                    let h = self.svc.jobs.plan_request(OpRequest::SetTags {
                        items: items.clone(),
                        change: rada_core::ops::TagChange::Add {
                            tag: rada_core::platform::Tag::named(&name),
                        },
                    });
                    self.begin_plan(Key::PlanningGeneric.tr(), h);
                }
                Err(e) => {
                    let mut iv = iv;
                    iv.error = Some(e);
                    self.windows.modal = Some(Modal::Input(iv));
                }
            },
            InputKind::OpenWith { path } => {
                let program = iv.text.trim().to_string();
                if program.is_empty() {
                    self.windows.modal = Some(Modal::Input(iv));
                    return;
                }
                let (path, platform, notes) = (
                    path.clone(),
                    self.svc.platform.clone(),
                    self.svc.notes.clone(),
                );
                self.redraw.after_launch();
                std::thread::spawn(move || {
                    if let Err(e) = platform.opener().open_with(&path, &program) {
                        let _ = notes.send(CoreEvent::Note {
                            text: e.to_string(),
                            error: true,
                        });
                    }
                });
            }
            InputKind::BulkRename { items } => match Pattern::parse(&iv.text) {
                Ok(_) => {
                    let h = self
                        .svc
                        .jobs
                        .plan_bulk_rename(items.clone(), iv.text.clone());
                    self.begin_plan(Key::PlanningRename.tr(), h);
                }
                Err(e) => {
                    let mut iv = iv;
                    iv.error = Some(e);
                    self.windows.modal = Some(Modal::Input(iv));
                }
            },
        }
    }

    // ------------------------------------------------------------------ keys

    fn yank(&mut self, mode: TransferMode) {
        if mode == TransferMode::Move && self.refuse_in_archive(Key::WhatCut.tr()) {
            return;
        }
        let paths = self.tab.targets();
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        self.ops.clipboard = Some(Clip { mode, paths });
        self.tab.sel.clear_marks();
        let verb = match (mode == TransferMode::Copy, n == 1) {
            (true, true) => Key::TYankCopiedOne,
            (true, false) => Key::TYankCopiedMany,
            (false, true) => Key::TYankCutOne,
            (false, false) => Key::TYankCutMany,
        };
        self.toast(
            ToastKind::Info,
            tf(
                verb,
                &[
                    &fmt::count(n as u64, Key::ItemOne, Key::ItemMany),
                    &self
                        .key_for(Action::Paste)
                        .unwrap_or_else(|| Key::KeyPasteWord.tr().into()),
                ],
            ),
            3,
        );
    }

    fn set_sort(&mut self, spec: SortSpec) {
        let keep = self.tab.cursor_name();
        self.tab.sort = spec;
        // From now on this tab keeps its order, whatever the configuration says.
        self.tab.sort_chosen = true;
        self.save_ui_state();
        self.tab.listing.set_sort(spec); // immediate, in memory
        self.rebuild_visible();
        self.tab.restore_cursor(keep);
        let arrow = if spec.descending() { "↓" } else { "↑" };
        self.toast(
            ToastKind::Info,
            tf(
                Key::TSortedBy,
                &[
                    &match spec.key {
                        SortKey::Name => Key::SortKeyName,
                        SortKey::Size => Key::SortKeySize,
                        SortKey::Modified => Key::SortKeyDate,
                        SortKey::Type => Key::SortKeyType,
                    }
                    .tr(),
                    &arrow,
                ],
            ),
            2,
        );
    }

    fn start_rename(&mut self) {
        if self.refuse_if_busy() || self.refuse_in_archive(Key::WhatRename.tr()) {
            return;
        }
        if let Some(e) = self.tab.current() {
            // The name on the disk (in a list of tagged items `name` is the whole path).
            let text = e
                .path
                .file_name()
                .unwrap_or(&e.name)
                .to_string_lossy()
                .into_owned();
            let from = e.path.clone();
            self.windows.modal = Some(Modal::Input(InputView::new(
                InputKind::Rename { from },
                text,
            )));
        }
    }

    fn start_bulk_rename(&mut self) {
        if self.refuse_if_busy() || self.refuse_in_archive(Key::WhatRename.tr()) {
            return;
        }
        let items = self.tab.targets();
        if items.is_empty() {
            return;
        }
        self.windows.modal = Some(Modal::Input(InputView::new(
            InputKind::BulkRename { items },
            "{name}_{n:2}.{ext}".to_string(),
        )));
    }

    fn palette_items(&self) -> Vec<PaletteItem> {
        let mut items: Vec<PaletteItem> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let home = self.svc.home.clone();
        let cwd = self.tab.cwd.clone();
        // A mount point is listed once, as a disk (so "/" is not both "System" and a disk).
        let mounts: std::collections::HashSet<PathBuf> = self
            .devices
            .volumes
            .iter()
            .map(|v| v.mount_point.clone())
            .collect();
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
                    Key::HomeLabel.tr().to_string()
                } else {
                    path.file_name()
                        .map(rada_core::display::name)
                        .unwrap_or_else(|| rada_core::display::path(path))
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
        for p in &self.places.bookmarks() {
            add(&mut items, p, PaletteKind::Bookmark, None, None);
        }
        for p in self.places.paths.recents.iter().filter(|p| **p != cwd) {
            add(&mut items, p, PaletteKind::Recent, None, None);
        }
        for place in &self.places.paths.places {
            add(&mut items, &place.path, PaletteKind::Place, None, None);
        }
        add(
            &mut items,
            Path::new(std::path::MAIN_SEPARATOR_STR),
            PaletteKind::Place,
            Some(Key::DiskSystem.tr().into()),
            Some(std::path::MAIN_SEPARATOR_STR.into()),
        );
        for v in &self.devices.volumes {
            let label = v.label.clone().unwrap_or_else(|| {
                if v.mount_point == Path::new(std::path::MAIN_SEPARATOR_STR) {
                    Key::DiskSystem.tr().to_string()
                } else {
                    rada_core::display::path(&v.mount_point)
                }
            });
            let free = v
                .available
                .map(|a| tf(Key::PalFreeDetail, &[&fmt::size(a)]))
                .unwrap_or_else(|| {
                    if v.responsive {
                        String::new()
                    } else {
                        Key::PalNotResponding.tr().into()
                    }
                });
            let detail = format!("{}{free}", rada_core::display::path(&v.mount_point));
            add(
                &mut items,
                &v.mount_point,
                PaletteKind::Disk,
                Some(label),
                Some(detail),
            );
        }
        if self.tab.tag_view().is_none() {
            for anc in cwd.ancestors().skip(1).take(4) {
                add(&mut items, anc, PaletteKind::Parent, None, None);
            }
        }
        let dirs: Vec<PathBuf> = self
            .tab
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
        self.windows.modal = Some(Modal::Palette(Box::new(PaletteView::new(
            items,
            self.svc.home.clone(),
        ))));
    }

    /// New recents/volumes arrived while the palette is open: refresh its list, keep the query.
    fn refresh_palette(&mut self) {
        if matches!(self.windows.modal, Some(Modal::Palette(_))) {
            let items = self.palette_items();
            if let Some(Modal::Palette(p)) = &mut self.windows.modal {
                p.set_items(items);
            }
        }
    }

    /// Developer hook (`--demo`): put the interface in a ready-made state, once.
    fn run_demo(&mut self) {
        let Some(d) = self.hooks.demo.take() else {
            return;
        };
        let (scene, arg) = d
            .scene
            .split_once(':')
            .map_or((d.scene.as_str(), None), |(a, b)| (a, Some(b)));
        match scene {
            "palette" => {
                self.open_palette();
                if let (Some(Modal::Palette(p)), Some(q)) = (&mut self.windows.modal, arg) {
                    for c in q.chars() {
                        p.insert(c);
                    }
                }
            }
            "plan" | "progress" => {
                let paths: Vec<PathBuf> = self
                    .tab
                    .visible
                    .iter()
                    .filter_map(|&i| self.tab.listing.all().get(i))
                    .map(|e| e.path.clone())
                    .collect();
                self.tab.sel.marked = self
                    .tab
                    .visible
                    .iter()
                    .filter_map(|&i| self.tab.listing.all().get(i))
                    .map(|e| e.name.clone())
                    .collect();
                let Some(dest) = d.dest else { return };
                self.hooks.demo_auto_run = scene == "progress";
                let opts = TransferOptions::copy(ConflictPolicy::Skip);
                let h = self.svc.jobs.plan_transfer(paths, dest, opts);
                self.begin_plan(Key::PlanningCopy.tr(), h);
            }
            "history" => {
                self.windows.modal = Some(Modal::History(HistoryView {
                    entries: Vec::new(),
                    selected: 0,
                    loading: true,
                }));
                self.svc.jobs.load_history();
            }
            // The list of the files used lately (the demo home has a `recently-used.xbel`).
            "recenti" => self.open_dir(rada_core::recents::view_path()),
            // A selection and something on the clipboard, to show the bottom bar at work.
            "selection" => {
                let files: Vec<_> = self
                    .tab
                    .listing
                    .all()
                    .iter()
                    .filter(|e| !e.is_dir())
                    .map(|e| (e.name.clone(), e.path.clone()))
                    .collect();
                self.tab.sel.marked = files.iter().take(3).map(|(n, _)| n.clone()).collect();
                self.ops.clipboard = Some(Clip {
                    mode: TransferMode::Copy,
                    paths: files
                        .iter()
                        .skip(3)
                        .take(2)
                        .map(|(_, p)| p.clone())
                        .collect(),
                });
            }
            // The state the README's pictures show: an item selected, two waiting on the
            // clipboard, the cursor on a picture, and a card saying the last copy went through.
            "vetrina" => {
                let by_name = |app: &App, name: &str| {
                    app.tab
                        .listing
                        .all()
                        .iter()
                        .find(|e| e.display == name)
                        .map(|e| (e.name.clone(), e.path.clone()))
                };
                let chosen: Vec<_> = ["backup.zip"]
                    .iter()
                    .filter_map(|n| by_name(self, n))
                    .collect();
                self.tab.sel.marked = chosen.iter().map(|(n, _)| n.clone()).collect();
                let waiting: Vec<PathBuf> = ["data.json", "main.rs"]
                    .iter()
                    .filter_map(|n| by_name(self, n))
                    .map(|(_, p)| p)
                    .collect();
                if !waiting.is_empty() {
                    self.ops.clipboard = Some(Clip {
                        mode: TransferMode::Copy,
                        paths: waiting,
                    });
                }
                self.tab.restore_cursor(Some(OsString::from("photo.jpg")));
                self.tab.ensure_visible();
                self.request_preview();
                let items = fmt::count(3, Key::ItemOne, Key::ItemMany);
                self.ops.undo_label = Some(tf(Key::DoneCopy, &[&items]));
                let folder = self
                    .places
                    .paths
                    .places
                    .iter()
                    .find(|p| p.kind == rada_core::platform::PlaceKind::Documents)
                    .map(|p| p.path.clone())
                    .unwrap_or_else(|| self.svc.home.clone());
                let name = self.folder_label_for_demo(&folder);
                self.notify(
                    Notice::new(NoticeKind::Ok, tf(Key::NCopyMany, &[&3, &name]))
                        .with_action(NoticeAction::Undo)
                        .showing(folder, None),
                );
            }
            "main" => {}
            "help" => self.windows.modal = Some(Modal::Help),
            // The menu of the right button on the picture, and the menus beside it.
            // "menubottom": the menu of a file among the last rows of the list.
            "menu" | "openwith" | "tagmenu" | "menubottom" => {
                let (file, at) = if scene == "menubottom" {
                    ("song.mp3", (30, 28))
                } else {
                    ("photo.jpg", (24, 12))
                };
                self.tab.restore_cursor(Some(OsString::from(file)));
                self.tab.ensure_visible();
                self.request_preview();
                let mut m = self.context_menu(true, at);
                let want = match scene {
                    "openwith" => Some(MenuCmd::OpenWithList),
                    "tagmenu" => Some(MenuCmd::Sub(MenuKind::Tags)),
                    _ => None,
                };
                if let Some(cmd) = want
                    && let Some(i) = m.items.iter().position(|it| it.cmd == cmd)
                {
                    m.selected = i;
                    self.menu_activate(m);
                } else {
                    self.windows.modal = Some(Modal::Menu(m));
                }
            }
            // Two cards piled: a copy that is running (slowly: RADA_DEMO_SLOW_MBPS) over one
            // that is done, or an error over one that is done.
            "toasts" | "toasts-error" => {
                let long = Duration::from_secs(60);
                let trash = self
                    .places
                    .paths
                    .places
                    .iter()
                    .find(|p| p.kind == rada_core::platform::PlaceKind::Trash)
                    .map(|p| p.path.clone());
                let mut done = Notice::new(NoticeKind::Ok, tf(Key::NTrashMany, &[&3]))
                    .with_action(NoticeAction::Undo)
                    .lasting(long);
                if let Some(t) = trash {
                    done = done.showing(t, None);
                }
                if scene == "toasts-error" {
                    self.notify(
                        Notice::new(NoticeKind::Error, Key::PvCannotImage.tr()).lasting(long),
                    );
                    self.notify(done);
                    return;
                }
                self.notify(done);
                let names = ["archive.tar.gz", "backup.zip"];
                self.tab.sel.marked = names.iter().map(|n| OsString::from(*n)).collect();
                let paths: Vec<PathBuf> = names
                    .iter()
                    .filter_map(|n| {
                        self.tab
                            .listing
                            .all()
                            .iter()
                            .find(|e| e.display == *n)
                            .map(|e| e.path.clone())
                    })
                    .collect();
                let Some(dest) = d.dest else { return };
                self.hooks.demo_auto_run = true;
                let opts = TransferOptions::copy(ConflictPolicy::KeepBoth);
                let h = self.svc.jobs.plan_transfer(paths, dest, opts);
                self.begin_plan(Key::PlanningCopy.tr(), h);
            }
            // The windows of a permanent deletion and of a copy that meets a file of the same
            // name, on three marked items.
            "delete" | "conflict" => {
                let names = ["archive.tar.gz", "backup.zip", "Cargo.toml"];
                self.tab.sel.marked = names.iter().map(|n| OsString::from(*n)).collect();
                self.tab.restore_cursor(Some(OsString::from("photo.jpg")));
                self.tab.ensure_visible();
                if scene == "delete" {
                    self.dispatch(Action::DeletePermanently);
                } else if let Some(dest) = d.dest {
                    let paths: Vec<PathBuf> = names
                        .iter()
                        .filter_map(|n| {
                            self.tab
                                .listing
                                .all()
                                .iter()
                                .find(|e| e.display == *n)
                                .map(|e| e.path.clone())
                        })
                        .collect();
                    let opts = TransferOptions::copy(ConflictPolicy::KeepBoth);
                    let h = self.svc.jobs.plan_transfer(paths, dest, opts);
                    self.begin_plan(Key::PlanningCopy.tr(), h);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ modal keys

    fn on_modal_key(&mut self, key: KeyEvent) {
        let Some(modal) = self.windows.modal.take() else {
            return;
        };
        match modal {
            Modal::Scanning { .. } => self.on_scanning_key(modal, key),
            Modal::Plan(pv) => self.on_plan_key(pv, key),
            Modal::Failure { job, info, reply } => self.on_failure_key(job, info, reply, key),
            Modal::Input(iv) => self.on_input_key(iv, key),
            Modal::History(h) => self.on_history_key(h, key),
            Modal::OpenWith(v) => self.on_open_with_key(v, key),
            Modal::Palette(p) => self.on_palette_key(p, key),
            Modal::Result(r) => self.on_result_key(r, key),
            Modal::ConfirmQuit => self.on_confirm_key(key),
            Modal::Help => self.on_help_key(key),
            Modal::Menu(m) => self.on_menu_key(m, key),
            Modal::FullPreview => self.on_full_preview_key(key),
            Modal::Update(view) => self.on_update_key(view, key),
            Modal::Access(d) => self.on_access_key(d, key),
            Modal::Eject(v) => self.on_eject_key(v, key),
            Modal::Font(v) => self.on_font_key(v, key),
            Modal::Properties(p) => self.on_properties_key(p, key),
        }
    }

    fn on_scanning_key(&mut self, scanning: Modal, key: KeyEvent) {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            if let Modal::Scanning { cancel, .. } = &scanning {
                cancel.cancel();
            }
            self.ops.plan_job = None;
        } else {
            self.windows.modal = Some(scanning);
        }
    }

    fn on_plan_key(&mut self, mut pv: Box<PlanView>, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Char('q') if !pv.needs_typed_confirmation() => {}
            KeyCode::Enter => {
                if pv.cancel_focus {
                    // The keyboard is on Cancel: Enter cancels.
                } else if pv.can_run() {
                    self.run_plan(pv);
                } else {
                    self.windows.modal = Some(Modal::Plan(pv));
                }
            }
            KeyCode::Char('c')
                if !pv.needs_typed_confirmation() && !matches!(pv.replan, Replan::None) =>
            {
                let policy = pv.plan.policy.next();
                self.replan_with(&mut pv, policy);
                self.windows.modal = Some(Modal::Plan(pv));
            }
            // ← → choose how name clashes are settled (the pills, in the order they are in).
            KeyCode::Left | KeyCode::Right
                if !pv.needs_typed_confirmation() && !matches!(pv.replan, Replan::None) =>
            {
                let dir = if key.code == KeyCode::Right { 1 } else { -1 };
                let policy = crate::ui::policy_step(pv.plan.policy, dir);
                if policy != pv.plan.policy {
                    self.replan_with(&mut pv, policy);
                }
                self.windows.modal = Some(Modal::Plan(pv));
            }
            // Tab moves the keyboard between the buttons.
            KeyCode::Tab | KeyCode::BackTab => {
                pv.cancel_focus = !pv.cancel_focus;
                self.windows.modal = Some(Modal::Plan(pv));
            }
            // d: every step, or the summary.
            KeyCode::Char('d') if !pv.needs_typed_confirmation() => {
                pv.details = !pv.details;
                pv.scroll = 0;
                self.windows.modal = Some(Modal::Plan(pv));
            }
            KeyCode::Down | KeyCode::Char('j') if !pv.needs_typed_confirmation() => {
                pv.scroll += 1;
                self.windows.modal = Some(Modal::Plan(pv));
            }
            KeyCode::Up | KeyCode::Char('k') if !pv.needs_typed_confirmation() => {
                pv.scroll = pv.scroll.saturating_sub(1);
                self.windows.modal = Some(Modal::Plan(pv));
            }
            KeyCode::PageDown => {
                pv.scroll += 10;
                self.windows.modal = Some(Modal::Plan(pv));
            }
            KeyCode::PageUp => {
                pv.scroll = pv.scroll.saturating_sub(10);
                self.windows.modal = Some(Modal::Plan(pv));
            }
            KeyCode::Backspace if pv.needs_typed_confirmation() => {
                pv.typed.pop();
                self.windows.modal = Some(Modal::Plan(pv));
            }
            KeyCode::Char(c) if pv.needs_typed_confirmation() && !ctrl => {
                pv.typed.push(c);
                self.windows.modal = Some(Modal::Plan(pv));
            }
            _ => self.windows.modal = Some(Modal::Plan(pv)),
        }
    }

    fn on_failure_key(
        &mut self,
        job: JobId,
        info: FailureInfo,
        reply: Sender<ErrorChoice>,
        key: KeyEvent,
    ) {
        let choice = match key.code {
            KeyCode::Char('s') | KeyCode::Enter => Some(ErrorChoice::Skip),
            KeyCode::Char('S') => Some(ErrorChoice::SkipAll),
            KeyCode::Char('r') => Some(ErrorChoice::Retry),
            KeyCode::Char('a') | KeyCode::Esc | KeyCode::Char('q') => Some(ErrorChoice::Abort),
            _ => None,
        };
        match choice {
            Some(c) => {
                let _ = reply.send(c);
            }
            None => self.windows.modal = Some(Modal::Failure { job, info, reply }),
        }
    }

    fn on_input_key(&mut self, mut iv: InputView, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Tab if matches!(iv.kind, InputKind::Compress { .. }) => {
                iv.cycle_format();
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Enter => {
                if iv.text.is_empty() {
                    self.windows.modal = Some(Modal::Input(iv));
                } else {
                    self.submit_input(iv);
                }
            }
            KeyCode::Char(c) if !ctrl => {
                iv.insert(c);
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Backspace => {
                iv.backspace();
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Delete => {
                iv.delete();
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Left => {
                iv.cursor = iv.cursor.saturating_sub(1);
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Right => {
                iv.cursor = (iv.cursor + 1).min(iv.text.chars().count());
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Home => {
                iv.cursor = 0;
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::End => {
                iv.cursor = iv.text.chars().count();
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Char('u') if ctrl => {
                iv.text.clear();
                iv.cursor = 0;
                self.windows.modal = Some(Modal::Input(iv));
            }
            _ => self.windows.modal = Some(Modal::Input(iv)),
        }
    }

    fn on_history_key(&mut self, mut h: HistoryView, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('U') => {}
            KeyCode::Down | KeyCode::Char('j') => {
                h.selected = (h.selected + 1).min(h.entries.len().saturating_sub(1));
                self.windows.modal = Some(Modal::History(h));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                h.selected = h.selected.saturating_sub(1);
                self.windows.modal = Some(Modal::History(h));
            }
            KeyCode::Enter | KeyCode::Char('u') => {
                if let Some(e) = h.entries.get(h.selected).cloned() {
                    if e.is_undoable() || e.can_retry_undo() {
                        self.start_undo(Some(e.id));
                    } else {
                        self.toast(ToastKind::Info, Key::TCannotUndoThis.tr(), 4);
                        self.windows.modal = Some(Modal::History(h));
                    }
                }
            }
            _ => self.windows.modal = Some(Modal::History(h)),
        }
    }

    fn on_palette_key(&mut self, mut p: Box<PaletteView>, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter => {
                if let Some(item) = p.current() {
                    let path = item.path.clone();
                    self.open_dir(path);
                }
            }
            KeyCode::Down => {
                p.move_by(1);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Up => {
                p.move_by(-1);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Char('n') | KeyCode::Char('j') if ctrl => {
                p.move_by(1);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Char('p') | KeyCode::Char('k') if ctrl => {
                p.move_by(-1);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::PageDown => {
                p.move_by(8);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::PageUp => {
                p.move_by(-8);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Char('u') if ctrl => {
                p.clear();
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Backspace => {
                p.backspace();
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Char(c) if !ctrl => {
                p.insert(c);
                self.windows.modal = Some(Modal::Palette(p));
            }
            _ => self.windows.modal = Some(Modal::Palette(p)),
        }
    }

    fn on_result_key(&mut self, mut r: ResultView, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('u') => self.start_undo(None),
            KeyCode::Char('z') if ctrl => self.start_undo(None),
            KeyCode::Down | KeyCode::Char('j') => {
                r.scroll += 1;
                self.windows.modal = Some(Modal::Result(r));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                r.scroll = r.scroll.saturating_sub(1);
                self.windows.modal = Some(Modal::Result(r));
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ dates for the list

    /// Work out the date text of these entries (indices into the listing), once; the list
    /// reads them with [`FolderInfo::date_text`].
    pub fn warm_dates(&mut self, indices: impl Iterator<Item = usize>) {
        let hour = self
            .now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() / 3600)
            .unwrap_or(0);
        let key = (hour, self.tab.listing.all().len());
        self.folder_info.dates_for(key);
        let now = self.now();
        for i in indices {
            if let Some(slot @ None) = self.folder_info.date_cache.get_mut(i)
                && let Some(e) = self.tab.listing.all().get(i)
            {
                *slot = Some(self.settings.explorer_date(e.used.or(e.mtime), now));
            }
        }
    }
}
