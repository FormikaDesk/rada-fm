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
}
