//! Application state and behaviour. Pure logic: no drawing, no I/O. Everything that
//! touches the disk is a request to a worker; everything the workers say arrives as an
//! event and changes state here.

use rada_core::i18n::Key;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use crossterm::event::KeyEvent;
use rada_core::events::{CoreEvent, FailureInfo, JobId};
use rada_core::model::SortSpec;
use rada_core::ops::{Cancel, ErrorChoice};
use rada_core::preview::Limits;

use crate::fmt::{DateFormat, DateStyle};
use crate::icons::IconSet;
use crate::images::{ImageMode, ImageUi};
use crate::keymap::Keymap;
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

mod actions;
mod address;
mod archives;
mod bar;
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
mod menuview;
mod mount;
mod mouse;
mod navigate;
mod navrows;
mod notice;
mod openwith;
mod ops;
mod outcome;
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
pub use demo::Demo;
pub use eject::{EjectStage, EjectView};
pub use font::{FontStage, FontView};
pub use input::FilterState;
pub use menus::MenuKind;
pub use menuview::{MenuCmd, MenuIcon, MenuItem, MenuTitle, MenuView, TagCmd};
pub use notice::{MAX_SHOWN, Notice, NoticeAction, NoticeKind, NoticeQueue};
pub use openwith::{OpenRow, OpenWithView};
pub use ops::{
    Clip, HistoryView, Operations, PlanView, Replan, ResultView, Running, is_confirmation,
};
pub use panels::GitView;
pub use preview::{ImageStatus, ImageView, PreviewPane};
pub use prompt::{InputKind, InputView};
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

    // ------------------------------------------------------------------ core events

    pub fn on_core_event(&mut self, ev: CoreEvent) {
        self.redraw.dirty = true;
        match ev {
            CoreEvent::Dir(d) => self.on_dir(d),
            CoreEvent::Watch(w) => self.on_watch(w),
            CoreEvent::Preview(p) => self.on_preview(p),
            CoreEvent::Volumes(v) => self.on_volumes(v),
            CoreEvent::Unmounted(list) => self.on_unmounted(list),
            CoreEvent::Mounted { id, name, result } => self.on_mounted(id, name, result),
            CoreEvent::Paths(p) => self.on_paths(p),
            CoreEvent::Job(j) => self.on_job(j),
            CoreEvent::Complete { dir, names } => self.on_complete(dir, names),
            CoreEvent::Git { dir, status } => self.on_git(dir, status),
            CoreEvent::Tree { dir, hidden, kids } => self.on_tree(dir, hidden, kids),
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
            CoreEvent::Note { text, error } => self.on_note(text, error),
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
}
