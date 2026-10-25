//! Mounting a stick or a phone that is plugged in and not open yet, from the sidebar: ask the
//! system on a thread of its own, say when it is done, and say in plain words what to do when it
//! cannot (a program to install, a phone to unlock).

use rada_core::platform::{AccessDenied, Unmounted, Volume};

use super::{App, ToastKind, demo_volumes};
use rada_core::events::CoreEvent;
use rada_core::i18n::{Key, tf};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The disks, sticks and phones the system lists, and the folders it holds back.
#[derive(Default)]
pub struct Devices {
    pub volumes: Vec<Volume>,
    /// Sticks and phones that are plugged in and not mounted: the sidebar offers to mount them.
    pub unmounted: Vec<Unmounted>,
    /// What is being mounted right now (by id), so that a second Enter does not start it again.
    pub(super) mounting: HashSet<String>,
    /// The list of volumes has arrived from the worker at least once (tests wait for it so
    /// that a late answer does not replace the list they set).
    pub volumes_seen: bool,
    /// Folders the system is waiting for the person to allow (macOS), as learnt from the folders
    /// that could not be opened.
    pub denied: Vec<AccessDenied>,
}

impl Devices {
    /// The id of the plugged-in device that `path` stands for in the sidebar, if any.
    pub fn unmounted_id(&self, path: &Path) -> Option<String> {
        self.unmounted
            .iter()
            .find(|u| Path::new(&u.id) == path)
            .map(|u| u.id.clone())
    }

    /// The devices to list: those plugged in, minus any that is already open (the two lists
    /// come from different threads and a moment apart).
    pub fn pending(&self) -> Vec<&Unmounted> {
        self.unmounted
            .iter()
            .filter(|u| !self.volumes.iter().any(|v| v.device == u.id))
            .collect()
    }

    /// The refusal that covers `path`, if the system is holding it back.
    pub fn denied_for(&self, path: &Path) -> Option<&AccessDenied> {
        self.denied.iter().find(|d| path.starts_with(&d.folder))
    }

    /// Remember a refusal, in the place of any earlier one for the same folder.
    pub(super) fn deny(&mut self, d: AccessDenied) {
        self.denied.retain(|x| x.folder != d.folder);
        self.denied.push(d);
    }

    /// A place opened: whatever the system was holding back for it, it is not any more.
    pub(super) fn allow(&mut self, path: &Path) {
        self.denied.retain(|d| !path.starts_with(&d.folder));
    }
}

impl App {
    /// The worker listed the volumes (or, in the demo, made-up ones are shown instead).
    pub(super) fn on_volumes(&mut self, volumes: Vec<Volume>) {
        self.devices.volumes_seen = true;
        self.devices.volumes = if self.hooks.demo_volumes {
            demo_volumes()
        } else {
            volumes
        };
        self.folder_info
            .sizes
            .set_volumes(&self.devices.volumes, self.svc.platform.device_root());
        self.refresh_palette();
    }

    pub(super) fn on_unmounted(&mut self, list: Vec<Unmounted>) {
        self.devices.unmounted = if self.hooks.demo_volumes {
            Vec::new()
        } else {
            list
        };
    }

    /// When the folder in view is inside a phone or a camera: the name the device goes by (the
    /// one the sidebar shows) and the folder that is the device, which stand for the long
    /// path of the virtual filesystem at the head of the path bar.
    pub fn device_head(&self) -> Option<(String, PathBuf)> {
        let root = self.svc.platform.device_root()?;
        let device = self.tab.cwd.strip_prefix(root).ok()?.components().next()?;
        let dir = root.join(device);
        let label = self
            .devices
            .volumes
            .iter()
            .find(|v| v.mount_point == dir)
            .and_then(|v| v.label.clone())?;
        Some((label, dir))
    }

    /// Mount the device `id`: a thread asks the system, an event comes back.
    pub fn start_mount(&mut self, id: &str) {
        let Some(device) = self.devices.unmounted.iter().find(|u| u.id == id).cloned() else {
            return;
        };
        if !self.svc.platform.volumes().can_mount(&device) {
            self.toast(ToastKind::Info, Key::TCannotEject.tr(), 3);
            return;
        }
        if !self.devices.mounting.insert(device.id.clone()) {
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
        self.devices.mounting.remove(&id);
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
