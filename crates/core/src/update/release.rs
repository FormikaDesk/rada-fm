//! What GitHub says about the releases, which one is wanted, and which file of it is for
//! this system.

use serde::Deserialize;

use super::Channel;
use super::version::Version;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    pub prerelease: bool,
    /// The release notes (Markdown, as written on GitHub).
    pub notes: String,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

/// The releases in the answer of the `releases` API. Drafts and tags that are not versions
/// are left out.
pub fn parse_releases(json: &str) -> Result<Vec<Release>, String> {
    let raw: Vec<RawRelease> =
        serde_json::from_str(json).map_err(|e| format!("not a list of releases: {e}"))?;
    Ok(raw
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let version = Version::parse(&r.tag_name)?;
            Some(Release {
                prerelease: r.prerelease || version.is_prerelease(),
                version,
                tag: r.tag_name,
                notes: r.body.unwrap_or_default(),
                assets: r
                    .assets
                    .into_iter()
                    .map(|a| Asset {
                        name: a.name,
                        url: a.browser_download_url,
                        size: a.size,
                    })
                    .collect(),
            })
        })
        .collect())
}

/// The channel a version follows when the choice is `auto`: whoever runs a pre-release also
/// sees the next ones, whoever runs a stable version only sees stable ones.
pub fn resolve_channel(choice: Channel, current: &Version) -> Channel {
    match choice {
        Channel::Auto if current.is_prerelease() => Channel::Prerelease,
        Channel::Auto => Channel::Stable,
        c => c,
    }
}

/// The newest release the channel allows that is newer than `current`.
pub fn newest<'a>(
    releases: &'a [Release],
    current: &Version,
    channel: Channel,
) -> Option<&'a Release> {
    let channel = resolve_channel(channel, current);
    releases
        .iter()
        .filter(|r| channel == Channel::Prerelease || !r.prerelease)
        .filter(|r| r.version > *current)
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// The target triple the release files are named after, for a system.
///
/// Linux binaries are static (musl), so they serve every distribution. On a Mac the program
/// runs as it was built, except under Rosetta, where the file for the machine itself is
/// wanted: an Intel build on Apple silicon is only a stopgap.
pub fn target_for(os: &str, arch: &str, translated: bool) -> Option<String> {
    let arch = match (os, arch, translated) {
        ("macos", "x86_64", true) => "aarch64",
        (_, "x86_64" | "x64", _) => "x86_64",
        (_, "aarch64" | "arm64", _) => "aarch64",
        _ => return None,
    };
    match (os, arch) {
        ("linux", a) => Some(format!("{a}-unknown-linux-musl")),
        ("macos", a) => Some(format!("{a}-apple-darwin")),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc".into()),
        _ => None,
    }
}

/// The name of the archive for a version and target, as the release workflow writes it.
pub fn archive_name(version: &Version, target: &str) -> String {
    let ext = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("rada-{version}-{target}.{ext}")
}

/// The folder inside the archive, and the program in it.
pub fn inner_path(version: &Version, target: &str) -> String {
    let exe = if target.contains("windows") {
        "rada.exe"
    } else {
        "rada"
    };
    format!("rada-{version}-{target}/{exe}")
}

/// The checksum a `SHA256SUMS` file gives for `name`: lines of `<hex>  <name>` (a `*` before
/// the name marks binary mode).
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, pre: bool) -> Release {
        Release {
            version: Version::parse(tag).unwrap(),
            tag: tag.into(),
            prerelease: pre,
            notes: String::new(),
            assets: Vec::new(),
        }
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn the_api_answer_is_read_and_drafts_are_left_out() {
        let json = r#"[
            {"tag_name":"v0.6.0-rc.2","prerelease":true,"draft":false,"body":"Notes","assets":[
              {"name":"SHA256SUMS","browser_download_url":"https://x/SHA256SUMS","size":10}]},
            {"tag_name":"v0.7.0","prerelease":false,"draft":true,"body":null,"assets":[]},
            {"tag_name":"nightly","prerelease":true,"draft":false,"body":"","assets":[]},
            {"tag_name":"v0.5.0","body":"Old","assets":[]}
        ]"#;
        let got = parse_releases(json).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].tag, "v0.6.0-rc.2");
        assert!(got[0].prerelease);
        assert_eq!(got[0].notes, "Notes");
        assert_eq!(got[0].asset("SHA256SUMS").unwrap().size, Some(10));
        assert!(!got[1].prerelease);
        assert!(parse_releases("{}").is_err());
        assert!(parse_releases("<html>").is_err());
    }

    #[test]
    fn a_stable_version_only_sees_stable_releases() {
        let all = [
            release("0.6.0-rc.2", true),
            release("0.5.1", false),
            release("0.6.0", false),
            release("0.7.0-rc.1", true),
        ];
        let got = newest(&all, &v("0.5.0"), Channel::Auto).unwrap();
        assert_eq!(got.tag, "0.6.0");
        assert!(newest(&all, &v("0.6.0"), Channel::Auto).is_none());
    }

    #[test]
    fn a_prerelease_also_sees_the_next_prereleases() {
        let all = [
            release("0.6.0-rc.1", true),
            release("0.6.0-rc.2", true),
            release("0.5.0", false),
        ];
        assert_eq!(
            newest(&all, &v("0.6.0-rc.1"), Channel::Auto).unwrap().tag,
            "0.6.0-rc.2"
        );
        assert!(newest(&all, &v("0.6.0-rc.2"), Channel::Auto).is_none());
        // Chosen by hand, the channel wins over what the running version is.
        assert!(newest(&all, &v("0.6.0-rc.1"), Channel::Stable).is_none());
        assert_eq!(
            newest(&all, &v("0.4.0"), Channel::Prerelease).unwrap().tag,
            "0.6.0-rc.2"
        );
    }

    #[test]
    fn the_release_after_a_prerelease_is_offered_to_its_users() {
        let all = [release("0.6.0-rc.2", true), release("0.6.0", false)];
        assert_eq!(
            newest(&all, &v("0.6.0-rc.2"), Channel::Auto).unwrap().tag,
            "0.6.0"
        );
    }

    #[test]
    fn each_system_gets_the_file_built_for_it() {
        let t = |os, arch, tr| target_for(os, arch, tr);
        assert_eq!(
            t("linux", "x86_64", false).as_deref(),
            Some("x86_64-unknown-linux-musl")
        );
        assert_eq!(
            t("linux", "aarch64", false).as_deref(),
            Some("aarch64-unknown-linux-musl")
        );
        assert_eq!(
            t("macos", "x86_64", false).as_deref(),
            Some("x86_64-apple-darwin")
        );
        assert_eq!(
            t("macos", "aarch64", false).as_deref(),
            Some("aarch64-apple-darwin")
        );
        // An Intel build running on Apple silicon is moved to the native one.
        assert_eq!(
            t("macos", "x86_64", true).as_deref(),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(
            t("windows", "x86_64", false).as_deref(),
            Some("x86_64-pc-windows-msvc")
        );
        assert_eq!(t("windows", "aarch64", false), None);
        assert_eq!(t("freebsd", "x86_64", false), None);
        assert_eq!(t("linux", "riscv64", false), None);
    }

    #[test]
    fn archive_names_follow_the_release_workflow() {
        let ver = v("0.6.0-rc.2");
        assert_eq!(
            archive_name(&ver, "x86_64-apple-darwin"),
            "rada-0.6.0-rc.2-x86_64-apple-darwin.tar.gz"
        );
        assert_eq!(
            archive_name(&ver, "x86_64-pc-windows-msvc"),
            "rada-0.6.0-rc.2-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            inner_path(&ver, "x86_64-pc-windows-msvc"),
            "rada-0.6.0-rc.2-x86_64-pc-windows-msvc/rada.exe"
        );
        assert_eq!(
            inner_path(&ver, "aarch64-apple-darwin"),
            "rada-0.6.0-rc.2-aarch64-apple-darwin/rada"
        );
    }

    #[test]
    fn the_checksum_of_a_file_is_found_by_its_name() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let sums = format!("{a}  rada-1.tar.gz\n{b} *rada-2.zip\nnot a line\n{a}  other\n");
        assert_eq!(checksum_for(&sums, "rada-1.tar.gz"), Some(a));
        assert_eq!(checksum_for(&sums, "rada-2.zip"), Some("b".repeat(64)));
        assert_eq!(checksum_for(&sums, "missing"), None);
        // A hash of the wrong length is no hash.
        assert_eq!(checksum_for("abc  rada-1.tar.gz\n", "rada-1.tar.gz"), None);
    }
}
