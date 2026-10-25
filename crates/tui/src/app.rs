//! Application state and behaviour. Pure logic: no drawing, no I/O. Everything that
//! touches the disk is a request to a worker; everything the workers say arrives as an
//! event and changes state here.

use rada_core::i18n::{Key, tf};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::archive::ArchiveKind;
use rada_core::events::{CoreEvent, FailureInfo, JobId};
use rada_core::journal::JournalEntry;
use rada_core::model::SortSpec;
use rada_core::ops::{
    Cancel, ConflictPolicy, ErrorChoice, Pattern, Plan, Scan, TransferMode, TransferOptions,
    UndoPlan,
};
use rada_core::ops::{ExtractInto, OpKind, OpRequest, WarningKind};
use rada_core::platform::Volume;
use rada_core::preview::Limits;

use crate::fmt::{self, DateFormat, DateStyle};
use crate::icons::IconSet;
use crate::images::{ImageMode, ImageUi};
use crate::keymap::{Action, Keymap};
use crate::palette::PaletteView;
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
mod demo;
mod eject;
mod font;
mod input;
mod jobs;
mod listing;
mod load;
mod menus;
mod mount;
mod navigate;
mod notice;
mod openwith;
mod ops;
mod palette;
mod panels;
mod preview;
mod prompt;
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

    // ------------------------------------------------------------------ starting operations

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

    // ------------------------------------------------------------------ dates for the list
}
