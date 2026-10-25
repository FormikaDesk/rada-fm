//! Reading a folder: asking for it, taking in the answer of the worker, and the changes the watcher reports.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::PathBuf;

use rada_core::events::{CoreEvent, DirEvent, WatchEvent};
use rada_core::i18n::{Key, tf};
use rada_core::model::{DirListing, SortKey, SortSpec};

use super::{App, LoadState, ToastKind, columns, demo_volumes};
use crate::nav::NavMove;

impl App {
    /// The order of the list of recent files: the last used first, whatever the order of the
    /// folders is. It lasts as long as the list is open.
    const RECENT_SORT: SortSpec = SortSpec {
        key: SortKey::Modified,
        reverse: false,
        dirs_first: false,
        natural: false,
    };

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

    pub(super) fn request_dir(&mut self, path: PathBuf) {
        if path != self.tab.cwd {
            self.tab.filter = None;
            self.tab.sel.drop_anchor();
        }
        let generation = self.tabs.next_generation();
        self.tab.load = LoadState::Loading(path.clone());
        self.svc.loader.load(path, generation);
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
}
