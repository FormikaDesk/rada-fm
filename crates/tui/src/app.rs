//! Application state and behaviour. Pure logic: no drawing, no I/O. Everything that
//! touches the disk is a request to a worker; everything the workers say arrives as an
//! event and changes state here.

use rada_core::i18n::{Key, tf};
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::archive::{ArchiveKind, ArchiveView};
use rada_core::events::{
    CoreEvent, DirEvent, FailureInfo, JobEvent, JobId, PreviewEvent, WatchEvent,
};
use rada_core::jobs::JobHandle;
use rada_core::journal::JournalEntry;
use rada_core::model::{DirListing, Entry, SortKey, SortSpec};
use rada_core::ops::{
    Cancel, ConflictPolicy, ErrorChoice, ExecReport, Pattern, Plan, Progress, Scan, TransferMode,
    TransferOptions, UndoPlan,
};
use rada_core::ops::{ExtractInto, OpKind, OpRequest, Totals, WarningKind};
use rada_core::platform::Volume;
use rada_core::preview::{ImageInfo, ImageState, Limits, Preview};

use crate::fmt::{self, DateFormat, DateStyle};
use crate::hits::{Hits, Target};
use crate::icons::IconSet;
use crate::images::{ImageMode, ImageUi, ResizeResult};
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

#[derive(Clone)]
pub struct Clip {
    pub mode: TransferMode,
    pub paths: Vec<PathBuf>,
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
    /// A PDF that could not be drawn: the text of its first page.
    pub text: Vec<String>,
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
    /// Where the result will be, for the card that says it is done.
    pub outcome: notice::Outcome,
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
    /// The basic profile (ASCII, 16 colours): no rounded ends, no shades, a plain look.
    pub basic: bool,
    /// The Tag column of the list: `None` follows the remembered choice, then shows it.
    pub tags_column: Option<bool>,
}

/// A theme that follows the terminal's background: which one, and how to show it. Resolved
/// when the terminal is started, because finding out may need to ask the terminal.
pub struct Adaptive {
    pub name: String,
    pub depth: crate::theme::ColorDepth,
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
        vol("/", "System", VolumeKind::Fixed, 512, 212),
        vol("/run/media/user/Data", "Data", VolumeKind::Fixed, 1000, 640),
        vol(
            "/run/media/user/USB",
            "USB drive",
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

mod address;
mod archives;
mod columns;
mod eject;
mod font;
mod input;
mod menus;
mod notice;
mod openwith;
mod props;
mod side;
mod sizes;
pub use sizes::FolderSizeView;
mod tabs;
mod update;

pub use address::{AddressEdit, BarHit, BarItem, BarMode};
pub use eject::{EjectStage, EjectView};
pub use font::{FontStage, FontView};
pub use input::{FilterState, MenuCmd, MenuItem, MenuView, TagCmd};
pub use menus::MenuKind;
pub use notice::{Notice, NoticeAction, NoticeKind, NoticeQueue};
pub use openwith::{OpenRow, OpenWithView};
pub use props::PropsView;
pub use tabs::TabState;
pub use update::{UpdateStatus, UpdateView};

/// Whether an entry carries a tag matching `want` (lower case): a name that contains it, or
/// the colour of that name. An empty `want` (`tag:` alone) means "any tag".
fn matches_tag(e: &Entry, want: &str) -> bool {
    use rada_core::platform::TagColor as Hue;
    if want.is_empty() {
        return !e.tags.is_empty();
    }
    // A colour by any of its names ("red", "rosso") is the tags of that colour.
    let hue = Hue::from_name(want);
    e.tags
        .iter()
        .any(|t| t.name.to_lowercase().contains(want) || (hue.is_some() && t.color == hue))
}

/// The language of the system's tag names (where there are tags).
fn svc_tag_language(svc: &Services) -> String {
    svc.platform
        .tags()
        .map(|t| t.language())
        .unwrap_or_else(|| "en".to_string())
}

pub struct App {
    pub th: Theme,
    pub icons: IconSet,
    svc: Services,

    pub cwd: PathBuf,
    /// Set while looking inside an archive: the folder shown is a read-only virtual one.
    pub archive: Option<ArchiveView>,
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
    /// Folders visited, for back and forward.
    pub nav: crate::nav::NavHistory,
    nav_move: Option<crate::nav::NavMove>,

    pub preview: PreviewState,
    pub image_ui: Option<ImageUi>,
    limits: Limits,
    preview_gen: u64,
    previewed: Option<PathBuf>,

    pub volumes: Vec<Volume>,
    /// Recent folders, state bookmarks and standard places (from the places worker).
    pub paths: rada_core::places::PathLists,
    cfg_bookmarks: Vec<PathBuf>,
    demo: Option<Demo>,
    /// Developer hook: show made-up disks instead of the machine's own (screenshots).
    demo_volumes: bool,
    demo_auto_run: bool,
    pub clipboard: Option<Clip>,
    pub modal: Option<Modal>,
    pub running: Option<Running>,
    /// What to tell the person: a card at the bottom right, one at a time (see `notice.rs`).
    pub notices: NoticeQueue,
    /// The folder above the one shown, for the Columns view (see `columns.rs`).
    pub columns_parent: Option<columns::ParentColumn>,
    parent_gen: u64,
    parent_asked: Option<PathBuf>,
    /// Added to the clock the notifications read: tests and screenshots move time on with it.
    notice_skew: Duration,
    pub watch_note: Option<String>,
    /// Number of rows the list can show; set by the renderer.
    pub viewport: Viewport,
    pub should_quit: bool,
    pub dirty: bool,
    pub spinner: usize,
    plan_job: Option<JobId>,

    pub keymap: Keymap,
    pub mouse: bool,
    pub show_hints: bool,
    /// The sidebar is wanted (the user's choice; the width decides how much of it shows).
    pub side_on: bool,
    /// What the last frame showed of it.
    pub side_mode: crate::sidebar::Mode,
    /// The keyboard is in the sidebar, on item `side_cursor`.
    pub side_focus: bool,
    pub side_cursor: usize,
    /// The sidebar item under the mouse.
    pub hover: Option<PathBuf>,
    pub term_width: u16,
    /// Where the field of the bar at the top was drawn (what is offered under it lines up
    /// with it).
    pub bar_field: ratatui::layout::Rect,
    /// Screen rectangles of everything clickable, rebuilt every frame.
    pub hits: Hits,
    pub filter: Option<FilterState>,
    sel_anchor: Option<(usize, BTreeSet<OsString>)>,
    last_click: Option<(Instant, usize)>,
    /// How soon a second click on the same row makes a double click.
    pub double_click: Duration,
    /// The language the names of the seven tag colours are written in (the system's).
    pub tag_lang: String,
    /// Tags of the user's own seen on the items of the folders shown so far (the sidebar's TAGS
    /// section lists them after the seven colours), by name.
    pub known_tags: Vec<rada_core::platform::Tag>,
    pub help_scroll: usize,
    /// Replaces the system clock when drawing relative dates (tests, screenshots).
    pub clock: Option<std::time::SystemTime>,

    // ---- the explorer interface
    /// How the folder of the front tab is shown.
    pub view: ViewMode,
    pub layout: LayoutKind,
    /// The details pane: `None` follows the width of the terminal.
    pub details: Option<bool>,
    pub dates: DateStyle,
    pub date_format: DateFormat,
    /// The basic profile: the soft shapes (rounded ends, shades) give way to plain ones.
    pub basic: bool,
    /// The Tag column of the list is shown.
    pub show_tags: bool,
    /// What is known of the size of folders (see `sizes.rs`).
    pub(crate) folder_sizes: sizes::Sizes,
    /// The time zone dates are shown in (the system's; tests pin it).
    pub tz: jiff::tz::TimeZone,
    pub remember_tabs: bool,
    /// The tabs; the slot of the front one is empty, its state lives in the fields above.
    tabs: Vec<TabState>,
    pub active_tab: usize,
    /// What the next undo would undo, in a few words (for the command bar).
    pub undo_label: Option<String>,
    /// The address bar while its text is being edited.
    pub address: Option<AddressEdit>,
    /// Text for the system clipboard (the "copy path" command); the event loop takes it.
    pub copied_text: Option<String>,
    /// Date strings of the visible rows, worked out once and kept until the folder, the
    /// clock's hour or the style changes.
    date_cache: Vec<Option<String>>,
    date_cache_key: (u64, usize),
    /// What the last frame showed of the details pane.
    pub details_shown: crate::ui::DetailsMode,
    /// Where tabs are saved and the rest of what is remembered between sessions.
    saved_ui: rada_core::uistate::UiState,
    /// Layout the last frame used, for the keyboard and the tests.
    pub screen: (u16, u16),
    /// Folders the system is waiting for the person to allow (macOS), as learnt from the folders
    /// that could not be opened.
    pub denied: Vec<rada_core::platform::AccessDenied>,
    /// The advice to enlarge a small window was closed (for the rest of this session).
    pub size_notice_dismissed: bool,
    /// The last frame showed it.
    pub size_notice_visible: bool,
    /// The version of this program, as the status bar shows it (the update worker's, when there
    /// is one; tests pin it so that a release does not change every screenshot).
    pub version: String,
    /// What the update worker last told.
    pub update: UpdateStatus,
    /// Somebody asked to look for updates and is waiting for the answer.
    update_asked: bool,
}

impl App {
    pub fn new(cfg: Config, svc: Services, image_ui: Option<ImageUi>) -> App {
        let decode_images = image_ui.is_some();
        // Which folder is in front, and which others were open last time.
        let mut saved: Vec<(PathBuf, ViewMode, SortSpec)> = Vec::new();
        let mut active = 0;
        if cfg.remember_tabs && !cfg.saved_ui.tabs.is_empty() {
            for t in &cfg.saved_ui.tabs {
                saved.push((
                    t.path.clone(),
                    t.view
                        .as_deref()
                        .and_then(ViewMode::parse)
                        .unwrap_or(cfg.view),
                    tabs::sort_from_saved(t, cfg.sort),
                ));
            }
            active = cfg.saved_ui.active_tab.min(saved.len() - 1);
            if cfg.start_explicit {
                match saved.iter().position(|(p, _, _)| *p == cfg.start_dir) {
                    Some(i) => active = i,
                    None => {
                        saved.push((cfg.start_dir.clone(), cfg.view, cfg.sort));
                        active = saved.len() - 1;
                    }
                }
            }
        } else {
            saved.push((cfg.start_dir.clone(), cfg.view, cfg.sort));
        }
        let (start_dir, view, sort) = saved[active].clone();
        let mut tab_slots: Vec<TabState> = saved
            .iter()
            .map(|(p, v, s)| TabState::fresh(p.clone(), *v, *s))
            .collect();
        tab_slots[active] = TabState::default();

        let mut app = App {
            th: cfg.theme,
            icons: cfg.icons,
            cwd: start_dir.clone(),
            archive: None,
            listing: DirListing::new(start_dir.clone(), Vec::new(), sort),
            load: LoadState::Loading(start_dir.clone()),
            dir_gen: 0,
            visible: Vec::new(),
            cursor: 0,
            scroll: 0,
            marked: BTreeSet::new(),
            show_hidden: cfg.show_hidden,
            sort,
            remembered: HashMap::new(),
            nav: crate::nav::NavHistory::new(start_dir.clone()),
            nav_move: None,
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
            demo_volumes: cfg.demo.is_some(),
            demo: cfg.demo,
            demo_auto_run: false,
            clipboard: None,
            modal: None,
            running: None,
            notices: NoticeQueue::default(),
            columns_parent: None,
            parent_gen: 0,
            parent_asked: None,
            notice_skew: Duration::ZERO,
            watch_note: None,
            viewport: Viewport::default(),
            should_quit: false,
            dirty: true,
            spinner: 0,
            plan_job: None,
            keymap: cfg.keymap,
            mouse: cfg.mouse,
            show_hints: cfg.show_hints,
            side_on: cfg.sidebar,
            side_mode: crate::sidebar::Mode::Hidden,
            side_focus: false,
            side_cursor: 0,
            hover: None,
            term_width: 0,
            bar_field: ratatui::layout::Rect::default(),
            hits: Hits::default(),
            filter: None,
            sel_anchor: None,
            last_click: None,
            double_click: Duration::from_millis(400),
            tag_lang: svc_tag_language(&svc),
            known_tags: Vec::new(),
            help_scroll: 0,
            clock: None,
            view,
            layout: cfg.layout,
            details: cfg.details_pane,
            dates: cfg.dates,
            date_format: cfg.date_format,
            basic: cfg.basic,
            show_tags: cfg.tags_column.or(cfg.saved_ui.tags_column).unwrap_or(true),
            folder_sizes: Default::default(),
            tz: jiff::tz::TimeZone::system(),
            remember_tabs: cfg.remember_tabs,
            tabs: tab_slots,
            active_tab: active,
            undo_label: None,
            address: None,
            copied_text: None,
            date_cache: Vec::new(),
            date_cache_key: (0, 0),
            details_shown: crate::ui::DetailsMode::Hidden,
            saved_ui: cfg.saved_ui,
            screen: (0, 0),
            denied: Vec::new(),
            size_notice_dismissed: false,
            size_notice_visible: false,
            version: svc.updater.as_ref().map_or_else(
                || env!("CARGO_PKG_VERSION").to_string(),
                |u| u.ctx().current.to_string(),
            ),
            update: UpdateStatus::None,
            update_asked: false,
            svc,
        };
        for w in std::mem::take(&mut app.keymap.warnings) {
            tracing::warn!("keymap: {w}");
        }
        if !app.svc.journal_ok {
            app.toast(ToastKind::Warn, Key::TJournalDown.tr(), 8);
        }
        if let Some(name) = cfg.select {
            app.remembered.insert(start_dir.clone(), name);
        }
        app.svc.places.load();
        app.svc.places.visit(start_dir.clone());
        app.svc.watcher.watch(start_dir.clone());
        app.request_dir(start_dir);
        app.svc.jobs.load_undo_label();
        // The daily check: the worker decides whether it is due, off this thread.
        if let Some(u) = &app.svc.updater {
            u.check_in_background();
        }
        if let Some(req) = cfg.request {
            let h = app.svc.jobs.plan_request(req);
            app.begin_plan(Key::PlanningGeneric.tr(), h);
        }
        app.maybe_offer_font(cfg.font_offer);
        app
    }

    /// The name to show for a folder or drive: the one the system's file manager shows, else
    /// `on_disk`.
    pub fn shown_name(&self, path: &Path, on_disk: String) -> String {
        self.paths
            .shown_name(path)
            .map(str::to_string)
            .unwrap_or(on_disk)
    }

    /// The refusal that covers `path`, if the system is holding it back.
    pub fn denied_for(&self, path: &Path) -> Option<&rada_core::platform::AccessDenied> {
        self.denied.iter().find(|d| path.starts_with(&d.folder))
    }

    /// "Now" for relative dates.
    pub fn now(&self) -> std::time::SystemTime {
        self.clock.unwrap_or_else(std::time::SystemTime::now)
    }

    pub fn home(&self) -> &Path {
        &self.svc.home
    }

    pub fn events(&self) -> crossbeam_channel::Receiver<CoreEvent> {
        self.svc.events.clone()
    }

    // ------------------------------------------------------------------ small helpers

    /// The time notifications go by: the clock, and what tests have moved it on.
    pub fn notice_now(&self) -> Instant {
        Instant::now() + self.notice_skew
    }

    /// Move the notifications' clock on (tests and screenshots: nobody waits six seconds).
    pub fn advance_notices(&mut self, by: Duration) {
        self.notice_skew += by;
        self.tick_notices();
    }

    fn tick_notices(&mut self) {
        let now = self.notice_now();
        if self.notices.tick(now) {
            self.dirty = true;
        }
    }

    /// A word about what happened, as a card. `secs` is how long a long message needs; a
    /// short one goes after about six seconds, an error stays until it is closed.
    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>, secs: u64) {
        let kind = match kind {
            ToastKind::Info => NoticeKind::Info,
            ToastKind::Ok => NoticeKind::Ok,
            ToastKind::Warn => NoticeKind::Warn,
            ToastKind::Error => NoticeKind::Error,
        };
        let ttl = Notice::TTL.max(Duration::from_secs(secs));
        self.notify(Notice::new(kind, text).lasting(ttl));
    }

    /// Queue a notification.
    pub fn notify(&mut self, notice: Notice) {
        let now = self.notice_now();
        self.notices.push(notice, now);
        self.dirty = true;
    }

    /// Take every notification away (tests: so that what comes next is not a left-over).
    pub fn clear_notices(&mut self) {
        self.notices = NoticeQueue::default();
        self.dirty = true;
    }

    /// The notification on screen.
    pub fn current_notice(&self) -> Option<&Notice> {
        self.notices.front()
    }

    pub fn tick(&mut self) {
        self.spinner = self.spinner.wrapping_add(1);
        self.tick_notices();
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
        self.date_cache.clear();
        let show = self.show_hidden;
        let terms: Vec<String> = self
            .filter
            .as_ref()
            .map(|f| f.text.split_whitespace().map(str::to_lowercase).collect())
            .unwrap_or_default();
        // `tag:red` keeps what carries a tag with that in its name (or of that colour); every
        // other word is part of the name.
        let (tag_terms, name_terms): (Vec<&String>, Vec<&String>) =
            terms.iter().partition(|t| t.starts_with("tag:"));
        let tag_terms: Vec<&str> = tag_terms.iter().map(|t| &t["tag:".len()..]).collect();
        self.visible = self
            .listing
            .all()
            .iter()
            .enumerate()
            .filter(|(_, e)| show || !e.hidden)
            .filter(|(_, e)| {
                name_terms.is_empty() || {
                    let name = e.display.to_lowercase();
                    name_terms.iter().all(|t| name.contains(t.as_str()))
                }
            })
            .filter(|(_, e)| tag_terms.iter().all(|want| matches_tag(e, want)))
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
        if self.view == ViewMode::Icons {
            // `scroll` is the first tile shown, always the start of a row.
            let cols = self.viewport.grid_cols.max(1);
            let rows = self.viewport.grid_rows.max(1);
            let cursor_row = self.cursor / cols;
            let mut first = self.scroll / cols;
            if cursor_row < first {
                first = cursor_row;
            } else if cursor_row >= first + rows {
                first = cursor_row + 1 - rows;
            }
            let last_row = self.visible.len().saturating_sub(1) / cols;
            first = first.min(last_row.saturating_sub(rows - 1));
            self.scroll = first * cols;
            return;
        }
        let rows = self.viewport.rows.max(1);
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
        let limits = Limits {
            show_hidden: self.show_hidden,
            ..self.limits.clone()
        };
        self.svc.previewer.request(path, self.preview_gen, limits);
    }

    // ------------------------------------------------------------------ navigation

    pub fn open_dir(&mut self, path: PathBuf) {
        if let Some(name) = self.cursor_name() {
            self.remembered.insert(self.cwd.clone(), name);
        }
        self.request_dir(path);
    }

    /// The tag whose items the list shows, when it is not a folder but the view of a tag.
    pub fn tag_view(&self) -> Option<String> {
        rada_core::tagsearch::view_name(&self.cwd)
    }

    /// This system keeps tags (everywhere but Windows).
    pub fn tags_supported(&self) -> bool {
        self.svc.platform.tags().is_some()
    }

    /// Remember the tags of the user's own that these items carry, for the sidebar.
    fn note_tags<'a>(&mut self, entries: impl IntoIterator<Item = &'a Entry>) {
        use rada_core::platform::TagColor as Hue;
        let before = self.known_tags.len();
        for t in entries.into_iter().flat_map(|e| e.tags.iter()) {
            if Hue::from_name(&t.name).is_none()
                && !self.known_tags.iter().any(|k| k.is_called(&t.name))
                && self.known_tags.len() < 60
            {
                self.known_tags.push(t.clone());
            }
        }
        if self.known_tags.len() != before {
            self.known_tags.sort_by_key(|t| t.name.to_lowercase());
        }
    }

    /// Say why nothing can be made here when the list is a tag's items, not a folder.
    pub(super) fn refuse_in_tag_view(&mut self, what: &str) -> bool {
        let Some(tag) = self.tag_view() else {
            return false;
        };
        self.toast(ToastKind::Warn, tf(Key::TTagViewRefuse, &[&tag, &what]), 5);
        true
    }

    /// The folder to use for something made next to the items: the one the first of them is
    /// in when the list is a tag's items, the folder shown otherwise.
    pub(super) fn folder_for_new(&self, items: &[PathBuf]) -> PathBuf {
        if self.tag_view().is_some() {
            if let Some(dir) = items.first().and_then(|p| p.parent()) {
                return dir.to_path_buf();
            }
            return self.svc.home.clone();
        }
        self.cwd.clone()
    }

    /// Back to the previous folder of the history (or forward to the next one).
    fn go_history(&mut self, back: bool) {
        let target = if back {
            self.nav.back_target()
        } else {
            self.nav.forward_target()
        };
        let Some(target) = target.map(Path::to_path_buf) else {
            return;
        };
        self.nav_move = Some(if back {
            NavMove::Back(target.clone())
        } else {
            NavMove::Forward(target.clone())
        });
        self.open_dir(target);
    }

    fn go_parent(&mut self) {
        // A tag's items have no parent: leave the way one came.
        if self.tag_view().is_some() {
            if self.nav.back_target().is_some() {
                self.go_history(true);
            } else {
                self.request_dir(self.svc.home.clone());
            }
            return;
        }
        let Some(parent) = self.cwd.parent().map(|p| p.to_path_buf()) else {
            self.toast(ToastKind::Info, Key::TAtTop.tr(), 2);
            return;
        };
        if let Some(name) = self.cwd.file_name() {
            self.remembered.insert(parent.clone(), name.to_os_string());
        }
        self.request_dir(parent);
    }

    fn enter(&mut self) {
        let Some(e) = self.current() else { return };
        if e.is_navigable_dir() {
            let target = e.path.clone();
            self.open_dir(target);
        } else if self.archive.is_some() {
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
        match self.current() {
            Some(e) if e.package => {
                let path = e.path.clone();
                self.open_dir(path);
            }
            _ => self.toast(ToastKind::Info, Key::TNotPackage.tr(), 2),
        }
    }

    /// The system's quick look at the item under the cursor (macOS).
    pub(super) fn quick_look(&mut self) {
        let Some(path) = self.current().map(|e| e.path.clone()) else {
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
        self.dirty = true;
        match ev {
            CoreEvent::Dir(d) => self.on_dir(d),
            CoreEvent::Watch(w) => self.on_watch(w),
            CoreEvent::Preview(p) => self.on_preview(p),
            CoreEvent::Volumes(v) => {
                self.volumes = if self.demo_volumes { demo_volumes() } else { v };
                self.folder_sizes.set_volumes(&self.volumes);
                self.refresh_palette();
            }
            CoreEvent::Paths(p) => {
                self.paths = p;
                self.refresh_palette();
            }
            CoreEvent::Job(j) => self.on_job(j),
            CoreEvent::Complete { dir, names } => self.on_complete(dir, names),
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
                if generation != self.dir_gen {
                    return; // an answer to a request that has been superseded
                }
                match result {
                    Ok(entries) => {
                        // It opened: whatever the system was holding back for this place, it
                        // is not any more.
                        self.denied.retain(|d| !path.starts_with(&d.folder));
                        self.note_tags(&entries);
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
                        let virtual_dir =
                            archive.is_some() || rada_core::tagsearch::view_name(&path).is_some();
                        self.archive = archive;
                        if !same_dir {
                            match self.nav_move.take() {
                                Some(NavMove::Back(to)) if to == path => self.nav.stepped_back(),
                                Some(NavMove::Forward(to)) if to == path => {
                                    self.nav.stepped_forward()
                                }
                                _ => self.nav.visit(path.clone()),
                            }
                            self.cwd = path.clone();
                            self.clear_marks();
                            self.cursor = 0;
                            self.scroll = 0;
                            // An archive's folders are not on the disk: nothing to watch, and
                            // not a place to come back to.
                            if !virtual_dir {
                                self.svc.places.visit(path.clone());
                                self.svc.watcher.watch(path);
                            }
                            self.previewed = None;
                        } else {
                            let existing: BTreeSet<OsString> =
                                self.listing.all().iter().map(|e| e.name.clone()).collect();
                            self.marked.retain(|n| existing.contains(n));
                        }
                        self.sync_folder_sizes();
                        self.rebuild_visible();
                        self.restore_cursor(keep);
                        self.load = LoadState::Ready;
                        self.request_preview();
                        self.run_demo();
                    }
                    Err(msg) => {
                        self.nav_move = None;
                        self.load = LoadState::Ready;
                        match denied {
                            Some(d) => self.on_access_denied(*d),
                            None => self.toast(ToastKind::Error, msg, 6),
                        }
                    }
                }
            }
            DirEvent::Patched { dir, updates, .. } => {
                if dir != self.cwd {
                    return;
                }
                let keep = self.cursor_name();
                self.note_tags(updates.iter().filter_map(|u| match u {
                    rada_core::model::EntryUpdate::Upsert(e) => Some(e),
                    _ => None,
                }));
                self.listing.apply(updates);
                self.sync_folder_sizes();
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
                self.forget_folder_sizes(&dir, &paths, rescan);
                // Going to another folder (or into an archive) is on its way: reading this one
                // again now would take the place of that request, and the person would be left
                // where they were. The folder that is opened is read as it is then.
                if matches!(&self.load, LoadState::Loading(target) if *target != dir) {
                    return;
                }
                self.dir_gen += 1;
                // A full read in progress is answered only for the newest request: a patch
                // would supersede it and leave the folder "loading" for good, so a change
                // that arrives meanwhile asks for another full read instead.
                if rescan || matches!(self.load, LoadState::Loading(_)) {
                    self.svc.loader.load(dir, self.dir_gen);
                } else {
                    self.svc.loader.patch(dir, paths, self.dir_gen);
                }
            }
            WatchEvent::Degraded(msg) => self.watch_note = Some(msg),
            WatchEvent::Failed(msg) => {
                self.watch_note = Some(msg.clone());
                self.toast(ToastKind::Warn, tf(Key::TLiveOff, &[&msg]), 6);
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
        self.preview.name = rada_core::display::name(&ev.name);
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
                            // Rounded corners, blended into the colour behind the picture
                            // (the theme's own background, or its canvas).
                            let bg = if self.basic {
                                None
                            } else {
                                crate::theme::rgb_of(self.th.bg.unwrap_or(self.th.canvas))
                            };
                            ui.show(img, bg);
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
                    text: ip.text,
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
                if self.plan_job == Some(job) || self.running.as_ref().is_some_and(|r| r.job == job)
                {
                    self.modal = None;
                    self.plan_job = None;
                    self.running = None;
                    // A sentence that does not fit a one-line message is read in a window.
                    if message.chars().count() > 70 {
                        self.modal = Some(Modal::Result(ResultView {
                            title: Key::ResultCouldNot.tr().to_string(),
                            lines: vec![(ToastKind::Error, message)],
                            scroll: 0,
                        }));
                    } else {
                        self.toast(ToastKind::Error, message, 6);
                    }
                }
            }
            JobEvent::UndoLabel { label, .. } => self.undo_label = label,
            JobEvent::NothingToUndo { job } => {
                if self.plan_job == Some(job) {
                    self.modal = None;
                    self.plan_job = None;
                    self.toast(ToastKind::Info, Key::TNothingUndo.tr(), 3);
                }
            }
            JobEvent::NothingToRedo { job } => {
                if self.plan_job == Some(job) {
                    self.modal = None;
                    self.plan_job = None;
                    self.toast(ToastKind::Info, Key::TNothingRedo.tr(), 4);
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
                self.modal = Some(Modal::Result(ResultView {
                    title: Key::ResultInterruptedTitle.tr().to_string(),
                    lines,
                    scroll: 0,
                }));
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
        use rada_core::ops::RunStatus::*;
        let mut lines: Vec<(ToastKind, String)> = Vec::new();
        for f in &report.failed {
            lines.push((ToastKind::Error, f.error.clone()));
        }
        if report.skipped_dependent > 0 {
            lines.push((
                ToastKind::Warn,
                tf(Key::NSkippedFolder, &[&report.skipped_dependent]),
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
            lines.push((ToastKind::Warn, tf(Key::NJournalErrors, &[&journal_errors])));
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
        if let Some(Modal::Plan(pv)) = &mut self.modal
            && pv.replanning == Some(job)
        {
            pv.plan = plan;
            pv.scan = scan.or(pv.scan.take());
            pv.replanning = None;
            pv.scroll = 0;
            return;
        }
        if self.plan_job != Some(job) {
            return; // the user cancelled while it was being planned
        }
        self.plan_job = None;
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
            self.toast(ToastKind::Warn, Key::TBusy.tr(), 3);
            true
        } else {
            false
        }
    }

    /// Archives are read-only: say so, with the way to do what was meant.
    pub(super) fn refuse_in_archive(&mut self, what: &str) -> bool {
        if self.archive.is_none() {
            return false;
        }
        self.toast(
            ToastKind::Warn,
            tf(
                Key::TArchiveReadOnly,
                &[
                    &what,
                    &self
                        .key_for(Action::Copy)
                        .unwrap_or_else(|| Key::KeyCopyWord.tr().into()),
                ],
            ),
            5,
        );
        true
    }

    fn start_paste(&mut self) {
        if self.refuse_if_busy()
            || self.refuse_in_archive(Key::WhatPasteHere.tr())
            || self.refuse_in_tag_view(Key::WhatPaste.tr())
        {
            return;
        }
        let Some(clip) = self.clipboard.clone() else {
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
            .plan_transfer(clip.paths, self.cwd.clone(), opts);
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
        let t = self.targets();
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
        let title = pv.plan.title.clone();
        let total = pv.plan.total_bytes();
        let (kind, totals, reversible) = (pv.plan.kind, pv.plan.totals, pv.plan.reversible);
        let outcome = self.outcome_of(&pv.plan);
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
            outcome,
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
                self.begin_plan(Key::PlanningRename.tr(), h);
            }
            InputKind::NewDir => {
                let h = self.svc.jobs.plan_mkdir(self.cwd.clone(), iv.text.clone());
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
                    self.modal = Some(Modal::Input(iv));
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
                let h = self.svc.jobs.plan_mkfile(self.cwd.clone(), iv.text.clone());
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
                    self.modal = Some(Modal::Input(iv));
                }
            },
            InputKind::OpenWith { path } => {
                let program = iv.text.trim().to_string();
                if program.is_empty() {
                    self.modal = Some(Modal::Input(iv));
                    return;
                }
                let (path, platform, notes) = (
                    path.clone(),
                    self.svc.platform.clone(),
                    self.svc.notes.clone(),
                );
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
        if mode == TransferMode::Move && self.refuse_in_archive(Key::WhatCut.tr()) {
            return;
        }
        let paths = self.targets();
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        self.clipboard = Some(Clip { mode, paths });
        self.clear_marks();
        let verb = if mode == TransferMode::Copy {
            Key::TYankCopied
        } else {
            Key::TYankCut
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
        let keep = self.cursor_name();
        self.sort = spec;
        self.listing.set_sort(spec); // immediate, in memory
        self.rebuild_visible();
        self.restore_cursor(keep);
        let arrow = if spec.reverse { "↓" } else { "↑" };
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
        if let Some(e) = self.current() {
            // The name on the disk (in a list of tagged items `name` is the whole path).
            let text = e
                .path
                .file_name()
                .unwrap_or(&e.name)
                .to_string_lossy()
                .into_owned();
            let from = e.path.clone();
            self.modal = Some(Modal::Input(InputView::new(
                InputKind::Rename { from },
                text,
            )));
        }
    }

    fn start_bulk_rename(&mut self) {
        if self.refuse_if_busy() || self.refuse_in_archive(Key::WhatRename.tr()) {
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
        // A mount point is listed once, as a disk (so "/" is not both "System" and a disk).
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
        for place in &self.paths.places {
            add(&mut items, &place.path, PaletteKind::Place, None, None);
        }
        add(
            &mut items,
            Path::new(std::path::MAIN_SEPARATOR_STR),
            PaletteKind::Place,
            Some(Key::DiskSystem.tr().into()),
            Some(std::path::MAIN_SEPARATOR_STR.into()),
        );
        for v in &self.volumes {
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
        if self.tag_view().is_none() {
            for anc in cwd.ancestors().skip(1).take(4) {
                add(&mut items, anc, PaletteKind::Parent, None, None);
            }
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
                self.begin_plan(Key::PlanningCopy.tr(), h);
            }
            "history" => {
                self.modal = Some(Modal::History(HistoryView {
                    entries: Vec::new(),
                    selected: 0,
                    loading: true,
                }));
                self.svc.jobs.load_history();
            }
            // A selection and something on the clipboard, to show the bottom bar at work.
            "selection" => {
                let files: Vec<_> = self
                    .listing
                    .all()
                    .iter()
                    .filter(|e| !e.is_dir())
                    .map(|e| (e.name.clone(), e.path.clone()))
                    .collect();
                self.marked = files.iter().take(3).map(|(n, _)| n.clone()).collect();
                self.clipboard = Some(Clip {
                    mode: TransferMode::Copy,
                    paths: files
                        .iter()
                        .skip(3)
                        .take(2)
                        .map(|(_, p)| p.clone())
                        .collect(),
                });
            }
            "main" => {}
            "help" => self.modal = Some(Modal::Help),
            "menu" => self.modal = Some(Modal::Menu(self.context_menu(true, (30, 11)))),
            _ => {}
        }
    }

    fn toggle_bookmark(&mut self) {
        if self.refuse_in_tag_view(Key::WhatBookmark.tr()) {
            return;
        }
        let cwd = self.cwd.clone();
        let add = !self.paths.bookmarks.contains(&cwd);
        self.set_bookmark(cwd, add);
    }

    /// Add a folder to the bookmarks, or take it out (nothing happens when it already is
    /// as asked).
    pub(super) fn set_bookmark(&mut self, path: PathBuf, add: bool) {
        if self.paths.bookmarks.contains(&path) == add {
            return;
        }
        self.svc.places.toggle_bookmark(path);
        self.toast(
            ToastKind::Ok,
            if add {
                Key::TBookmarked.tr()
            } else {
                Key::TBookmarkRemoved.tr()
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
                    if !pv.needs_typed_confirmation() && !matches!(pv.replan, Replan::None) =>
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
                KeyCode::Tab if matches!(iv.kind, InputKind::Compress { .. }) => {
                    iv.cycle_format();
                    self.modal = Some(Modal::Input(iv));
                }
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
                            self.toast(ToastKind::Info, Key::TCannotUndoThis.tr(), 4);
                            self.modal = Some(Modal::History(h));
                        }
                    }
                }
                _ => self.modal = Some(Modal::History(h)),
            },
            Modal::OpenWith(v) => self.on_open_with_key(v, key),
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
            Modal::FullPreview => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    self.preview.scroll += 1;
                    self.modal = Some(Modal::FullPreview);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.preview.scroll = self.preview.scroll.saturating_sub(1);
                    self.modal = Some(Modal::FullPreview);
                }
                KeyCode::PageDown => {
                    self.preview.scroll += 20;
                    self.modal = Some(Modal::FullPreview);
                }
                KeyCode::PageUp => {
                    self.preview.scroll = self.preview.scroll.saturating_sub(20);
                    self.modal = Some(Modal::FullPreview);
                }
                KeyCode::Char('H') => {
                    self.dispatch(Action::ToggleHex);
                    self.modal = Some(Modal::FullPreview);
                }
                KeyCode::Char('Q') => {
                    self.dispatch(Action::QuickLook);
                    self.modal = Some(Modal::FullPreview);
                }
                // Closing is deliberate: Esc, q, Space or Enter.
                KeyCode::Esc | KeyCode::Char('q' | ' ') | KeyCode::Enter => {}
                _ => self.modal = Some(Modal::FullPreview),
            },
            Modal::Update(view) => self.on_update_key(view, key),
            Modal::Access(d) => self.on_access_key(d, key),
            Modal::Eject(v) => self.on_eject_key(v, key),
            Modal::Font(v) => self.on_font_key(v, key),
            Modal::Properties(mut p) => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    p.scroll += 1;
                    self.modal = Some(Modal::Properties(p));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    p.scroll = p.scroll.saturating_sub(1);
                    self.modal = Some(Modal::Properties(p));
                }
                KeyCode::Esc | KeyCode::Char('q' | ' ' | 'i') | KeyCode::Enter => {}
                _ => self.modal = Some(Modal::Properties(p)),
            },
        }
    }

    // ------------------------------------------------------------------ dates for the list

    /// Work out the date text of these entries (indices into the listing), once; the list
    /// reads them with [`date_text`](Self::date_text).
    pub fn warm_dates(&mut self, indices: impl Iterator<Item = usize>) {
        let hour = self
            .now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() / 3600)
            .unwrap_or(0);
        let key = (hour, self.listing.all().len());
        if self.date_cache_key != key || self.date_cache.len() != key.1 {
            self.date_cache.clear();
            self.date_cache.resize(key.1, None);
            self.date_cache_key = key;
        }
        let now = self.now();
        for i in indices {
            if let Some(slot @ None) = self.date_cache.get_mut(i)
                && let Some(e) = self.listing.all().get(i)
            {
                *slot = Some(fmt::explorer_date_in(
                    e.mtime,
                    now,
                    self.dates,
                    self.date_format,
                    &self.tz,
                ));
            }
        }
    }

    /// The date text of an entry that [`warm_dates`](Self::warm_dates) has seen.
    pub fn date_text(&self, listing_index: usize) -> &str {
        self.date_cache
            .get(listing_index)
            .and_then(|s| s.as_deref())
            .unwrap_or("")
    }

    /// Take the text waiting for the system clipboard, if any.
    pub fn take_copied_text(&mut self) -> Option<String> {
        self.copied_text.take()
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
