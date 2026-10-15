//! Drawing documents without poppler or any program to install: what the system itself can
//! do.
//!
//! * **macOS**: `sips` renders the first page of a PDF; `qlmanage -t` asks Quick Look for the
//!   thumbnail of anything else it knows (Office documents, Pages, Numbers, Keynote, RTF,
//!   videos…), the same picture the Finder shows.
//! * **Windows**: `Windows.Data.Pdf`, the PDF renderer that comes with Windows 10 and 11, for
//!   PDFs; the shell's thumbnails (`IShellItemImageFactory`) for the rest, when Windows has a
//!   handler for the type.
//!
//! Like poppler, everything runs in the image worker with a time limit, is stopped when the
//! person moves on, and lands in the preview cache.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use super::image::{ImageInfo, ImageLimits};
use super::pdf::{self, PdfProblem};

/// Which system's tools draw documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemRenderer {
    Mac,
    Windows,
}

impl SystemRenderer {
    /// The renderer of the system rada runs on, if it has one.
    pub fn native() -> Option<SystemRenderer> {
        if cfg!(target_os = "macos") {
            Some(SystemRenderer::Mac)
        } else if cfg!(windows) {
            Some(SystemRenderer::Windows)
        } else {
            None
        }
    }
}

/// The programs macOS draws with.
#[derive(Clone, Debug, Default)]
pub struct MacTools {
    pub sips: Option<PathBuf>,
    pub qlmanage: Option<PathBuf>,
}

impl MacTools {
    pub fn find_in(path: &OsStr) -> MacTools {
        let find = |name: &str| crate::platform::which::which_in(path, name);
        MacTools {
            sips: find("sips"),
            qlmanage: find("qlmanage"),
        }
    }

    fn for_limits(limits: &ImageLimits) -> MacTools {
        // The folder asked for (or `PATH`) first; then where the tools live, which a minimal
        // `PATH` (a program started from the desktop) may leave out.
        let mut path = limits
            .pdf_tools_path
            .clone()
            .unwrap_or_else(|| std::env::var_os("PATH").unwrap_or_default());
        path.push(":/usr/bin:/usr/sbin");
        MacTools::find_in(&path)
    }
}

/// What kind of document an extension is, for the types the system can draw a thumbnail of;
/// `None` for anything else. Pictures and PDFs are not here: they have their own path.
pub fn thumbnail_label(name: &str, renderer: SystemRenderer) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    let label = match ext.as_str() {
        "doc" | "docx" | "dot" | "dotx" | "docm" => "Word document",
        "xls" | "xlsx" | "xlsm" | "xlt" | "xltx" => "Excel spreadsheet",
        "ppt" | "pptx" | "pps" | "ppsx" | "pptm" => "PowerPoint presentation",
        "odt" | "ott" => "OpenDocument text",
        "ods" | "ots" => "OpenDocument spreadsheet",
        "odp" | "otp" => "OpenDocument presentation",
        "rtf" => "Rich text document",
        "epub" => "EPUB book",
        "mov" | "mp4" | "m4v" | "avi" | "mkv" | "webm" | "wmv" | "mpg" | "mpeg" | "3gp" => "Video",
        "heic" | "heif" | "psd" | "ai" | "eps" | "raw" | "cr2" | "nef" | "arw" | "dng" => "Picture",
        // Apple's formats are drawn by Quick Look, and by nothing on Windows.
        "pages" if renderer == SystemRenderer::Mac => "Pages document",
        "numbers" if renderer == SystemRenderer::Mac => "Numbers spreadsheet",
        "key" if renderer == SystemRenderer::Mac => "Keynote presentation",
        _ => return None,
    };
    Some(label)
}

// ------------------------------------------------------------------------------ the cache

/// Where a picture of `path` made by `tag` is kept: it changes when the file does.
fn cache_png(limits: &ImageLimits, path: &Path, info: &ImageInfo, tag: &str) -> (PathBuf, PathBuf) {
    let cache = limits
        .pdf_cache
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("rada-previews"));
    let png = pdf::cache_file_tagged(&cache, path, info, limits.max_edge, tag);
    (cache, png)
}

/// The picture from the cache, or made by `make` (which writes a PNG to the file it is
/// given) and kept.
fn cached(
    limits: &ImageLimits,
    path: &Path,
    info: &ImageInfo,
    tag: &str,
    make: &dyn Fn(&Path) -> Result<(), PdfProblem>,
) -> Result<PathBuf, PdfProblem> {
    let (cache, png) = cache_png(limits, path, info, tag);
    if png.is_file() {
        if let Ok(f) = std::fs::File::options().append(true).open(&png) {
            let _ = f.set_modified(SystemTime::now());
        }
        return Ok(png);
    }
    std::fs::create_dir_all(&cache)
        .map_err(|e| PdfProblem::Damaged(format!("cannot use the preview cache: {e}")))?;
    // Made under a temporary name and renamed: a half-written picture is never cached.
    let part = png.with_extension("part.png");
    let out = make(&part).and_then(|()| {
        if part.is_file() {
            std::fs::rename(&part, &png)
                .map_err(|e| PdfProblem::Damaged(format!("cannot store the preview: {e}")))?;
            Ok(png.clone())
        } else {
            Err(PdfProblem::Damaged("the system made no picture".into()))
        }
    });
    if out.is_err() {
        let _ = std::fs::remove_file(&part);
    } else {
        pdf::maybe_prune(&cache, limits);
    }
    out
}

// ------------------------------------------------------------------------------ rendering

/// The first page of a PDF, as a PNG file in the cache.
pub fn render_pdf(
    renderer: SystemRenderer,
    path: &Path,
    info: &ImageInfo,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> Result<PathBuf, PdfProblem> {
    cached(
        limits,
        path,
        info,
        "pdf-system-v1",
        &|part| match renderer {
            SystemRenderer::Mac => mac_render_pdf(path, part, limits, abandon),
            SystemRenderer::Windows => windows_render_pdf(path, part, limits, abandon),
        },
    )
}

/// A document's thumbnail, as a PNG file in the cache.
pub fn render_thumbnail(
    renderer: SystemRenderer,
    path: &Path,
    info: &ImageInfo,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> Result<PathBuf, PdfProblem> {
    cached(limits, path, info, "thumb-v1", &|part| match renderer {
        SystemRenderer::Mac => mac_thumbnail(path, part, limits, abandon),
        SystemRenderer::Windows => windows_thumbnail(path, part, limits, abandon),
    })
}

/// `sips -s format png -Z <edge> file --out png`: the first page of the PDF.
fn mac_render_pdf(
    path: &Path,
    out: &Path,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> Result<(), PdfProblem> {
    let tools = MacTools::for_limits(limits);
    let sips = tools.sips.ok_or(PdfProblem::NoTool("sips"))?;
    let mut cmd = Command::new(sips);
    cmd.args(["-s", "format", "png", "-Z"])
        .arg(limits.max_edge.to_string())
        .arg(path)
        .arg("--out")
        .arg(out);
    let run = pdf::run_limited(cmd, limits.pdf_timeout, 16 * 1024, abandon)?;
    if !run.status_ok {
        return Err(sips_problem(&run.stderr));
    }
    Ok(())
}

/// What `sips` or `qlmanage` said, as a problem.
pub fn sips_problem(stderr: &str) -> PdfProblem {
    let low = stderr.to_lowercase();
    if low.contains("password") || low.contains("encrypted") || low.contains("locked") {
        return PdfProblem::Password;
    }
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("the file could not be drawn");
    PdfProblem::Damaged(line.trim_start_matches("Error: ").to_string())
}

/// `qlmanage -t -s <edge> -o <dir> file`: the thumbnail Quick Look makes for the Finder.
fn mac_thumbnail(
    path: &Path,
    out: &Path,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> Result<(), PdfProblem> {
    let tools = MacTools::for_limits(limits);
    let ql = tools.qlmanage.ok_or(PdfProblem::NoTool("qlmanage"))?;
    let dir = out.with_extension("dir");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| PdfProblem::Damaged(format!("cannot use the preview cache: {e}")))?;
    let mut cmd = Command::new(ql);
    cmd.args(["-t", "-s"])
        .arg(limits.max_edge.to_string())
        .arg("-o")
        .arg(&dir)
        .arg(path);
    let run = pdf::run_limited(cmd, limits.pdf_timeout, 16 * 1024, abandon);
    // It writes `<dir>/<file name>.png`; it exits 0 even when there is nothing to show.
    let made = path.file_name().map(|n| {
        let mut f = n.to_os_string();
        f.push(".png");
        dir.join(f)
    });
    let result = match run {
        Err(e) => Err(e),
        Ok(r) if !r.status_ok => Err(sips_problem(&r.stderr)),
        Ok(_) => match made {
            Some(m) if m.is_file() => std::fs::rename(&m, out)
                .map_err(|e| PdfProblem::Damaged(format!("cannot store the preview: {e}"))),
            _ => Err(PdfProblem::Damaged(
                "Quick Look has no preview for this file".into(),
            )),
        },
    };
    let _ = std::fs::remove_dir_all(&dir);
    result
}

// ---------------------------------------------------------------------------------- Windows

#[cfg(windows)]
mod win {
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::{
        StorageFile,
        Streams::{DataReader, InMemoryRandomAccessStream},
    };
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GetDIBits, GetObjectW, HGDIOBJ,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
        SIIGBF_THUMBNAILONLY,
    };
    use windows::core::HSTRING;

    use super::PdfProblem;

    /// Run `work` on a thread of its own, initialised for COM, giving up after `limit` or
    /// when `abandon()` says so (the thread is left to finish by itself).
    fn on_com_thread<T: Send + 'static>(
        limit: Duration,
        abandon: &dyn Fn() -> bool,
        work: impl FnOnce() -> Result<T, PdfProblem> + Send + 'static,
    ) -> Result<T, PdfProblem> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("rada-winpreview".into())
            .spawn(move || {
                // SAFETY: balanced with CoUninitialize below, on the same thread.
                let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let out = work();
                if init.is_ok() {
                    // SAFETY: the initialisation above succeeded.
                    unsafe { CoUninitialize() };
                }
                let _ = tx.send(out);
            })
            .map_err(|e| PdfProblem::Damaged(format!("cannot start a thread: {e}")))?;
        let start = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(15)) {
                Ok(r) => return r,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(PdfProblem::Damaged("the renderer stopped".into()));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if abandon() {
                return Err(PdfProblem::Abandoned);
            }
            if start.elapsed() > limit {
                return Err(PdfProblem::TooSlow(limit));
            }
        }
    }

    fn bad(e: windows::core::Error) -> PdfProblem {
        // The reader says "the document is password protected" in its own way.
        let msg = e.message();
        if msg.to_lowercase().contains("password") || e.code().0 as u32 == 0x8007_052B {
            PdfProblem::Password
        } else {
            PdfProblem::Damaged(msg.trim().to_string())
        }
    }

    /// The first page of a PDF through `Windows.Data.Pdf`, as a PNG written to `out`.
    pub fn render_pdf(
        path: &Path,
        out: &Path,
        edge: u32,
        limit: Duration,
        abandon: &dyn Fn() -> bool,
    ) -> Result<(), PdfProblem> {
        let (path, out) = (path.to_path_buf(), out.to_path_buf());
        on_com_thread(limit, abandon, move || {
            let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
                .and_then(|o| o.join())
                .map_err(bad)?;
            let doc = PdfDocument::LoadFromFileAsync(&file)
                .and_then(|o| o.join())
                .map_err(bad)?;
            if doc.IsPasswordProtected().unwrap_or(false) {
                return Err(PdfProblem::Password);
            }
            let page = doc.GetPage(0).map_err(bad)?;
            let size = page.Size().map_err(bad)?;
            let (w, h) = (size.Width.max(1.0), size.Height.max(1.0));
            let scale = edge as f32 / w.max(h);
            let opts = PdfPageRenderOptions::new().map_err(bad)?;
            opts.SetDestinationWidth((w * scale).round().max(1.0) as u32)
                .map_err(bad)?;
            opts.SetDestinationHeight((h * scale).round().max(1.0) as u32)
                .map_err(bad)?;
            // Paper is white: the renderer leaves the page transparent otherwise.
            opts.SetBackgroundColor(windows::UI::Color {
                A: 255,
                R: 255,
                G: 255,
                B: 255,
            })
            .map_err(bad)?;
            let stream = InMemoryRandomAccessStream::new().map_err(bad)?;
            page.RenderWithOptionsToStreamAsync(&stream, &opts)
                .and_then(|o| o.join())
                .map_err(bad)?;
            let len = stream.Size().map_err(bad)? as u32;
            let input = stream.GetInputStreamAt(0).map_err(bad)?;
            let reader = DataReader::CreateDataReader(&input).map_err(bad)?;
            reader.LoadAsync(len).and_then(|o| o.join()).map_err(bad)?;
            let mut bytes = vec![0u8; len as usize];
            reader.ReadBytes(&mut bytes).map_err(bad)?;
            std::fs::write(&out, bytes)
                .map_err(|e| PdfProblem::Damaged(format!("cannot store the preview: {e}")))?;
            Ok(())
        })
    }

    /// The shell's thumbnail of a file, as a PNG written to `out`.
    pub fn thumbnail(
        path: &Path,
        out: &Path,
        edge: u32,
        limit: Duration,
        abandon: &dyn Fn() -> bool,
    ) -> Result<(), PdfProblem> {
        let (path, out) = (path.to_path_buf(), out.to_path_buf());
        on_com_thread(limit, abandon, move || {
            // SAFETY: a NUL-terminated string made by HSTRING; the result is a COM object.
            let factory: IShellItemImageFactory =
                unsafe { SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None) }
                    .map_err(bad)?;
            let side = edge.min(i32::MAX as u32) as i32;
            // SAFETY: plain call on a live COM object.
            let hbm = unsafe {
                factory.GetImage(
                    SIZE { cx: side, cy: side },
                    SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK,
                )
            }
            .map_err(|_| PdfProblem::Damaged("Windows has no preview for this file".into()))?;
            let mut bm = BITMAP::default();
            // SAFETY: `bm` is a BITMAP of the size passed.
            let got = unsafe {
                GetObjectW(
                    HGDIOBJ(hbm.0),
                    std::mem::size_of::<BITMAP>() as i32,
                    Some(&mut bm as *mut BITMAP as *mut _),
                )
            };
            if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
                // SAFETY: the bitmap is ours.
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(hbm.0));
                }
                return Err(PdfProblem::Damaged(
                    "Windows has no preview for this file".into(),
                ));
            }
            let (w, h) = (bm.bmWidth as usize, bm.bmHeight as usize);
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; w * h * 4];
            // SAFETY: the buffer holds w*h 32-bit pixels, as the header says.
            let (lines, _) = unsafe {
                let dc = CreateCompatibleDC(None);
                let n = GetDIBits(
                    dc,
                    hbm,
                    0,
                    h as u32,
                    Some(bgra.as_mut_ptr() as *mut _),
                    &mut info,
                    DIB_RGB_COLORS,
                );
                let _ = DeleteDC(dc);
                let _ = DeleteObject(HGDIOBJ(hbm.0));
                (n, ())
            };
            if lines == 0 {
                return Err(PdfProblem::Damaged(
                    "Windows has no preview for this file".into(),
                ));
            }
            for px in bgra.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
            // A thumbnail without an alpha channel is opaque.
            if bgra.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
                for px in bgra.as_chunks_mut::<4>().0 {
                    px[3] = 255;
                }
            }
            let img = image::RgbaImage::from_raw(w as u32, h as u32, bgra)
                .ok_or_else(|| PdfProblem::Damaged("the picture has the wrong size".into()))?;
            img.save_with_format(&out, image::ImageFormat::Png)
                .map_err(|e| PdfProblem::Damaged(format!("cannot store the preview: {e}")))?;
            Ok(())
        })
    }
}

fn windows_render_pdf(
    path: &Path,
    out: &Path,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> Result<(), PdfProblem> {
    #[cfg(windows)]
    {
        win::render_pdf(path, out, limits.max_edge, limits.pdf_timeout, abandon)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, out, limits, abandon);
        Err(PdfProblem::NoTool("Windows.Data.Pdf"))
    }
}

fn windows_thumbnail(
    path: &Path,
    out: &Path,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> Result<(), PdfProblem> {
    #[cfg(windows)]
    {
        win::thumbnail(path, out, limits.max_edge, limits.pdf_timeout, abandon)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, out, limits, abandon);
        Err(PdfProblem::NoTool("the shell's thumbnails"))
    }
}

/// How long a thumbnail may take: documents and videos are slower to open than a PDF page.
pub fn thumbnail_timeout(limits: &ImageLimits) -> Duration {
    limits.pdf_timeout.max(Duration::from_secs(10))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_types_the_system_can_draw_are_named() {
        let m = SystemRenderer::Mac;
        assert_eq!(thumbnail_label("Report.DOCX", m), Some("Word document"));
        assert_eq!(thumbnail_label("budget.xlsx", m), Some("Excel spreadsheet"));
        assert_eq!(thumbnail_label("talk.key", m), Some("Keynote presentation"));
        assert_eq!(thumbnail_label("letter.pages", m), Some("Pages document"));
        assert_eq!(thumbnail_label("film.mov", m), Some("Video"));
        assert_eq!(thumbnail_label("a.rtf", m), Some("Rich text document"));
        assert_eq!(thumbnail_label("photo.heic", m), Some("Picture"));
        // Pictures rada decodes itself and PDFs have their own path; plain text is text.
        for n in ["a.png", "a.pdf", "a.txt", "noext", ".hidden", "a.rs"] {
            assert_eq!(thumbnail_label(n, m), None, "{n}");
        }
        // Apple's formats are Quick Look's alone.
        assert_eq!(
            thumbnail_label("letter.pages", SystemRenderer::Windows),
            None
        );
        assert_eq!(
            thumbnail_label("report.docx", SystemRenderer::Windows),
            Some("Word document")
        );
    }

    #[test]
    fn sips_and_quick_look_complaints_become_problems() {
        assert_eq!(
            sips_problem("Error: password required to open this PDF"),
            PdfProblem::Password
        );
        assert_eq!(
            sips_problem("\nError 13: no valid image data\n"),
            PdfProblem::Damaged("Error 13: no valid image data".into())
        );
        assert_eq!(
            sips_problem(""),
            PdfProblem::Damaged("the file could not be drawn".into())
        );
    }

    #[test]
    fn the_native_renderer_follows_the_system() {
        assert_eq!(
            SystemRenderer::native(),
            if cfg!(target_os = "macos") {
                Some(SystemRenderer::Mac)
            } else if cfg!(windows) {
                Some(SystemRenderer::Windows)
            } else {
                None
            }
        );
    }
}
