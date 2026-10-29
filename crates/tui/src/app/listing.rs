//! What the front tab shows: the entries of the folder that pass the filter, which one is under
//! the cursor, and what an operation would apply to.

use rada_core::i18n::{Key, tf};
use rada_core::model::{Entry, SortKey, SortSpec};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::PathBuf;

use super::{App, LoadState, TabState, ToastKind};

impl TabState {
    pub fn entry_at(&self, vis_index: usize) -> Option<&Entry> {
        self.visible
            .get(vis_index)
            .and_then(|&i| self.listing.all().get(i))
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entry_at(self.sel.cursor)
    }

    /// Paths an operation applies to: the marked entries, or else the one under the cursor.
    pub fn targets(&self) -> Vec<PathBuf> {
        if !self.sel.marked.is_empty() {
            return self
                .listing
                .all()
                .iter()
                .filter(|e| self.sel.marked.contains(&e.name))
                .map(|e| e.path.clone())
                .collect();
        }
        self.current()
            .map(|e| vec![e.path.clone()])
            .unwrap_or_default()
    }

    pub(crate) fn ensure_visible(&mut self) {
        let (view, viewport) = (self.view, self.viewport);
        self.sel.keep_visible(view, viewport, self.visible.len());
    }

    pub(super) fn cursor_name(&self) -> Option<OsString> {
        self.current().map(|e| e.name.clone())
    }

    pub(super) fn restore_cursor(&mut self, name: Option<OsString>) {
        let idx = name.and_then(|n| {
            self.visible
                .iter()
                .position(|&i| self.listing.all()[i].name == n)
        });
        match idx {
            Some(i) => self.sel.cursor = i,
            None => self.sel.cursor = self.sel.cursor.min(self.visible.len().saturating_sub(1)),
        }
        self.ensure_visible();
    }

    /// Forget the marks of entries that are no longer in the folder.
    pub(super) fn drop_stale_marks(&mut self) {
        let existing: BTreeSet<OsString> =
            self.listing.all().iter().map(|e| e.name.clone()).collect();
        self.sel.marked.retain(|n| existing.contains(n));
    }

    pub(super) fn mark_all(&mut self) {
        let all: Vec<OsString> = self
            .visible
            .iter()
            .map(|&i| self.listing.all()[i].name.clone())
            .collect();
        self.sel.toggle_all(all);
    }

    /// The tag whose items the list shows, when it is not a folder but the view of a tag.
    pub fn tag_view(&self) -> Option<String> {
        rada_core::tagsearch::view_name(&self.cwd)
    }

    /// Whether the list shows the files used lately (the "Recent" entry of the sidebar).
    pub fn recents_view(&self) -> bool {
        rada_core::recents::is_view(&self.cwd)
    }

    /// What the list is when it is not a folder: "Tag: Blu", "Recent". `None` for a folder.
    pub fn view_title(&self) -> Option<String> {
        if self.recents_view() {
            return Some(Key::PlaceRecent.tr().to_string());
        }
        self.tag_view()
            .map(|tag| tf(Key::TagViewTitle, &[&rada_core::display::text(&tag)]))
    }

    pub fn sort_label(&self) -> String {
        let arrow = if self.sort.descending() { "↓" } else { "↑" };
        format!("{} {arrow}", self.sort.key.label())
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.load, LoadState::Loading(_))
    }
}

/// Whether an entry carries a tag matching `want` (lower case): a name that contains it, or
/// the colour of that name. An empty `want` (`tag:` alone) means "any tag".
fn matches_tag(e: &Entry, want: &str) -> bool {
    use rada_core::platform::TagColor as Hue;
    if want.is_empty() {
        return !e.tags.is_empty();
    }
    // A colour by any of its names ("red", "rosso") is the tags of that colour.
    let hue = Hue::from_name(want);
    e.tags
        .iter()
        .any(|t| t.name.to_lowercase().contains(want) || (hue.is_some() && t.color == hue))
}

/// Whether every one of `terms` (lower case) is in the name of an entry. The name shown and
/// the name on disk both count: "Scrivania" and "Desktop" find the same folder.
fn matches_name(e: &Entry, terms: &[&String]) -> bool {
    let name = e.display.to_lowercase();
    let real = e.name.to_string_lossy().to_lowercase();
    terms
        .iter()
        .all(|t| name.contains(t.as_str()) || real.contains(t.as_str()))
}

impl App {
    pub(super) fn toggle_hidden(&mut self) {
        let keep = self.tab.cursor_name();
        self.settings.show_hidden = !self.settings.show_hidden;
        self.rebuild_visible();
        self.tab.restore_cursor(keep);
        // A folder's preview lists what the list lists.
        self.preview.invalidate();
        self.request_preview();
    }

    pub(super) fn rebuild_visible(&mut self) {
        self.folder_info.date_cache.clear();
        let show = self.settings.show_hidden;
        // Choosing a folder: only folders are listed.
        let dirs_only = self
            .session
            .chooser
            .as_ref()
            .is_some_and(|c| c.folders_only());
        let terms: Vec<String> = self
            .tab
            .filter
            .as_ref()
            .map(|f| f.text.split_whitespace().map(str::to_lowercase).collect())
            .unwrap_or_default();
        // `tag:red` keeps what carries a tag with that in its name (or of that colour); every
        // other word is part of the name.
        let (tag_terms, name_terms): (Vec<&String>, Vec<&String>) =
            terms.iter().partition(|t| t.starts_with("tag:"));
        let tag_terms: Vec<&str> = tag_terms.iter().map(|t| &t["tag:".len()..]).collect();
        self.tab.visible = self
            .tab
            .listing
            .all()
            .iter()
            .enumerate()
            .filter(|(_, e)| show || !e.hidden)
            .filter(|(_, e)| !dirs_only || e.is_dir())
            .filter(|(_, e)| name_terms.is_empty() || matches_name(e, &name_terms))
            .filter(|(_, e)| tag_terms.iter().all(|want| matches_tag(e, want)))
            .map(|(i, _)| i)
            .collect();
    }

    pub(super) fn set_sort(&mut self, spec: SortSpec) {
        let keep = self.tab.cursor_name();
        self.tab.sort = spec;
        // From now on this tab keeps its order, whatever the configuration says.
        self.tab.sort_chosen = true;
        self.save_ui_state();
        self.tab.listing.set_sort(spec); // immediate, in memory
        self.rebuild_visible();
        self.tab.restore_cursor(keep);
        let arrow = if spec.descending() { "↓" } else { "↑" };
        self.toast(
            ToastKind::Info,
            tf(
                Key::TSortedBy,
                &[
                    &match spec.key {
                        SortKey::Name => Key::SortKeyName,
                        SortKey::Size => Key::SortKeySize,
                        SortKey::Modified => Key::SortKeyDate,
                        SortKey::Type => Key::SortKeyType,
                    }
                    .tr(),
                    &arrow,
                ],
            ),
            2,
        );
    }

    /// Work out the date text of these entries (indices into the listing), once; the list
    /// reads them with [`FolderInfo::date_text`].
    pub fn warm_dates(&mut self, indices: impl Iterator<Item = usize>) {
        let hour = self
            .now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() / 3600)
            .unwrap_or(0);
        let key = (hour, self.tab.listing.all().len());
        self.folder_info.dates_for(key);
        let now = self.now();
        for i in indices {
            if let Some(slot @ None) = self.folder_info.date_cache.get_mut(i)
                && let Some(e) = self.tab.listing.all().get(i)
            {
                *slot = Some(self.settings.explorer_date(e.used.or(e.mtime), now));
            }
        }
    }
}
