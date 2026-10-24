//! Shared harness: the interface driven headlessly with real workers on a sandbox.
#![allow(dead_code)]

mod watchdog;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use rada_core::fs::LocalFs;
use rada_core::model::SortSpec;
use rada_core::testutil::*;
use rada_tui::app::{App, Config, Modal};
use rada_tui::hits::Target;
use rada_tui::keymap::Chord;
use rada_tui::{IconSet, Services, Theme, ui};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Adjusts the configuration before the interface starts.
pub type Tweak = Box<dyn FnOnce(&mut Config)>;

/// A filesystem and a platform other than the sandbox's own.
pub type System = (
    Arc<dyn rada_core::fs::FsEngine>,
    Arc<dyn rada_core::platform::Platform>,
);

pub struct H {
    _teardown: watchdog::Teardown,
    pub app: App,
    pub term: Terminal<TestBackend>,
    _sb: Sandbox,
    _watch: watchdog::Watch,
}

impl H {
    /// What the program asked the system to open (the sandbox's opener starts nothing).
    pub fn opened(&self) -> Vec<String> {
        self._sb.opened()
    }

    pub fn new(sb: Sandbox, start: PathBuf, w: u16, h: u16) -> H {
        H::with_images(
            sb,
            start,
            w,
            h,
            Some(rada_tui::ImageUi::halfblocks()),
            Default::default(),
        )
    }

    pub fn with_images(
        sb: Sandbox,
        start: PathBuf,
        w: u16,
        h: u16,
        images: Option<rada_tui::ImageUi>,
        limits: rada_core::preview::Limits,
    ) -> H {
        H::build(sb, start, w, h, images, limits, None)
    }

    /// An operation handed in as data, as `rada --request` does.
    pub fn with_request(
        sb: Sandbox,
        start: PathBuf,
        w: u16,
        h: u16,
        req: rada_core::ops::OpRequest,
    ) -> H {
        H::build(
            sb,
            start,
            w,
            h,
            Some(rada_tui::ImageUi::halfblocks()),
            Default::default(),
            Some(req),
        )
    }

    /// With the user's limits for extracting archives.
    pub fn with_archive_limits(
        sb: Sandbox,
        start: PathBuf,
        w: u16,
        h: u16,
        limits: rada_core::archive::ArchiveLimits,
    ) -> H {
        H::build_with(
            sb,
            start,
            w,
            h,
            Some(rada_tui::ImageUi::halfblocks()),
            Default::default(),
            None,
            limits,
            None,
            None,
        )
    }

    /// On a system other than the sandbox's own (a stand-in for macOS, a filesystem that
    /// refuses), with the configuration adjusted by `tweak`.
    pub fn with_system(
        sb: Sandbox,
        start: PathBuf,
        w: u16,
        h: u16,
        system: System,
        tweak: impl FnOnce(&mut Config) + 'static,
    ) -> H {
        H::build_with(
            sb,
            start,
            w,
            h,
            Some(rada_tui::ImageUi::halfblocks()),
            Default::default(),
            None,
            Default::default(),
            Some(Box::new(tweak)),
            Some(system),
        )
    }

    /// With the configuration adjusted by `tweak` before the interface starts.
    pub fn with_config(
        sb: Sandbox,
        start: PathBuf,
        w: u16,
        h: u16,
        tweak: impl FnOnce(&mut Config) + 'static,
    ) -> H {
        H::build_with(
            sb,
            start,
            w,
            h,
            Some(rada_tui::ImageUi::halfblocks()),
            Default::default(),
            None,
            Default::default(),
            Some(Box::new(tweak)),
            None,
        )
    }

    fn build(
        sb: Sandbox,
        start: PathBuf,
        w: u16,
        h: u16,
        images: Option<rada_tui::ImageUi>,
        limits: rada_core::preview::Limits,
        request: Option<rada_core::ops::OpRequest>,
    ) -> H {
        H::build_with(
            sb,
            start,
            w,
            h,
            images,
            limits,
            request,
            Default::default(),
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_with(
        sb: Sandbox,
        start: PathBuf,
        w: u16,
        h: u16,
        images: Option<rada_tui::ImageUi>,
        limits: rada_core::preview::Limits,
        request: Option<rada_core::ops::OpRequest>,
        archive_limits: rada_core::archive::ArchiveLimits,
        tweak: Option<Tweak>,
        system: Option<System>,
    ) -> H {
        let (fs, platform) = system.unwrap_or_else(|| (Arc::new(LocalFs), sb.platform()));
        let svc = Services::start_with(fs, platform, Some(sb.journal()), archive_limits);
        let mut cfg = Config {
            start_dir: start,
            icons: IconSet::UnicodeIcons,
            show_hidden: false,
            sort: SortSpec::default(),
            theme: Theme::rada(),
            adaptive: None,
            image_mode: rada_tui::ImageMode::Halfblocks,
            limits,
            select: None,
            bookmarks: Vec::new(),
            demo: None,
            keymap: Default::default(),
            cmd_keys: false,
            font_offer: None,
            request,
            mouse: true,
            show_hints: true,
            sidebar: true,
            layout: rada_tui::LayoutKind::Explorer,
            // The older tests draw the classic layout at every size; the ones of the panels
            // ask for them.
            layout_pref: rada_tui::LayoutPref::Classic,
            // Previews stay visible at the widths the older tests use.
            details_pane: Some(true),
            view: rada_tui::ViewMode::Details,
            remember_tabs: true,
            saved_ui: Default::default(),
            start_explicit: false,
            dates: rada_tui::fmt::DateStyle::Relative,
            date_format: rada_tui::fmt::DateFormat::DEFAULT,
            sidebar_density: Default::default(),
            nav_mode: Default::default(),
            basic: false,
            tags_column: None,
            chooser: None,
        };
        if let Some(t) = tweak {
            t(&mut cfg);
        }
        let mut app = App::new(cfg, svc, images);
        // The version is in the status bar: pinned, a release does not change every snapshot.
        app.updates.version = "0.6.0".into();
        // A double click is two clicks within a time. The tests do not depend on how fast a
        // machine is: any two clicks of a test are a double click unless the test says
        // otherwise (see `a_slow_second_click_is_not_a_double_click`).
        app.settings.double_click = Duration::from_secs(600);
        let mut h = H {
            _teardown: watchdog::Teardown,
            app,
            term: Terminal::new(TestBackend::new(w, h)).unwrap(),
            _sb: sb,
            _watch: watchdog::Watch::start(),
        };
        h.settle();
        h
    }

    /// Process worker events until `done` holds (or fail after a few seconds).
    pub fn wait(&mut self, what: &str, mut done: impl FnMut(&App) -> bool) {
        watchdog::call(format!("waiting for: {what}"));
        let rx = self.app.events();
        let resized = self.app.preview.image_ui.as_ref().map(|u| u.results());
        // Generous: CI machines are slow, and a test that waits is waiting for a worker thread.
        let end = Instant::now() + Duration::from_secs(20);
        while !done(&self.app) {
            assert!(
                Instant::now() < end,
                "timed out waiting for: {what}\n{}",
                self.screen()
            );
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(20)) {
                watchdog::step("handling an event of a worker");
                self.app.on_core_event(ev);
            }
            // The picture is resized and encoded by its own thread once it has been drawn.
            if let Some(r) = &resized {
                while let Ok(done) = r.try_recv() {
                    self.app.on_image_resized(done);
                }
            }
            watchdog::step("tick");
            self.app.tick();
            let _ = self.screen(); // drawing is what asks for the resize
        }
    }

    /// Like `wait`, with `make` run again every so often until `done` holds: what is made
    /// before the watcher is in place is missed by it, and nothing says when that is over.
    pub fn wait_making(&mut self, what: &str, make: impl Fn(), mut done: impl FnMut(&App) -> bool) {
        watchdog::call(format!("waiting for: {what}"));
        let mut made = None::<Instant>;
        self.wait(what, |a| {
            if made.is_none_or(|t| t.elapsed() > Duration::from_millis(400)) {
                make();
                made = Some(Instant::now());
            }
            done(a)
        });
    }

    /// Let workers and the encoder thread deliver for a moment, drawing as the real loop does.
    pub fn pump(&mut self, ms: u64) {
        watchdog::call(format!("pumping events for {ms} ms"));
        let rx = self.app.events();
        let resized = self.app.preview.image_ui.as_ref().map(|u| u.results());
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(10)) {
                watchdog::step("handling an event of a worker");
                self.app.on_core_event(ev);
            }
            if let Some(r) = &resized {
                while let Ok(done) = r.try_recv() {
                    self.app.on_image_resized(done);
                }
            }
            let _ = self.screen();
        }
    }

    pub fn settle(&mut self) {
        self.wait("initial load", |a| !a.is_loading());
    }

    pub fn key(&mut self, c: KeyCode) {
        watchdog::call(format!("key {c:?}"));
        self.app.on_key(KeyEvent::new(c, KeyModifiers::NONE));
    }

    pub fn keys(&mut self, s: &str) {
        for c in s.chars() {
            self.key(KeyCode::Char(c));
        }
    }

    /// Press a key written like in the configuration: `ctrl+c`, `shift+delete`, `F2`, `G`.
    pub fn press(&mut self, spec: &str) {
        let c = Chord::parse(spec).unwrap_or_else(|e| panic!("{e}"));
        watchdog::call(format!("key {spec}"));
        self.app.on_key(KeyEvent::new(c.code, c.mods));
    }

    pub fn mouse(&mut self, kind: MouseEventKind, x: u16, y: u16, mods: KeyModifiers) {
        watchdog::call(format!("mouse {kind:?} at {x},{y}"));
        self.app.on_mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: mods,
        });
    }

    /// Centre of what the last frame registered for `target`.
    pub fn where_is(&mut self, target: &Target) -> (u16, u16) {
        let _ = self.screen();
        let r = self
            .app
            .frame
            .hits
            .find(target)
            .unwrap_or_else(|| panic!("{target:?} is not on screen:\n{}", self.screen()));
        (r.x + r.width / 2, r.y)
    }

    pub fn click_on(&mut self, target: &Target, mods: KeyModifiers) {
        let (x, y) = self.where_is(target);
        self.mouse(MouseEventKind::Down(MouseButton::Left), x, y, mods);
    }

    pub fn click(&mut self, target: &Target) {
        self.click_on(target, KeyModifiers::NONE);
    }

    /// Two clicks on `target`, one right after the other: its place is looked up once, and
    /// nothing is drawn in between.
    pub fn double_click(&mut self, target: &Target) {
        let (x, y) = self.where_is(target);
        for _ in 0..2 {
            self.mouse(
                MouseEventKind::Down(MouseButton::Left),
                x,
                y,
                KeyModifiers::NONE,
            );
        }
    }

    pub fn right_click(&mut self, target: &Target) {
        let (x, y) = self.where_is(target);
        self.mouse(
            MouseEventKind::Down(MouseButton::Right),
            x,
            y,
            KeyModifiers::NONE,
        );
    }

    pub fn marked_names(&self) -> Vec<String> {
        self.app
            .marked
            .iter()
            .map(|n| n.to_string_lossy().into_owned())
            .collect()
    }

    pub fn cursor_name(&self) -> String {
        self.app
            .current()
            .map(|e| e.display.clone())
            .unwrap_or_default()
    }

    /// Wait until the folder shown lists `name`, then put the cursor on it with the Down key,
    /// at most once per entry: an empty list would never get there.
    pub fn cursor_to(&mut self, name: &str) {
        self.wait(&format!("{name} in the folder"), |a| {
            (0..a.visible.len()).any(|i| a.entry_at(i).is_some_and(|e| e.display == name))
        });
        for _ in 0..self.app.visible.len() {
            if self.cursor_name() == name {
                return;
            }
            self.key(KeyCode::Down);
        }
        let on = self.cursor_name();
        assert_eq!(
            on,
            name,
            "the cursor never reached {name}\n{}",
            self.screen()
        );
    }

    pub fn screen(&mut self) -> String {
        watchdog::step("drawing a frame");
        self.term.draw(|f| ui::draw(f, &mut self.app)).unwrap();
        let buf = self.term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            // A wide glyph (CJK, emoji) covers two cells; the second one is not text.
            let mut skip = 0;
            for x in 0..buf.area.width {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let sym = buf[(x, y)].symbol();
                skip = unicode_width::UnicodeWidthStr::width(sym).saturating_sub(1);
                out.push_str(sym);
            }
            out.push('\n');
        }
        out
    }

    /// The buffer of a frame drawn from nothing: it depends on the state of the interface and
    /// on nothing else. `screen` and the buffer of the backend keep what earlier frames drew
    /// in the second cell of a wide glyph (the terminal skips it), and which frames came
    /// before depends on when the workers answered; clearing first makes the colours of those
    /// cells the same on every run.
    pub fn redrawn_buffer(&mut self) -> ratatui::buffer::Buffer {
        self.term.clear().unwrap();
        self.term.draw(|f| ui::draw(f, &mut self.app)).unwrap();
        self.term.backend().buffer().clone()
    }

    pub fn names(&self) -> Vec<String> {
        (0..self.app.visible.len())
            .filter_map(|i| self.app.entry_at(i))
            .map(|e| e.display.clone())
            .collect()
    }

    pub fn modal_is_plan(&self) -> bool {
        matches!(self.app.windows.modal, Some(Modal::Plan(_)))
    }
}

pub fn sandbox_with_files() -> (Sandbox, PathBuf) {
    let sb = Sandbox::new();
    sb.write("proj.v1/b.txt", "bee\nsecond line");
    sb.write("proj.v1/a.txt", "ay");
    sb.write("proj.v1/.hidden", "h");
    sb.hide("proj.v1/.hidden");
    sb.write("proj.v1/sub/inner.txt", "inner");
    let dir = sb.path("proj.v1");
    (sb, dir)
}

pub fn run_plan_and_wait(h: &mut H) {
    assert!(h.modal_is_plan(), "{}", h.screen());
    h.key(KeyCode::Enter);
    h.wait("operation to finish", |a| {
        a.ops.running.is_none() && !matches!(a.windows.modal, Some(Modal::Scanning { .. }))
    });
}

pub fn plan_modal(h: &mut H) {
    h.wait("a plan window", |a| {
        matches!(a.windows.modal, Some(Modal::Plan(_)))
    });
}

// ---------------------------------------------------------------------------- the explorer scene

use std::time::SystemTime;

/// 2026-10-10 14:30 UTC: the clock of every explorer screenshot.
pub fn demo_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_642_600)
}

fn set_age(path: &std::path::Path, secs: u64) {
    let t = demo_now() - Duration::from_secs(secs);
    set_mtime(path, t);
}

/// `~/projects/demo` as in the prototype: folders, archives, code, a picture and a movie,
/// with the standard places and three disks, and a clock that never moves.
pub fn demo_scene(w: u16, h: u16) -> H {
    let sb = Sandbox::new();
    let home = sb.dirs.home.clone();
    for d in [
        "Desktop",
        "Downloads",
        "Documents",
        "Pictures",
        "Music",
        "Videos",
    ] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    let dir = home.join("projects/demo");
    let day = 86_400u64;
    let put = |name: &str, size: usize, secs: u64| {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "x".repeat(size.min(4096))).unwrap();
        if size > 4096 {
            let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
            f.set_len(size as u64).unwrap();
        }
        set_age(&p, secs);
    };
    for (d, secs) in [
        ("assets", 3 * day),
        ("docs", day + 3600),
        ("scripts", 9 * day),
        ("src", 5 * 3600),
        ("tests", 2 * day),
    ] {
        let p = dir.join(d);
        std::fs::create_dir_all(&p).unwrap();
        set_age(&p, secs);
    }
    put("archive.tar.gz", 18_454_938, 8 * day);
    put("backup.zip", 40_894_464, 97 * day);
    put("Cargo.toml", 82, 3 * day);
    put("data.json", 49, 6 * day);
    put("demo.mp4", 95_420_416, 14 * day);
    put("main.rs", 80, 20 * 60);
    put("logo.svg", 197, 5 * day);
    put("notes.txt", 28, day + 7200);
    put("photo.jpg", 72_397, 3600);
    put("README.md", 145, 3700);
    put("report.pdf", 2_400_000, 4 * day);
    put("screenshot.png", 5_472, day + 5000);
    put("setup.sh", 21, 12 * day);
    put("song.mp3", 7_800_000, 72 * day);
    let mut h = H::new(sb, dir, w, h);
    h.wait("the places", |a| !a.places.paths.places.is_empty());
    h.app.hooks.clock = Some(demo_now());
    h.app.settings.details = None;
    h.app.settings.tz = jiff::tz::TimeZone::UTC;
    // Creation times are the real ones of the sandbox: leave them out of the screenshots.
    let entries: Vec<rada_core::model::Entry> = h.app.listing.all().to_vec();
    h.app.listing.apply(
        entries
            .into_iter()
            .map(|mut e| {
                e.created = None;
                rada_core::model::EntryUpdate::Upsert(e)
            })
            .collect(),
    );
    h.app.devices.volumes = {
        use rada_core::platform::{Volume, VolumeKind};
        let gb = 1_000_000_000u64;
        let system_root = h
            .app
            .cwd
            .ancestors()
            .last()
            .unwrap_or(Path::new("/"))
            .to_string_lossy()
            .into_owned();
        let v = |mount: &str, label: &str, kind, total: u64, free: u64| Volume {
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
            // The root of whatever disk the sandbox is on: "/" here, "D:\\" on Windows.
            v(&system_root, "System", VolumeKind::Fixed, 512, 197),
            v("/mnt/data", "Data", VolumeKind::Fixed, 1000, 596),
            v("/mnt/usb", "USB drive", VolumeKind::Removable, 64, 38),
        ]
    };
    h
}

/// The interface stays responsive while a worker is busy for seconds: nearly every key and
/// frame is quick, and none waits for the worker. A single slow sample is a machine that
/// stalled for a moment (a busy runner), not a keypress that waited for the work, which would
/// take as long as the work itself.
pub fn assert_never_blocked(mut samples: Vec<Duration>, what: &str) {
    assert!(!samples.is_empty());
    samples.sort();
    let worst = *samples.last().unwrap();
    let typical = samples[(samples.len() * 9 / 10).min(samples.len() - 1)];
    assert!(
        typical < Duration::from_millis(100),
        "{what}: nine in ten took up to {typical:?} (the slowest {worst:?})"
    );
    assert!(
        worst < Duration::from_millis(1500),
        "{what}: one took {worst:?}, as if it waited for the worker"
    );
}
