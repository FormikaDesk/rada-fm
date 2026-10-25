//! What a finished operation leaves behind and where, and the card that says it went through.

use std::path::{Path, PathBuf};

use rada_core::i18n::{Key, tf};
use rada_core::ops::{OpKind, Plan};

use super::{App, Notice, NoticeAction, NoticeKind, Running};
use crate::fmt;
use crate::keymap::Action;

/// Where a finished operation left its result, for the "Show" of its card.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// The folder the result is in.
    pub folder: Option<PathBuf>,
    /// The item to put the cursor on there.
    pub select: Option<std::ffi::OsString>,
    /// What the result is called, where the message names it (a new folder, an archive).
    pub subject: String,
}

fn name_of(p: &Path) -> std::ffi::OsString {
    p.file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default()
}

/// The archive that was made is where the card shows, with the cursor on it.
fn compressed_outcome(plan: &Plan) -> Outcome {
    let archive = plan.destination.clone().unwrap_or_default();
    Outcome {
        folder: archive.parent().map(Path::to_path_buf),
        subject: rada_core::display::name(&name_of(&archive)),
        select: Some(name_of(&archive)),
    }
}

/// The first item renamed is where the card shows, with the cursor on its new name.
fn renamed_outcome(plan: &Plan) -> Outcome {
    use rada_core::ops::Step;
    let first = plan.steps.iter().find_map(|s| match s {
        Step::Rename { to, .. } => Some(to.clone()),
        _ => None,
    });
    Outcome {
        folder: first
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf),
        select: first.as_deref().map(name_of),
        subject: first
            .as_deref()
            .map(|p| rada_core::display::name(&name_of(p)))
            .unwrap_or_default(),
    }
}

/// The folder or file that was made is where the card shows, with the cursor on it.
fn made_outcome(plan: &Plan) -> Outcome {
    use rada_core::ops::Step;
    let made = plan.steps.iter().find_map(|s| match s {
        Step::MakeDir { path, .. } | Step::MakeFile { path, .. } => Some(path.clone()),
        _ => None,
    });
    Outcome {
        folder: plan.destination.clone(),
        select: made.as_deref().map(name_of),
        subject: made
            .as_deref()
            .map(|p| rada_core::display::name(&name_of(p)))
            .unwrap_or_default(),
    }
}

/// "1 item copied to X" / "3 items copied to X", and the ones with nothing to name.
fn done_text(kind: OpKind, n: u64, dest: &str, subject: &str) -> String {
    let count = fmt::thousands(n);
    let one = n == 1;
    let two = |a: Key, b: Key, last: &str| {
        if one {
            tf(a, &[&last])
        } else {
            tf(b, &[&count, &last])
        }
    };
    let counted = |a: Key, b: Key| {
        if one {
            a.tr().to_string()
        } else {
            tf(b, &[&count])
        }
    };
    match kind {
        OpKind::Copy => two(Key::NCopyOne, Key::NCopyMany, dest),
        OpKind::Move => two(Key::NMoveOne, Key::NMoveMany, dest),
        OpKind::Trash => counted(Key::NTrashOne, Key::NTrashMany),
        OpKind::Rename | OpKind::BulkRename => {
            if one {
                tf(Key::NRenameOne, &[&subject])
            } else {
                tf(Key::NRenameMany, &[&count])
            }
        }
        OpKind::Compress => two(Key::NCompressOne, Key::NCompressMany, subject),
        OpKind::Extract => two(Key::NExtractOne, Key::NExtractMany, dest),
        OpKind::SetTags => counted(Key::NTagsOne, Key::NTagsMany),
        OpKind::MakeDir => tf(Key::NFolderCreated, &[&subject]),
        OpKind::MakeFile => tf(Key::NFileCreated, &[&subject]),
        OpKind::Delete => counted(Key::NDeleteOne, Key::NDeleteMany),
        OpKind::Undo => Key::NUndone.tr().to_string(),
    }
}

impl App {
    /// What a plan will leave behind and where, worked out before it runs.
    pub(super) fn outcome_of(&self, plan: &Plan) -> Outcome {
        match plan.kind {
            OpKind::Copy | OpKind::Move | OpKind::Extract => Outcome {
                folder: plan.destination.clone(),
                ..Outcome::default()
            },
            OpKind::Compress => compressed_outcome(plan),
            OpKind::Rename | OpKind::BulkRename => renamed_outcome(plan),
            OpKind::MakeDir | OpKind::MakeFile => made_outcome(plan),
            OpKind::Trash => Outcome {
                folder: self
                    .places
                    .paths
                    .places
                    .iter()
                    .find(|p| p.kind == rada_core::platform::PlaceKind::Trash)
                    .map(|p| p.path.clone()),
                ..Outcome::default()
            },
            OpKind::SetTags => Outcome {
                folder: Some(self.tab.cwd.clone()),
                ..Outcome::default()
            },
            OpKind::Delete | OpKind::Undo => Outcome::default(),
        }
    }

    /// [`folder_label`](Self::folder_label), for the demo scene of the README's pictures.
    pub(super) fn folder_label_for_demo(&self, p: &Path) -> String {
        self.folder_label(p)
    }

    /// A folder as a message names it: "Documents", or "Home" for the home folder.
    fn folder_label(&self, p: &Path) -> String {
        if p == self.home() {
            return Key::HomeLabel.tr().to_string();
        }
        let on_disk = p
            .file_name()
            .map(rada_core::display::name)
            .unwrap_or_else(|| rada_core::display::path(p));
        self.places.shown_name(p, on_disk)
    }

    /// The card for an operation that went through cleanly: what was done, to how many items
    /// and where, with the way to take it back and the way to see it.
    pub(super) fn done_notice(&self, r: &Running) -> Notice {
        let n = r.totals.items.max(1);
        let dest = r
            .outcome
            .folder
            .as_deref()
            .map(|p| self.folder_label(p))
            .unwrap_or_default();
        let text = done_text(r.kind, n, &dest, &r.outcome.subject);
        let action = match r.kind {
            OpKind::Undo => NoticeAction::Redo,
            _ if r.reversible => NoticeAction::Undo,
            _ => NoticeAction::None,
        };
        let mut notice = Notice::new(NoticeKind::Ok, text).with_action(action);
        if let Some(folder) = &r.outcome.folder
            && r.kind != OpKind::Delete
            && r.kind != OpKind::Undo
        {
            notice = notice.showing(folder.clone(), r.outcome.select.clone());
        }
        notice
    }

    /// A click on a notification card.
    pub(super) fn click_notice(&mut self, id: u64, hit: crate::hits::NoticeHit) {
        use crate::hits::NoticeHit;
        let now = self.notice_now();
        let shown = self.windows.notices.get(id).cloned();
        self.redraw.dirty = true;
        match hit {
            NoticeHit::CancelRun => {
                // The same as Esc: the operation that is running is cancelled.
                self.dispatch(Action::ClearSelection);
                return;
            }
            // The card of a running operation has nothing else to click.
            _ if id == 0 => return,
            _ => {}
        }
        self.windows.notices.dismiss(id, now);
        match hit {
            NoticeHit::Undo => self.dispatch(Action::Undo),
            NoticeHit::Redo => self.dispatch(Action::Redo),
            NoticeHit::Show => {
                if let Some(Notice {
                    show: Some(folder),
                    select,
                    ..
                }) = shown
                {
                    if let Some(name) = select {
                        self.tab.remembered.insert(folder.clone(), name);
                    }
                    self.open_dir(folder);
                }
            }
            NoticeHit::Close | NoticeHit::Card | NoticeHit::CancelRun => {}
        }
    }
}
