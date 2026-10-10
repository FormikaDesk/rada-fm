//! The Properties window: what is known about the item under the cursor, or about the
//! selection, taken from the listing already in memory (nothing is read from the disk).

use super::*;

pub struct PropsView {
    pub title: String,
    /// Label and value, in order.
    pub rows: Vec<(String, String)>,
    pub scroll: usize,
}

impl App {
    pub fn open_properties(&mut self) {
        let view = if self.marked.len() > 1 {
            self.selection_properties()
        } else {
            let Some(e) = self.current() else { return };
            self.item_properties(e)
        };
        self.modal = Some(Modal::Properties(view));
    }

    fn selection_properties(&self) -> PropsView {
        let chosen: Vec<&Entry> = self
            .listing
            .all()
            .iter()
            .filter(|e| self.marked.contains(&e.name))
            .collect();
        let size: u64 = chosen.iter().filter(|e| !e.is_dir()).map(|e| e.size).sum();
        let folders = chosen.iter().filter(|e| e.is_dir()).count();
        let mut rows = vec![
            (
                "Items".to_string(),
                format!(
                    "{} ({} files, {})",
                    chosen.len(),
                    chosen.len() - folders,
                    fmt::count(folders as u64, "folder", "folders")
                ),
            ),
            (
                "Size".to_string(),
                format!(
                    "{} ({} bytes), folders not counted",
                    fmt::size(size),
                    fmt::thousands(size)
                ),
            ),
            (
                "Location".to_string(),
                fmt::short_path(&self.cwd, self.home()),
            ),
        ];
        if self.archive.is_some() {
            rows.push(("Inside".to_string(), "an archive (read-only)".to_string()));
        }
        PropsView {
            title: format!("{} selected", chosen.len()),
            rows,
            scroll: 0,
        }
    }

    fn item_properties(&self, e: &Entry) -> PropsView {
        let tz = jiff::tz::TimeZone::system();
        let when = |t| fmt::date_with_day(t, self.now(), self.date_format, &tz);
        let mut rows: Vec<(String, String)> = vec![
            ("Name".into(), e.display.clone()),
            ("Type".into(), e.type_label.to_string()),
            (
                "Location".into(),
                fmt::short_path(e.path.parent().unwrap_or(&self.cwd), self.home()),
            ),
        ];
        if let Some(l) = &e.link {
            rows.push(("Link to".into(), rada_core::display::path(&l.target)));
        }
        if !e.is_dir() {
            rows.push((
                "Size".into(),
                format!("{} ({} bytes)", fmt::size(e.size), fmt::thousands(e.size)),
            ));
        }
        rows.push(("Modified".into(), when(e.mtime)));
        if e.created.is_some() {
            rows.push(("Created".into(), when(e.created)));
        }
        if let Some(m) = e.mode {
            rows.push((
                "Permissions".into(),
                format!("{}  {:04o}", rada_core::preview::mode_string(m), m & 0o7777),
            ));
        }
        let mut flags: Vec<&str> = Vec::new();
        if e.hidden {
            flags.push("hidden");
        }
        if e.readonly {
            flags.push("read-only");
        }
        if e.executable {
            flags.push("executable");
        }
        if !flags.is_empty() {
            rows.push(("Attributes".into(), flags.join(", ")));
        }
        if let Some(err) = &e.error {
            rows.push(("Problem".into(), err.clone()));
        }
        if self.archive.is_some() {
            rows.push(("Inside".into(), "an archive (read-only)".into()));
        }
        PropsView {
            title: e.display.clone(),
            rows,
            scroll: 0,
        }
    }
}
