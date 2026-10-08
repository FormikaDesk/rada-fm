//! File-name rules. Everything here works on a single name component (`OsStr`), never
//! on a whole path: splitting an extension off a path string is what broke
//! superfile in folders like `v1.2/` (B1).

use std::ffi::{OsStr, OsString};
use std::path::Path;

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
        return Err(format!("the name is {len} bytes long (limit {})", rules.max_name_bytes));
    }
    if !rules.reserved_names.is_empty() {
        let stem = text.split('.').next().unwrap_or("").to_ascii_uppercase();
        if rules.reserved_names.contains(&stem.as_str()) {
            return Err(format!("{stem} is a reserved name"));
        }
        if text.ends_with(' ') || text.ends_with('.') {
            return Err("a name cannot end with a space or a dot".into());
        }
    }
    Ok(())
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
    if let Some(inner) = Path::new(&stem).extension() {
        if inner.eq_ignore_ascii_case("tar") {
            let base = Path::new(&stem).file_stem().map(OsStr::to_os_string).unwrap_or_default();
            let mut full = OsString::from(".");
            full.push(inner);
            full.push(&ext_full);
            stem = base;
            ext_full = full;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PathRules;

    #[test]
    fn extension_splitting_works_on_names_only() {
        let s = |n: &str| {
            let (a, b) = split_ext(OsStr::new(n));
            (a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned())
        };
        assert_eq!(s("a.txt"), ("a".into(), ".txt".into()));
        assert_eq!(s("archive.tar.gz"), ("archive".into(), ".tar.gz".into()));
        assert_eq!(s(".bashrc"), (".bashrc".into(), "".into()));
        assert_eq!(s("noext"), ("noext".into(), "".into()));
        assert_eq!(s("v1.2"), ("v1".into(), ".2".into()));
    }

    #[test]
    fn numbering() {
        assert_eq!(numbered(OsStr::new("a.txt"), 2), OsString::from("a (2).txt"));
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
