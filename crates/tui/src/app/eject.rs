//! Ejecting a disk from the sidebar: ask, do it on a thread of its own, and if the disk is
//! busy say so (and, where the system tells, who is using it).

use rada_core::platform::Volume;

use super::{App, Modal, ToastKind};
use crossterm::event::{KeyCode, KeyEvent};
use rada_core::events::CoreEvent;
use rada_core::i18n::{Key, tf};
use std::path::{Path, PathBuf};

pub enum EjectStage {
    Confirm,
    Working,
    Failed(String),
}

pub struct EjectView {
    pub name: String,
    pub volume: Volume,
    pub stage: EjectStage,
}

impl App {
    /// The disk mounted at `mount`, if the system can eject it.
    pub fn ejectable(&self, mount: &Path) -> Option<&Volume> {
        self.volumes
            .iter()
            .find(|v| v.mount_point == mount)
            .filter(|v| self.svc.platform.volumes().can_eject(v))
    }

    /// Ask whether to eject the disk at `mount`.
    pub fn start_eject(&mut self, mount: &Path) {
        let Some(volume) = self.ejectable(mount).cloned() else {
            self.toast(ToastKind::Info, Key::TCannotEject.tr(), 3);
            return;
        };
        let name = crate::sidebar::device_name(&volume);
        let name = self.shown_name(&volume.mount_point, name);
        self.modal = Some(Modal::Eject(EjectView {
            name,
            volume,
            stage: EjectStage::Confirm,
        }));
    }

    pub(super) fn on_eject_key(&mut self, mut view: EjectView, key: KeyEvent) {
        match (&view.stage, key.code) {
            (EjectStage::Working, KeyCode::Esc) => return,
            (EjectStage::Working, _) => {}
            (_, KeyCode::Esc | KeyCode::Char('q')) => return,
            (_, KeyCode::Enter | KeyCode::Char('y')) => {
                // The disk the folder in view is on cannot be ejected from under it.
                if self.cwd.starts_with(&view.volume.mount_point) {
                    let home = self.svc.home.clone();
                    self.open_dir(home);
                }
                view.stage = EjectStage::Working;
                let platform = self.svc.platform.clone();
                let notes = self.svc.notes.clone();
                let volume = view.volume.clone();
                let name = view.name.clone();
                std::thread::spawn(move || {
                    let result = platform.volumes().eject(&volume).map_err(|e| e.to_string());
                    let _ = notes.send(CoreEvent::Ejected {
                        mount: volume.mount_point,
                        name,
                        result,
                    });
                });
            }
            _ => {}
        }
        self.modal = Some(Modal::Eject(view));
    }

    pub(super) fn on_ejected(&mut self, mount: PathBuf, name: String, result: Result<(), String>) {
        let waiting = matches!(
            &self.modal,
            Some(Modal::Eject(v)) if v.volume.mount_point == mount
        );
        match result {
            Ok(()) => {
                if waiting {
                    self.modal = None;
                }
                self.toast(ToastKind::Ok, tf(Key::TEjected, &[&name]), 6);
                // A phone going away is no change of mounts: look again now, not in five seconds.
                self.svc.volumes.refresh_now();
            }
            Err(why) => {
                if let Some(Modal::Eject(v)) = &mut self.modal
                    && v.volume.mount_point == mount
                {
                    v.stage = EjectStage::Failed(why);
                } else if let Some(volume) = self.volumes.iter().find(|v| v.mount_point == mount) {
                    self.modal = Some(Modal::Eject(EjectView {
                        name,
                        volume: volume.clone(),
                        stage: EjectStage::Failed(why),
                    }));
                }
            }
        }
    }
}
