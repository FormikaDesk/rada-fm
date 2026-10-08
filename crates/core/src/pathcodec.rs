//! Lossless (de)serialisation of paths and `OsString`s.
//!
//! Journal data must round-trip file names that are not valid UTF-8, so names are
//! stored either as a plain JSON string (the common case) or as a hex byte string.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Repr {
    Text(String),
    Bytes { hex: String },
}

fn to_repr(s: &OsStr) -> Repr {
    match s.to_str() {
        Some(t) => Repr::Text(t.to_string()),
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                Repr::Bytes {
                    hex: hex(s.as_bytes()),
                }
            }
            #[cfg(not(unix))]
            {
                Repr::Text(s.to_string_lossy().into_owned())
            }
        }
    }
}

fn from_repr(r: Repr) -> Result<OsString, String> {
    match r {
        Repr::Text(t) => Ok(OsString::from(t)),
        Repr::Bytes { hex: h } => {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt;
                Ok(OsString::from_vec(unhex(&h)?))
            }
            #[cfg(not(unix))]
            {
                Err(format!(
                    "byte path {h:?} cannot be represented on this platform"
                ))
            }
        }
    }
}

#[cfg_attr(not(unix), allow(dead_code))]
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg_attr(not(unix), allow(dead_code))]
fn unhex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex string".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

/// `#[serde(with = "pathcodec::path")]` for `PathBuf` fields.
pub mod path {
    use super::*;

    pub fn serialize<S: Serializer>(p: &Path, s: S) -> Result<S::Ok, S::Error> {
        to_repr(p.as_os_str()).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PathBuf, D::Error> {
        let r = Repr::deserialize(d)?;
        from_repr(r)
            .map(PathBuf::from)
            .map_err(serde::de::Error::custom)
    }
}

/// `#[serde(with = "pathcodec::os")]` for `OsString` fields.
pub mod os {
    use super::*;

    pub fn serialize<S: Serializer>(p: &OsStr, s: S) -> Result<S::Ok, S::Error> {
        to_repr(p).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<OsString, D::Error> {
        let r = Repr::deserialize(d)?;
        from_repr(r).map_err(serde::de::Error::custom)
    }
}

/// `#[serde(with = "pathcodec::opt_path")]` for `Option<PathBuf>`.
pub mod opt_path {
    use super::*;

    pub fn serialize<S: Serializer>(p: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
        p.as_ref().map(|p| to_repr(p.as_os_str())).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PathBuf>, D::Error> {
        match Option::<Repr>::deserialize(d)? {
            None => Ok(None),
            Some(r) => from_repr(r)
                .map(|o| Some(PathBuf::from(o)))
                .map_err(serde::de::Error::custom),
        }
    }
}

/// `#[serde(with = "pathcodec::paths")]` for `Vec<PathBuf>`.
pub mod paths {
    use super::*;

    pub fn serialize<S: Serializer>(v: &[PathBuf], s: S) -> Result<S::Ok, S::Error> {
        v.iter()
            .map(|p| to_repr(p.as_os_str()))
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PathBuf>, D::Error> {
        Vec::<Repr>::deserialize(d)?
            .into_iter()
            .map(|r| {
                from_repr(r)
                    .map(PathBuf::from)
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}

/// `#[serde(with = "pathcodec::path_pairs")]` for `Vec<(PathBuf, PathBuf)>`.
pub mod path_pairs {
    use super::*;

    pub fn serialize<S: Serializer>(v: &[(PathBuf, PathBuf)], s: S) -> Result<S::Ok, S::Error> {
        v.iter()
            .map(|(a, b)| (to_repr(a.as_os_str()), to_repr(b.as_os_str())))
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<(PathBuf, PathBuf)>, D::Error> {
        Vec::<(Repr, Repr)>::deserialize(d)?
            .into_iter()
            .map(|(a, b)| {
                let a = from_repr(a).map_err(serde::de::Error::custom)?;
                let b = from_repr(b).map_err(serde::de::Error::custom)?;
                Ok((PathBuf::from(a), PathBuf::from(b)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct W(#[serde(with = "super::path")] PathBuf);

    #[test]
    fn utf8_round_trip_is_plain_text() {
        let w = W(PathBuf::from("/a/b ü 🎉"));
        let j = serde_json::to_string(&w).unwrap();
        assert_eq!(j, "\"/a/b ü 🎉\"");
        assert_eq!(serde_json::from_str::<W>(&j).unwrap(), w);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_round_trip() {
        use std::os::unix::ffi::OsStringExt;
        let w = W(PathBuf::from(OsString::from_vec(vec![
            b'/', b'x', 0xff, 0xfe, b'y',
        ])));
        let j = serde_json::to_string(&w).unwrap();
        assert!(j.contains("hex"));
        assert_eq!(serde_json::from_str::<W>(&j).unwrap(), w);
    }
}
