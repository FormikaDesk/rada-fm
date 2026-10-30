//! File-name rules. Everything here works on a single name component (`OsStr`), never
//! on a whole path: splitting an extension off a path string is what broke
//! other file managers in folders like `v1.2/`.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::fs::FsEngine;
use crate::platform::PathRules;

/// `Ok` if `name` can be created as a single directory entry on this platform.
pub fn validate(rules: &PathRules, name: &OsStr) -> Result<(), String> {
    if name.is_empty() {
        return Err("the name is empty".into());
    }
    if name == "." || name == ".." {
        return Err(format!("{:?} is not a valid name", name.to_string_lossy()));
    }
    #[cfg(unix)]
    let len = {
        use std::os::unix::ffi::OsStrExt;
        let bytes = name.as_bytes();
        if bytes.contains(&0) || bytes.contains(&b'/') {
            return Err("a name cannot contain '/' or NUL".into());
        }
        bytes.len()
    };
    #[cfg(not(unix))]
    let len = name.len();
    let text = name.to_string_lossy();
    if let Some(c) = text.chars().find(|c| rules.forbidden_chars.contains(c)) {
        return Err(format!("a name cannot contain {c:?}"));
    }
    if len > rules.max_name_bytes {
        return Err(format!(
            "the name is {len} bytes long (limit {})",
            rules.max_name_bytes
        ));
    }
    if !rules.reserved_names.is_empty() {
        let stem = device_stem(&text);
        if rules.reserved_names.contains(&stem.as_str()) {
            return Err(format!("{stem} is a reserved name"));
        }
        if text.ends_with(' ') || text.ends_with('.') {
            return Err("a name cannot end with a space or a dot".into());
        }
    }
    Ok(())
}

/// What Windows looks at to tell a device from a file: the name up to the first dot, without
/// the spaces before it, in capitals, with `¹ ² ³` read as `1 2 3` (they name devices too).
fn device_stem(text: &str) -> String {
    text.split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .chars()
        .map(|c| match c {
            '\u{b9}' => '1',
            '\u{b2}' => '2',
            '\u{b3}' => '3',
            c => c.to_ascii_uppercase(),
        })
        .collect()
}

/// Whether `to` is the name of `from` spelled with other capitals, on a filesystem that ignores
/// case: the same folder, names equal but for case, the same file under both, and a folder that
/// lists the one name `from` has and not `to`. Where case matters, two such names are two files
/// (or two links to one) and both are listed.
pub fn is_case_alias(fs: &dyn FsEngine, from: &Path, to: &Path) -> bool {
    let (Some(dir), Some(a), Some(b)) = (from.parent(), from.file_name(), to.file_name()) else {
        return false;
    };
    let lower = |n: &OsStr| n.to_string_lossy().to_lowercase();
    if to.parent() != Some(dir) || a == b || lower(a) != lower(b) {
        return false;
    }
    let (Ok(x), Ok(y)) = (fs.lstat(from), fs.lstat(to)) else {
        return false;
    };
    let one_file = x.ino.is_some() && x.ino == y.ino && x.dev == y.dev;
    one_file
        && fs
            .read_dir(dir)
            .is_ok_and(|l| l.items.iter().all(|i| i.name != b))
}

/// Split a *file name* into stem and extension (with its dot). `.tar.gz` stays together
/// and a leading dot is not an extension separator.
pub fn split_ext(name: &OsStr) -> (OsString, OsString) {
    let p = Path::new(name);
    let (Some(stem), Some(ext)) = (p.file_stem(), p.extension()) else {
        return (name.to_os_string(), OsString::new());
    };
    let mut ext_full = OsString::from(".");
    ext_full.push(ext);
    let mut stem = stem.to_os_string();
    // archive.tar.gz -> ("archive", ".tar.gz")
    if let Some(inner) = Path::new(&stem).extension()
        && inner.eq_ignore_ascii_case("tar")
    {
        let base = Path::new(&stem)
            .file_stem()
            .map(OsStr::to_os_string)
            .unwrap_or_default();
        let mut full = OsString::from(".");
        full.push(inner);
        full.push(&ext_full);
        stem = base;
        ext_full = full;
    }
    (stem, ext_full)
}

/// `name (n).ext`
pub fn numbered(name: &OsStr, n: u32) -> OsString {
    let (stem, ext) = split_ext(name);
    let mut out = stem;
    out.push(format!(" ({n})"));
    out.push(ext);
    out
}

/// The first `name (n).ext` in `parent` that `taken` does not claim.
pub fn first_free(parent: &Path, name: &OsStr, taken: impl Fn(&Path) -> bool) -> PathBuf {
    (1u32..)
        .map(|n| parent.join(numbered(name, n)))
        .find(|candidate| !taken(candidate))
        .expect("u32 range exhausted")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PathRules;

    #[test]
    fn extension_splitting_works_on_names_only() {
        let s = |n: &str| {
            let (a, b) = split_ext(OsStr::new(n));
            (
                a.to_string_lossy().into_owned(),
                b.to_string_lossy().into_owned(),
            )
        };
        assert_eq!(s("a.txt"), ("a".into(), ".txt".into()));
        assert_eq!(s("archive.tar.gz"), ("archive".into(), ".tar.gz".into()));
        assert_eq!(s(".bashrc"), (".bashrc".into(), "".into()));
        assert_eq!(s("noext"), ("noext".into(), "".into()));
        assert_eq!(s("v1.2"), ("v1".into(), ".2".into()));
    }

    #[test]
    fn numbering() {
        assert_eq!(
            numbered(OsStr::new("a.txt"), 2),
            OsString::from("a (2).txt")
        );
        assert_eq!(numbered(OsStr::new("dir"), 1), OsString::from("dir (1)"));
    }

    #[test]
    fn validation() {
        let r = PathRules::POSIX;
        assert!(validate(&r, OsStr::new("ok name 🎉.txt")).is_ok());
        assert!(validate(&r, OsStr::new("")).is_err());
        assert!(validate(&r, OsStr::new("a/b")).is_err());
        assert!(validate(&r, OsStr::new("..")).is_err());
        assert!(validate(&r, OsStr::new(&"x".repeat(300))).is_err());
    }
}
