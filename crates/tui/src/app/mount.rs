//! Mounting a stick or a phone that is plugged in and not open yet, from the sidebar: ask the
//! system on a thread of its own, say when it is done, and say in plain words what to do when it
//! cannot (a program to install, a phone to unlock).

use rada_core::platform::Unmounted;

use super::{App, ToastKind};
use rada_core::events::CoreEvent;
use rada_core::i18n::{Key, tf};
use std::path::{Path, PathBuf};

impl App {
    /// The id of the plugged-in device that `path` stands for in the sidebar, if any.
    pub fn unmounted_id(&self, path: &Path) -> Option<String> {
        self.unmounted
            .iter()
            .find(|u| Path::new(&u.id) == path)
            .map(|u| u.id.clone())
    }

    /// When the folder in view is inside a phone or a camera: the name the device goes by (the
    /// one the sidebar shows) and the folder that is the device, which stand for the long
    /// path of the virtual filesystem at the head of the path bar.
    pub fn device_head(&self) -> Option<(String, PathBuf)> {
        let root = self.svc.platform.device_root()?;
        let device = self.cwd.strip_prefix(root).ok()?.components().next()?;
        let dir = root.join(device);
        let label = self
            .volumes
            .iter()
            .find(|v| v.mount_point == dir)
            .and_then(|v| v.label.clone())?;
        Some((label, dir))
    }

    /// The devices to list: those plugged in, minus any that is already open (the two lists
    /// come from different threads and a moment apart).
    pub fn pending_devices(&self) -> Vec<&Unmounted> {
        self.unmounted
            .iter()
            .filter(|u| !self.volumes.iter().any(|v| v.device == u.id))
            .collect()
    }

    /// Mount the device `id`: a thread asks the system, an event comes back.
    pub fn start_mount(&mut self, id: &str) {
        let Some(device) = self.unmounted.iter().find(|u| u.id == id).cloned() else {
            return;
        };
        if !self.svc.platform.volumes().can_mount(&device) {
            self.toast(ToastKind::Info, Key::TCannotEject.tr(), 3);
            return;
        }
        if !self.mounting.insert(device.id.clone()) {
            return;
        }
        self.toast(ToastKind::Info, tf(Key::TMounting, &[&device.name]), 3);
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        std::thread::spawn(move || {
            let result = platform.volumes().mount(&device).map_err(|e| e.to_string());
            let _ = notes.send(CoreEvent::Mounted {
                id: device.id,
                name: device.name,
                result,
            });
        });
    }

    pub(super) fn on_mounted(&mut self, id: String, name: String, result: Result<(), String>) {
        // What was being mounted is free to be asked again, whichever way it ended.
        self.mounting.remove(&id);
        match result {
            Ok(()) => {
                self.toast(ToastKind::Ok, tf(Key::TMounted, &[&name]), 6);
                // The new folder is in the sidebar a moment from now, not at the next poll.
                self.svc.volumes.refresh_now();
            }
            Err(why) => self.toast(ToastKind::Error, format!("{name}: {why}"), 10),
        }
    }
}
