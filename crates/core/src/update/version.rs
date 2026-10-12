//! Version numbers with the pre-release part (`0.6.0-rc.2`), ordered the way semver orders
//! them: `0.6.0-rc.1 < 0.6.0-rc.2 < 0.6.0 < 0.6.1`.

use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ident {
    Num(u64),
    Text(String),
}

impl Ord for Ident {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Ident::Num(a), Ident::Num(b)) => a.cmp(b),
            (Ident::Num(_), Ident::Text(_)) => Ordering::Less,
            (Ident::Text(_), Ident::Num(_)) => Ordering::Greater,
            (Ident::Text(a), Ident::Text(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for Ident {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pre: Vec<Ident>,
}

impl Version {
    /// `1.2.3`, `v1.2.3`, `1.2.3-rc.1`; build metadata (`+abc`) is ignored.
    pub fn parse(text: &str) -> Option<Version> {
        let t = text.trim().trim_start_matches('v');
        let t = t.split('+').next()?;
        let (core, pre) = match t.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (t, None),
        };
        let mut nums = core.split('.');
        let major = nums.next()?.parse().ok()?;
        let minor = nums.next()?.parse().ok()?;
        let patch = nums.next()?.parse().ok()?;
        if nums.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(p) => {
                if p.is_empty() {
                    return None;
                }
                p.split('.')
                    .map(|id| {
                        if id.is_empty()
                            || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                        {
                            None
                        } else if id.chars().all(|c| c.is_ascii_digit()) {
                            id.parse().ok().map(Ident::Num)
                        } else {
                            Some(Ident::Text(id.to_string()))
                        }
                    })
                    .collect::<Option<Vec<_>>>()?
            }
        };
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // A release comes after its own pre-releases.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            f.write_str("-")?;
            for (i, id) in self.pre.iter().enumerate() {
                if i > 0 {
                    f.write_str(".")?;
                }
                match id {
                    Ident::Num(n) => write!(f, "{n}")?,
                    Ident::Text(t) => f.write_str(t)?,
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("{s} does not parse"))
    }

    #[test]
    fn versions_are_ordered_like_semver() {
        let order = [
            "0.5.0",
            "0.6.0-rc.1",
            "0.6.0-rc.2",
            "0.6.0-rc.10",
            "0.6.0",
            "0.6.1-beta",
            "0.6.1",
            "0.10.0",
            "1.0.0",
        ];
        for pair in order.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(v("v0.6.0"), v("0.6.0+build5"));
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"));
        assert!(v("1.0.0-alpha.1") < v("1.0.0-alpha.beta"));
        assert!(v("1.0.0-2") < v("1.0.0-alpha"));
    }

    #[test]
    fn things_that_are_not_versions_do_not_parse() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "1.2.3-",
            "1.2.3-a..b",
            "nightly",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_version_is_written_back_as_it_was() {
        for s in ["0.6.0", "0.6.0-rc.2", "1.2.3-alpha.1.x"] {
            assert_eq!(v(s).to_string(), s);
        }
        assert!(v("0.6.0-rc.2").is_prerelease());
        assert!(!v("0.6.0").is_prerelease());
    }
}
