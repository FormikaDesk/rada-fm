//! Shared harness: the interface driven headlessly with real workers on a sandbox.
#![allow(dead_code)]

use std::path::PathBuf;
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

pub struct H {
    pub app: App,
    pub term: Terminal<TestBackend>,
    _sb: Sandbox,
}

impl H {
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

    fn build(
        sb: Sandbox,
        start: PathBuf,
        w: u16,
        h: u16,
        images: Option<rada_tui::ImageUi>,
        limits: rada_core::preview::Limits,
        request: Option<rada_core::ops::OpRequest>,
    ) -> H {
        let svc = Services::start(Arc::new(LocalFs), sb.platform(), Some(sb.journal()));
        let cfg = Config {
            start_dir: start,
            icons: IconSet::Unicode,
            show_hidden: false,
            sort: SortSpec::default(),
            theme: Theme::rada(),
            image_mode: rada_tui::ImageMode::Halfblocks,
            limits,
            select: None,
            bookmarks: Vec::new(),
            demo: None,
            keymap: Default::default(),
            request,
            mouse: true,
        };
        let app = App::new(cfg, svc, images);
        let mut h = H {
            app,
            term: Terminal::new(TestBackend::new(w, h)).unwrap(),
            _sb: sb,
        };
        h.settle();
        h
    }

    /// Process worker events until `done` holds (or fail after a few seconds).
    pub fn wait(&mut self, what: &str, mut done: impl FnMut(&App) -> bool) {
        let rx = self.app.events();
        let resized = self.app.image_ui.as_ref().map(|u| u.results());
        let end = Instant::now() + Duration::from_secs(8);
        while !done(&self.app) {
            assert!(
                Instant::now() < end,
                "timed out waiting for: {what}\n{}",
                self.screen()
            );
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(20)) {
                self.app.on_core_event(ev);
            }
            // The picture is resized and encoded by its own thread once it has been drawn.
            if let Some(r) = &resized {
                while let Ok(done) = r.try_recv() {
                    self.app.on_image_resized(done);
                }
            }
            self.app.tick();
            let _ = self.screen(); // drawing is what asks for the resize
        }
    }

    /// Let workers and the encoder thread deliver for a moment, drawing as the real loop does.
    pub fn pump(&mut self, ms: u64) {
        let rx = self.app.events();
        let resized = self.app.image_ui.as_ref().map(|u| u.results());
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(10)) {
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
        self.app.on_key(KeyEvent::new(c.code, c.mods));
    }

    pub fn mouse(&mut self, kind: MouseEventKind, x: u16, y: u16, mods: KeyModifiers) {
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

    pub fn screen(&mut self) -> String {
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

    pub fn names(&self) -> Vec<String> {
        (0..self.app.visible.len())
            .filter_map(|i| self.app.entry_at(i))
            .map(|e| e.display.clone())
            .collect()
    }

    pub fn modal_is_plan(&self) -> bool {
        matches!(self.app.modal, Some(Modal::Plan(_)))
    }
}

pub fn sandbox_with_files() -> (Sandbox, PathBuf) {
    let sb = Sandbox::new();
    sb.write("proj.v1/b.txt", "bee\nsecond line");
    sb.write("proj.v1/a.txt", "ay");
    sb.write("proj.v1/.hidden", "h");
    sb.write("proj.v1/sub/inner.txt", "inner");
    let dir = sb.path("proj.v1");
    (sb, dir)
}

pub fn run_plan_and_wait(h: &mut H) {
    assert!(h.modal_is_plan(), "{}", h.screen());
    h.key(KeyCode::Enter);
    h.wait("operation to finish", |a| {
        a.running.is_none() && !matches!(a.modal, Some(Modal::Scanning { .. }))
    });
}

pub fn plan_modal(h: &mut H) {
    h.wait("a plan window", |a| matches!(a.modal, Some(Modal::Plan(_))));
}
