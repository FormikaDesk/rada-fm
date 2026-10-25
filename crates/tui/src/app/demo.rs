//! The scenes of --demo: the interface put in a ready-made state, once.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use rada_core::i18n::{Key, tf};
use rada_core::ops::{ConflictPolicy, TransferMode, TransferOptions};

use super::{App, Clip, HistoryView, MenuCmd, MenuKind, Modal, Notice, NoticeAction, NoticeKind};
use crate::fmt;
use crate::keymap::Action;

impl App {
    /// Developer hook (`--demo`): put the interface in a ready-made state, once.
    pub(super) fn run_demo(&mut self) {
        let Some(d) = self.hooks.demo.take() else {
            return;
        };
        let (scene, arg) = d
            .scene
            .split_once(':')
            .map_or((d.scene.as_str(), None), |(a, b)| (a, Some(b)));
        match scene {
            "palette" => self.demo_palette(arg),
            "plan" | "progress" => self.demo_plan(scene == "progress", d.dest),
            "history" => self.demo_history(),
            // The list of the files used lately (the demo home has a `recently-used.xbel`).
            "recenti" => self.open_dir(rada_core::recents::view_path()),
            "selection" => self.demo_selection(),
            "vetrina" => self.demo_showcase(),
            "main" => {}
            "help" => self.windows.modal = Some(Modal::Help),
            "menu" | "openwith" | "tagmenu" | "menubottom" => self.demo_menu(scene),
            "toasts" | "toasts-error" => self.demo_toasts(scene == "toasts-error", d.dest),
            "delete" | "conflict" => self.demo_delete(scene == "delete", d.dest),
            _ => {}
        }
    }

    fn demo_palette(&mut self, query: Option<&str>) {
        self.open_palette();
        if let (Some(Modal::Palette(p)), Some(q)) = (&mut self.windows.modal, query) {
            for c in q.chars() {
                p.insert(c);
            }
        }
    }

    /// Every entry shown marked and planned to be copied to `dest`; `progress` runs it at once.
    fn demo_plan(&mut self, progress: bool, dest: Option<PathBuf>) {
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
        let Some(dest) = dest else { return };
        self.hooks.demo_auto_run = progress;
        let opts = TransferOptions::copy(ConflictPolicy::Skip);
        let h = self.svc.jobs.plan_transfer(paths, dest, opts);
        self.begin_plan(Key::PlanningCopy.tr(), h);
    }

    fn demo_history(&mut self) {
        self.windows.modal = Some(Modal::History(HistoryView {
            entries: Vec::new(),
            selected: 0,
            loading: true,
        }));
        self.svc.jobs.load_history();
    }

    /// A selection and something on the clipboard, to show the bottom bar at work.
    fn demo_selection(&mut self) {
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

    /// The state the README's pictures show: an item selected, two waiting on the clipboard,
    /// the cursor on a picture, and a card saying the last copy went through.
    fn demo_showcase(&mut self) {
        let chosen: Vec<_> = ["backup.zip"]
            .iter()
            .filter_map(|n| self.entry_named(n))
            .collect();
        self.tab.sel.marked = chosen.iter().map(|(n, _)| n.clone()).collect();
        let waiting: Vec<PathBuf> = ["data.json", "main.rs"]
            .iter()
            .filter_map(|n| self.entry_named(n))
            .map(|(_, p)| p)
            .collect();
        if !waiting.is_empty() {
            self.ops.clipboard = Some(Clip {
                mode: TransferMode::Copy,
                paths: waiting,
            });
        }
        self.demo_cursor_on("photo.jpg");
        self.request_preview();
        let items = fmt::count(3, Key::ItemOne, Key::ItemMany);
        self.ops.undo_label = Some(tf(Key::DoneCopy, &[&items]));
        self.demo_copied_card();
    }

    /// The card that says three items were copied to Documents, with the way to undo it.
    fn demo_copied_card(&mut self) {
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

    /// The menu of the right button on the picture, and the menus beside it.
    /// "menubottom": the menu of a file among the last rows of the list.
    fn demo_menu(&mut self, scene: &str) {
        let (file, at) = if scene == "menubottom" {
            ("song.mp3", (30, 28))
        } else {
            ("photo.jpg", (24, 12))
        };
        self.demo_cursor_on(file);
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

    /// Two cards piled: a copy that is running (slowly: RADA_DEMO_SLOW_MBPS) over one that is
    /// done, or an error over one that is done.
    fn demo_toasts(&mut self, error: bool, dest: Option<PathBuf>) {
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
        if error {
            self.notify(Notice::new(NoticeKind::Error, Key::PvCannotImage.tr()).lasting(long));
            self.notify(done);
            return;
        }
        self.notify(done);
        let names = ["archive.tar.gz", "backup.zip"];
        self.tab.sel.marked = names.iter().map(|n| OsString::from(*n)).collect();
        let paths = self.paths_named(&names);
        let Some(dest) = dest else { return };
        self.hooks.demo_auto_run = true;
        let opts = TransferOptions::copy(ConflictPolicy::KeepBoth);
        let h = self.svc.jobs.plan_transfer(paths, dest, opts);
        self.begin_plan(Key::PlanningCopy.tr(), h);
    }

    /// The windows of a permanent deletion and of a copy that meets a file of the same name,
    /// on three marked items.
    fn demo_delete(&mut self, delete: bool, dest: Option<PathBuf>) {
        let names = ["archive.tar.gz", "backup.zip", "Cargo.toml"];
        self.tab.sel.marked = names.iter().map(|n| OsString::from(*n)).collect();
        self.demo_cursor_on("photo.jpg");
        if delete {
            self.dispatch(Action::DeletePermanently);
        } else if let Some(dest) = dest {
            let paths = self.paths_named(&names);
            let opts = TransferOptions::copy(ConflictPolicy::KeepBoth);
            let h = self.svc.jobs.plan_transfer(paths, dest, opts);
            self.begin_plan(Key::PlanningCopy.tr(), h);
        }
    }

    /// The name and the path of the entry shown as `name`.
    fn entry_named(&self, name: &str) -> Option<(OsString, PathBuf)> {
        self.tab
            .listing
            .all()
            .iter()
            .find(|e| e.display == name)
            .map(|e| (e.name.clone(), e.path.clone()))
    }

    /// The paths of the entries shown as `names`, the ones that exist.
    fn paths_named(&self, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .filter_map(|n| self.entry_named(n))
            .map(|(_, p)| p)
            .collect()
    }

    fn demo_cursor_on(&mut self, file: &str) {
        self.tab.restore_cursor(Some(OsString::from(file)));
        self.tab.ensure_visible();
    }
}
