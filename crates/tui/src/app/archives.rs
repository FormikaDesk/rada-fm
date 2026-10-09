//! Archives in the interface: telling which items are archives, extracting, compressing.

use rada_core::archive::{self, ArchiveKind, name_is_zip_document, name_suggests_archive};
use rada_core::ops::{ConflictPolicy, ExtractInto, OpRequest};

use super::*;

impl App {
    /// Whether Enter should open `e` as a folder: its name says archive, or its content did
    /// (the preview read it) and it is not a document that merely is a ZIP.
    pub(super) fn archive_candidate(&self, e: &Entry) -> bool {
        if e.is_dir() || e.kind != rada_core::fs::FileKind::File {
            return false;
        }
        if name_suggests_archive(&e.name) {
            return true;
        }
        self.preview.path.as_deref() == Some(e.path.as_path())
            && !name_is_zip_document(&e.name)
            && matches!(&self.preview.content, Some(Preview::Archive(a)) if a.problem.is_none())
    }

    pub fn in_archive(&self) -> bool {
        self.archive.is_some()
    }

    /// The cursor is on an archive (outside one): extraction is on offer.
    pub fn on_archive_item(&self) -> bool {
        self.archive.is_none() && self.current().is_some_and(|e| self.archive_candidate(e))
    }

    /// The entries an action applies to: the marked ones, else the one under the cursor.
    fn target_entries(&self) -> Vec<&Entry> {
        if !self.marked.is_empty() {
            return self
                .listing
                .all()
                .iter()
                .filter(|e| self.marked.contains(&e.name))
                .collect();
        }
        self.current().into_iter().collect()
    }

    /// Extract here (or, with `here` false, into a folder named after the archive).
    pub(super) fn start_extract(&mut self, here: bool) {
        if self.refuse_if_busy() {
            return;
        }
        // Inside an archive: the marked members, or everything. The folder is the archive's.
        if let Some(view) = self.archive.clone() {
            let only: Vec<PathBuf> = if self.marked.is_empty() {
                Vec::new()
            } else {
                self.targets()
                    .iter()
                    .filter_map(|p| p.strip_prefix(&view.archive).ok().map(Path::to_path_buf))
                    .collect()
            };
            let dest = view
                .archive
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.cwd.clone());
            if !only.is_empty() && !here {
                self.toast(
                    ToastKind::Info,
                    format!(
                        "to extract part of an archive somewhere else: copy it ({}), go there and paste ({})",
                        self.key_for(Action::Copy).unwrap_or_else(|| "copy".into()),
                        self.key_for(Action::Paste).unwrap_or_else(|| "paste".into()),
                    ),
                    5,
                );
                return;
            }
            if only.is_empty() && !here {
                return self.ask_extract_folder(view.archive, dest);
            }
            return self.plan_extract_now(view.archive, only, dest, ExtractInto::Here);
        }
        let archives: Vec<PathBuf> = self
            .target_entries()
            .into_iter()
            .filter(|e| self.archive_candidate(e))
            .map(|e| e.path.clone())
            .collect();
        match archives.len() {
            0 => self.toast(
                ToastKind::Info,
                "select an archive to extract (zip, tar, 7z, rar, .gz, .xz, .zst…)",
                3,
            ),
            1 => {
                let archive = archives[0].clone();
                let dest = self.cwd.clone();
                if here {
                    self.plan_extract_now(archive, Vec::new(), dest, ExtractInto::Here);
                } else {
                    self.ask_extract_folder(archive, dest);
                }
            }
            _ => self.toast(ToastKind::Info, "extract one archive at a time", 3),
        }
    }

    fn plan_extract_now(
        &mut self,
        archive: PathBuf,
        only: Vec<PathBuf>,
        dest: PathBuf,
        into: ExtractInto,
    ) {
        let h = self.svc.jobs.plan_request(OpRequest::Extract {
            archive,
            destination: dest,
            into,
            only,
            conflict: ConflictPolicy::Skip,
        });
        self.begin_plan("Reading the archive", h);
    }

    fn ask_extract_folder(&mut self, archive: PathBuf, dest: PathBuf) {
        let default = archive
            .file_name()
            .map(|n| archive::archive_stem(n).to_string_lossy().into_owned())
            .unwrap_or_default();
        self.modal = Some(Modal::Input(InputView::new(
            InputKind::ExtractTo {
                archive,
                dest,
                default: default.clone(),
            },
            default,
        )));
    }

    /// Ask for a name and a format, then plan the new archive.
    pub(super) fn start_compress(&mut self) {
        if self.refuse_if_busy() || self.refuse_in_archive("compress from here") {
            return;
        }
        let sources = self.targets();
        if sources.is_empty() {
            return;
        }
        let format = ArchiveKind::Zip;
        let base = if sources.len() == 1 {
            let n = sources[0].file_name().unwrap_or_default();
            // A file loses its extension (report.pdf → report.zip); a folder keeps its name.
            let is_dir = self.current().is_some_and(|e| e.is_dir());
            if is_dir {
                n.to_string_lossy().into_owned()
            } else {
                Path::new(n)
                    .file_stem()
                    .unwrap_or(n)
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            self.cwd
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "archive".into())
        };
        let text = format!("{base}{}", format.extension());
        self.modal = Some(Modal::Input(InputView::new(
            InputKind::Compress {
                sources,
                dir: self.cwd.clone(),
                format,
            },
            text,
        )));
    }
}

impl InputView {
    /// Tab in the name of a new archive: the next format, with the extension changed to match.
    pub(super) fn cycle_format(&mut self) {
        let InputKind::Compress { format, .. } = &mut self.kind else {
            return;
        };
        let old = *format;
        let next = old.next();
        let stem = self
            .text
            .strip_suffix(old.extension())
            .unwrap_or(&self.text)
            .to_string();
        *format = next;
        self.text = format!("{stem}{}", next.extension());
        self.cursor = self.text.chars().count();
        self.error = None;
    }
}
