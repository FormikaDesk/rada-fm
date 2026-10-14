//! The files used lately, as a folder-like list ("Recent") like the one of a tag.
//!
//! The view is named by a path that is not on any disk (a tag view with a name nobody can
//! type), so everything a tag's list cannot do, this one cannot either: the files are the
//! system's to say, nothing is made *in* it. Where the list comes from depends on the system:
//!
//! * macOS: Spotlight (`mdfind`, the last-used date of the last 30 days);
//! * Linux: `recently-used.xbel` in the data folder, the file the desktop's programs write;
//! * Windows: the user's `Recent` folder, whose shortcuts (`.lnk`) are read for what they
//!   point at.
//!
//! A source that is not there, or fails, means no entry in the sidebar and no error. What is
//! here parses and tidies; reading the system is the platform's.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::fs::FsEngine;
use crate::model::{self, Entry};
use crate::platform::Platform;
use crate::tagsearch;

/// How many files the list holds.
pub const LIMIT: usize = 200;

/// How far back Spotlight is asked to look.
pub const DAYS: u32 = 30;

/// The name of the view among tags. Starts with a control character: no one types it.
const NAME: &str = "\u{1}recent";

/// The path that stands for the view.
pub fn view_path() -> PathBuf {
    tagsearch::view_path(NAME)
}

/// Whether `path` is the view.
pub fn is_view(path: &Path) -> bool {
    tagsearch::view_name(path).is_some_and(|n| n == NAME)
}

/// One file and when it was last used.
pub type Used = (PathBuf, SystemTime);

/// What a source found, made into the list: only files that are still there (and not
/// hidden, nor in a program's own folder), each once, the newest first, at most [`LIMIT`].
pub fn tidy(found: Vec<Used>, home: &Path) -> Vec<Used> {
    let mut seen = HashSet::new();
    let mut out: Vec<Used> = found
        .into_iter()
        .filter(|(p, _)| p.is_absolute() && !is_program_data(p, home) && p.is_file())
        .filter(|(p, _)| seen.insert(p.clone()))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out.truncate(LIMIT);
    out
}

/// A path the system or a program keeps for itself: a hidden folder or file anywhere below
/// the home folder (`.cache`, `.config`), or the `Library` folder of a Mac.
fn is_program_data(p: &Path, home: &Path) -> bool {
    let Ok(rest) = p.strip_prefix(home) else {
        return false;
    };
    let mut parts = rest.components().map(|c| c.as_os_str().to_string_lossy());
    parts.any(|c| c.starts_with('.')) || rest.starts_with("Library")
}

/// The entries of the view. Each entry's `name` is its full path (two files called `a.txt`
/// in different folders are different rows), its `display` the name on disk, its `used` the
/// last time it was used.
pub fn entries(
    fs: &dyn FsEngine,
    platform: &dyn Platform,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Entry>, String> {
    let Some(found) = platform.recent_files(cancel) else {
        return Err(format!(
            "no list of recent files here ({})",
            platform.name()
        ));
    };
    Ok(tidy(found, &platform.dirs().home)
        .into_iter()
        .filter_map(|(p, at)| {
            let mut e = model::read_entry(fs, platform, &p)?;
            e.name = e.path.as_os_str().to_os_string();
            e.used = Some(at);
            Some(e)
        })
        .collect())
}

// ------------------------------------------------------------------------------ Linux

/// The files of a `recently-used.xbel`: each bookmark's `href` (a `file://` URL), with the
/// latest of its `modified` / `visited` dates and those of the programs that opened it
/// (`added` only when it has no other).
#[cfg(unix)]
pub fn parse_xbel(text: &str) -> Vec<Used> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("<bookmark ") {
        let after = &rest[i..];
        // A bookmark ends at its closing tag, or where the next one begins (one written with
        // no content closes itself).
        let end = [
            after.find("</bookmark>"),
            after[1..].find("<bookmark ").map(|n| n + 1),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(after.len());
        let block = &after[..end];
        rest = &after[end..];
        let Some(head_end) = block.find('>') else {
            continue;
        };
        let head = &block[..head_end];
        let Some(href) = attribute(head, "href") else {
            continue;
        };
        let Some(encoded) = href.strip_prefix("file://") else {
            continue;
        };
        // `file://host/path` is not here; `file:///path` is.
        if !encoded.starts_with('/') {
            continue;
        }
        let path = PathBuf::from(crate::platform::freedesktop::percent_decode(&unescape(
            encoded,
        )));
        let mut used: Vec<SystemTime> = ["modified", "visited"]
            .iter()
            .filter_map(|a| attribute(head, a))
            .filter_map(|v| timestamp(&v))
            .collect();
        // The programs that opened it: `<bookmark:application … modified="…"/>`.
        let mut apps = &block[head_end..];
        while let Some(j) = apps.find("<bookmark:application ") {
            let tag_end = apps[j..].find('>').map_or(apps.len(), |k| j + k);
            if let Some(t) = attribute(&apps[j..tag_end], "modified").and_then(|v| timestamp(&v)) {
                used.push(t);
            }
            apps = &apps[tag_end..];
        }
        if used.is_empty() {
            used.extend(attribute(head, "added").and_then(|v| timestamp(&v)));
        }
        if let Some(at) = used.into_iter().max() {
            out.push((path, at));
        }
    }
    out
}

/// The value of `name="…"` in a tag's text.
#[cfg(unix)]
fn attribute(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    let len = tag[at..].find('"')?;
    Some(tag[at..at + len].to_string())
}

#[cfg(unix)]
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// An ISO 8601 date (`2026-10-12T08:30:11Z`, `…11.25Z`, with an offset) as a point in time.
#[cfg(unix)]
fn timestamp(s: &str) -> Option<SystemTime> {
    s.parse::<jiff::Timestamp>().ok().map(SystemTime::from)
}

// ------------------------------------------------------------------------------ macOS

/// The arguments for `mdfind`: what was used in the last [`DAYS`] days under `home`, printed
/// with the date.
pub fn mdfind_args(home: &Path) -> Vec<String> {
    vec![
        "-onlyin".into(),
        home.to_string_lossy().into_owned(),
        "-attr".into(),
        "kMDItemLastUsedDate".into(),
        format!("kMDItemLastUsedDate >= $time.today(-{DAYS})"),
    ]
}

/// The arguments that only count (to see that Spotlight answers).
pub fn mdfind_probe_args(home: &Path) -> Vec<String> {
    vec![
        "-onlyin".into(),
        home.to_string_lossy().into_owned(),
        "-count".into(),
        format!("kMDItemLastUsedDate >= $time.today(-{DAYS})"),
    ]
}

/// Whether `mdfind -count` answered with a number.
pub fn probe_answer_is_a_count(out: &str) -> bool {
    out.trim().parse::<u64>().is_ok()
}

/// The lines `mdfind -attr kMDItemLastUsedDate` prints: `<path>   kMDItemLastUsedDate = <date>`.
/// A line with a date that cannot be read is kept with `None`, and the caller falls back to
/// the file's own dates; lines that are no path are left out.
pub fn parse_mdfind_attr(out: &str) -> Vec<(PathBuf, Option<SystemTime>)> {
    const KEY: &str = "kMDItemLastUsedDate";
    out.lines()
        .filter(|l| l.starts_with('/'))
        .filter_map(|l| {
            let at = l.rfind(KEY)?;
            let path = l[..at].trim_end();
            let value = l[at + KEY.len()..].trim_start().strip_prefix('=')?.trim();
            (!path.is_empty()).then(|| (PathBuf::from(path), mac_date(value)))
        })
        .collect()
}

/// `2026-10-12 08:30:11 +0000`, as `mdfind` writes a date.
fn mac_date(s: &str) -> Option<SystemTime> {
    if s == "(null)" {
        return None;
    }
    jiff::Timestamp::strptime("%Y-%m-%d %H:%M:%S %z", s)
        .ok()
        .map(SystemTime::from)
}

// ------------------------------------------------------------------------------ Windows

/// What a shortcut (`.lnk`) of the Recent folder points at, read from the file itself (the
/// format of MS-SHLLINK), without the shell: the local path of its target. Shortcuts to
/// network shares, to things that are not files and anything unreadable give `None`.
pub fn lnk_target(b: &[u8]) -> Option<PathBuf> {
    let u16le = |at: usize| -> Option<usize> {
        Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
    };
    let u32le = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    if u32le(0)? != 0x4C {
        return None;
    }
    let flags = u32le(20)?;
    let mut at = 76;
    if flags & 0x1 != 0 {
        // The list of the target's identifiers: skip it.
        at += 2 + u16le(at)?;
    }
    if flags & 0x2 == 0 {
        return None;
    }
    let info = at;
    let header = u32le(info + 4)?;
    let info_flags = u32le(info + 8)?;
    // The local path is only there when the flag says so.
    if info_flags & 0x1 == 0 {
        return None;
    }
    let ansi = |off: usize| -> Option<String> {
        let start = info.checked_add(off)?;
        let tail = b.get(start..)?;
        let n = tail.iter().position(|c| *c == 0)?;
        Some(String::from_utf8_lossy(&tail[..n]).into_owned())
    };
    let wide = |off: usize| -> Option<String> {
        let start = info.checked_add(off)?;
        let tail = b.get(start..)?;
        let units: Vec<u16> = tail
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .take_while(|u| *u != 0)
            .collect();
        String::from_utf16(&units).ok()
    };
    let (base, suffix) = if header >= 0x24 {
        let (bo, so) = (u32le(info + 28)?, u32le(info + 32)?);
        (wide(bo)?, wide(so).unwrap_or_default())
    } else {
        let (bo, so) = (u32le(info + 16)?, u32le(info + 24)?);
        (ansi(bo)?, ansi(so).unwrap_or_default())
    };
    let path = format!("{base}{suffix}");
    // A drive letter and a colon, or it is not a path of a disk.
    let ok =
        path.len() > 3 && path.as_bytes()[1] == b':' && path.as_bytes()[0].is_ascii_alphabetic();
    ok.then(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn the_view_is_a_path_nobody_can_type_and_not_a_tag() {
        let p = view_path();
        assert!(is_view(&p));
        assert!(!is_view(&tagsearch::view_path("Blu")));
        assert!(!is_view(Path::new("/home/u/recent")));
        // It is a tag view for everything a tag's list cannot do.
        assert!(tagsearch::view_name(&p).is_some());
    }

    #[test]
    fn the_list_is_files_that_exist_once_each_newest_first_and_not_program_data() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        for d in ["docs", ".cache", "Library/Preferences"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        let make = |rel: &str| {
            let p = home.join(rel);
            std::fs::write(&p, "x").unwrap();
            p
        };
        let (a, b, c) = (make("docs/a.txt"), make("docs/b.txt"), make("docs/c.txt"));
        let hidden = make(".cache/x.db");
        let lib = make("Library/Preferences/x.plist");
        let gone = home.join("docs/gone.txt");
        let folder = home.join("docs");
        let found = vec![
            (a.clone(), at(100)),
            (b.clone(), at(300)),
            (b.clone(), at(50)),
            (gone, at(999)),
            (folder, at(998)),
            (hidden, at(997)),
            (lib, at(996)),
            (c.clone(), at(200)),
            (PathBuf::from("relative.txt"), at(995)),
        ];
        let out = tidy(found, &home);
        let paths: Vec<_> = out.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(paths, [b, c, a]);
    }

    #[test]
    fn at_most_two_hundred_are_kept() {
        let t = tempfile::tempdir().unwrap();
        let found: Vec<Used> = (0..LIMIT + 30)
            .map(|i| {
                let p = t.path().join(format!("f{i}.txt"));
                std::fs::write(&p, "x").unwrap();
                (p, at(i as u64))
            })
            .collect();
        let out = tidy(found, t.path());
        assert_eq!(out.len(), LIMIT);
        assert_eq!(out[0].1, at((LIMIT + 29) as u64), "the newest first");
    }

    #[cfg(unix)]
    #[test]
    fn a_recently_used_file_is_read_with_the_latest_date_of_each_bookmark() {
        let xbel = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
  <bookmark href="file:///home/u/My%20Docs/a%20%26%20b.txt" added="2026-10-01T10:00:00Z" modified="2026-10-02T10:00:00Z" visited="2026-10-03T10:00:00Z">
    <info><metadata owner="http://freedesktop.org">
      <bookmark:applications>
        <bookmark:application name="Editor" exec="&apos;editor %u&apos;" modified="2026-10-05T10:00:00.250Z" count="2"/>
      </bookmark:applications>
    </metadata></info>
  </bookmark>
  <bookmark href="file:///home/u/only-added.txt" added="2026-09-01T00:00:00Z" modified="2026-09-01T00:00:00Z"/>
  <bookmark href="https://example.org/x" added="2026-10-09T00:00:00Z" modified="2026-10-09T00:00:00Z"/>
  <bookmark href="file://otherhost/x" added="2026-10-09T00:00:00Z" modified="2026-10-09T00:00:00Z"/>
  <bookmark href="file:///home/u/no-date.txt"/>
  <bookmark added="2026-10-09T00:00:00Z"/>
</xbel>"#;
        let got = parse_xbel(xbel);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].0, PathBuf::from("/home/u/My Docs/a & b.txt"));
        let want = "2026-10-05T10:00:00.250Z"
            .parse::<jiff::Timestamp>()
            .unwrap();
        assert_eq!(
            got[0].1,
            SystemTime::from(want),
            "the program's date is the latest"
        );
        assert_eq!(got[1].0, PathBuf::from("/home/u/only-added.txt"));
        assert!(parse_xbel("not xml at all").is_empty());
        assert!(parse_xbel("<bookmark href=\"file:///x\" modified=\"junk\"").is_empty());
    }

    /// The whole way on the sandbox's own system: a list in the data folder, the files that
    /// still exist, newest first, each row named by its path and carrying its date.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_list_of_this_system_becomes_entries_with_their_dates() {
        use crate::testutil::Sandbox;
        let sb = Sandbox::new();
        let a = sb.write("docs/a.txt", "a");
        let b = sb.write("work/b.txt", "bb");
        let gone = sb.path("docs/gone.txt");
        let platform = sb.platform();
        // No list yet: nothing to show, and the sidebar has no entry.
        assert!(!platform.recent_available());
        assert!(entries(&crate::fs::LocalFs, platform.as_ref(), &|| false).is_err());
        let line = |p: &Path, when: &str| {
            format!(
                "<bookmark href=\"file://{}\" added=\"{when}\" modified=\"{when}\" visited=\"{when}\"/>\n",
                p.display()
            )
        };
        let xbel = format!(
            "<?xml version=\"1.0\"?>\n<xbel version=\"1.0\">\n{}{}{}</xbel>\n",
            line(&a, "2026-10-01T10:00:00Z"),
            line(&b, "2026-10-09T10:00:00Z"),
            line(&gone, "2026-10-10T10:00:00Z"),
        );
        std::fs::create_dir_all(&sb.dirs.data).unwrap();
        std::fs::write(sb.dirs.data.join("recently-used.xbel"), xbel).unwrap();
        assert!(platform.recent_available());
        let got = entries(&crate::fs::LocalFs, platform.as_ref(), &|| false).unwrap();
        let paths: Vec<_> = got.iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths,
            [b.clone(), a],
            "newest first, the deleted one is gone"
        );
        assert_eq!(got[0].name, b.as_os_str());
        assert_eq!(got[0].display, "b.txt");
        let want = "2026-10-09T10:00:00Z".parse::<jiff::Timestamp>().unwrap();
        assert_eq!(got[0].used, Some(SystemTime::from(want)));
    }

    #[test]
    fn what_spotlight_prints_is_read_with_its_dates() {
        let out = "/Users/me/Documents/a b.pdf   kMDItemLastUsedDate = 2026-10-12 08:30:11 +0000\n\
                   /Users/me/c.txt kMDItemLastUsedDate = (null)\n\
                   /Users/me/d.txt kMDItemLastUsedDate = nonsense\n\
                   Spotlight is warming up\n\
                   /Users/me/e.txt\n";
        let got = parse_mdfind_attr(out);
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(got[0].0, PathBuf::from("/Users/me/Documents/a b.pdf"));
        let want = "2026-10-12T08:30:11Z".parse::<jiff::Timestamp>().unwrap();
        assert_eq!(got[0].1, Some(SystemTime::from(want)));
        assert_eq!(got[1].1, None);
        assert_eq!(got[2].1, None);
        let args = mdfind_args(Path::new("/Users/me"));
        assert_eq!(args[0], "-onlyin");
        assert!(args.last().unwrap().contains("$time.today(-30)"));
        assert!(probe_answer_is_a_count("17\n"));
        assert!(!probe_answer_is_a_count("The Spotlight server is disabled"));
    }

    /// A shortcut the way Windows writes it, with the local path in both forms.
    fn lnk(base: &str, suffix: &str, local: bool, unicode: bool) -> Vec<u8> {
        let mut b = vec![0u8; 76];
        b[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
        // HasLinkTargetIDList | HasLinkInfo
        b[20..24].copy_from_slice(&3u32.to_le_bytes());
        // An identifier list of 4 bytes.
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&[0, 0, 0, 0]);
        let header = if unicode { 0x24u32 } else { 0x1C };
        let mut tail = Vec::new();
        let base_off = header as usize;
        let mut data = Vec::new();
        let (bo, so);
        if unicode {
            bo = base_off;
            for u in base.encode_utf16().chain(Some(0)) {
                data.extend_from_slice(&u.to_le_bytes());
            }
            so = base_off + data.len();
            for u in suffix.encode_utf16().chain(Some(0)) {
                data.extend_from_slice(&u.to_le_bytes());
            }
        } else {
            bo = base_off;
            data.extend_from_slice(base.as_bytes());
            data.push(0);
            so = base_off + data.len();
            data.extend_from_slice(suffix.as_bytes());
            data.push(0);
        }
        tail.extend_from_slice(&(header + data.len() as u32).to_le_bytes());
        tail.extend_from_slice(&header.to_le_bytes());
        tail.extend_from_slice(&u32::from(local).to_le_bytes());
        tail.extend_from_slice(&0u32.to_le_bytes()); // volume id
        let (ansi_base, ansi_suffix) = if unicode { (0, 0) } else { (bo, so) };
        tail.extend_from_slice(&(ansi_base as u32).to_le_bytes());
        tail.extend_from_slice(&0u32.to_le_bytes()); // network
        tail.extend_from_slice(&(ansi_suffix as u32).to_le_bytes());
        if unicode {
            tail.extend_from_slice(&(bo as u32).to_le_bytes());
            tail.extend_from_slice(&(so as u32).to_le_bytes());
        }
        tail.extend_from_slice(&data);
        b.extend_from_slice(&tail);
        b
    }

    #[test]
    fn a_shortcut_gives_the_local_path_it_points_at() {
        for unicode in [false, true] {
            let b = lnk("C:\\Users\\me\\Docs\\", "relatorio.txt", true, unicode);
            assert_eq!(
                lnk_target(&b).unwrap(),
                PathBuf::from("C:\\Users\\me\\Docs\\relatorio.txt"),
                "unicode {unicode}"
            );
            if unicode {
                let accented = lnk("C:\\Docs\\", "relatório.txt", true, true);
                assert_eq!(
                    lnk_target(&accented).unwrap(),
                    PathBuf::from("C:\\Docs\\relatório.txt")
                );
            }
            let plain = lnk("D:\\a\\", "b.txt", true, unicode);
            assert_eq!(
                lnk_target(&plain).unwrap(),
                PathBuf::from("D:\\a\\b.txt"),
                "unicode {unicode}"
            );
            // No local path (a network share), or not a path of a disk.
            assert_eq!(
                lnk_target(&lnk("\\\\srv\\share\\", "b.txt", false, unicode)),
                None
            );
            assert_eq!(lnk_target(&lnk("srv", "", true, unicode)), None);
        }
    }

    #[test]
    fn a_damaged_shortcut_is_nothing_and_never_a_panic() {
        let good = lnk("C:\\a\\", "b.txt", true, true);
        assert!(lnk_target(&good).is_some());
        for n in 0..good.len() {
            let _ = lnk_target(&good[..n]);
        }
        let mut wrong = good.clone();
        wrong[0] = 0x4D;
        assert_eq!(lnk_target(&wrong), None);
        let mut wild = good.clone();
        let l = wild.len();
        wild[82..86].copy_from_slice(&u32::MAX.to_le_bytes());
        wild[l - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
        let _ = lnk_target(&wild);
        assert_eq!(lnk_target(&[]), None);
    }
}
