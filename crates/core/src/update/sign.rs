//! The signature of a release.
//!
//! The checksum of a download only protects against a damaged file: `SHA256SUMS` lives in the
//! same release as the program, so whoever can change one can change the other. What cannot be
//! changed from GitHub is a minisign signature made with a key that is kept offline. Every
//! release carries `SHA256SUMS.minisig`, a signature of `SHA256SUMS` whose *trusted comment* is
//! `rada <tag>`; rada checks it against the public keys below before it trusts a single hash,
//! and the comment must name the release being installed, so that a genuine signature of an
//! older release cannot be shown again as a newer one.
//!
//! There are two keys: the one releases are signed with, and a reserve that is kept apart and
//! never used until the first is lost. A release signed with either is accepted, so moving to
//! the reserve is a new release signed with it; a copy of rada built from this source is the
//! way to change the list.

use minisign_verify::{PublicKey, Signature};

use crate::i18n::{Key, tf};

/// Prefix of a public key in [`PUBLIC_KEYS`] that has not been filled in yet. A list that has
/// only such entries accepts no signature at all: nothing is installed from an unofficial build.
pub const PLACEHOLDER: &str = "PLACEHOLDER";

/// The public keys a release may be signed with, in the base64 form of a minisign `.pub` file
/// (the second line of it, `RW…`): the main key, then the reserve.
pub const PUBLIC_KEYS: [&str; 2] = [
    // PLACEHOLDER: the main key. Replaced by the public key made for the first signed release.
    "PLACEHOLDER-main-key-not-made-yet",
    // PLACEHOLDER: the reserve key, kept apart.
    "PLACEHOLDER-reserve-key-not-made-yet",
];

/// The list of keys a copy of rada trusts, as owned strings (what [`super::Ctx`] holds).
pub fn builtin_keys() -> Vec<String> {
    // A build made for the tests (the `testutil` feature, which no release has) can be given
    // a key by the environment, so that the program itself can be run against a stand-in
    // server. Nothing in a release reads the environment for a key.
    #[cfg(feature = "testutil")]
    if let Some(key) = std::env::var("RADA_TEST_SIGNING_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
    {
        return vec![key];
    }
    PUBLIC_KEYS.iter().map(|k| (*k).to_string()).collect()
}

/// The trusted comment a release of this tag must have been signed with.
pub fn comment_for(tag: &str) -> String {
    format!("rada {tag}")
}

/// What was proved, for the log.
#[derive(Debug, PartialEq, Eq)]
pub struct Signed {
    /// Which of the keys signed it (its position in the list).
    pub key: usize,
    pub comment: String,
}

/// Check that `signature` (the text of `SHA256SUMS.minisig`) is a valid signature of `sums`
/// by one of `keys`, and that its trusted comment is exactly `rada <tag>`. The errors are
/// messages for the person, and mean: install nothing.
pub fn verify_sums(
    sums: &[u8],
    signature: &str,
    tag: &str,
    keys: &[String],
) -> Result<Signed, String> {
    let usable: Vec<(usize, PublicKey)> = keys
        .iter()
        .enumerate()
        .filter(|(_, k)| !k.starts_with(PLACEHOLDER))
        .filter_map(|(i, k)| PublicKey::from_base64(k.trim()).ok().map(|k| (i, k)))
        .collect();
    if usable.is_empty() {
        return Err(Key::UpSigNoKey.tr().to_string());
    }
    let sig = Signature::decode(signature).map_err(|e| tf(Key::UpSigBad, &[&e]))?;
    // The signature names the key that made it; trying each key is how it is found, and the
    // wrong keys fail on that name before any arithmetic.
    let mut last = None;
    for (i, key) in &usable {
        match key.verify(sums, &sig, false) {
            Ok(()) => {
                let signed = sig.trusted_comment().to_string();
                let want = comment_for(tag);
                if signed != want {
                    return Err(tf(Key::UpSigTag, &[&signed, &want]));
                }
                return Ok(Signed {
                    key: *i,
                    comment: signed,
                });
            }
            Err(e) => last = Some(e),
        }
    }
    Err(tf(
        Key::UpSigBad,
        &[&last.map(|e| e.to_string()).unwrap_or_default()],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_built_in_are_still_placeholders_until_the_first_signed_release() {
        // When the real keys go in, this test is replaced by one that fails on a placeholder.
        assert!(PUBLIC_KEYS.iter().all(|k| k.starts_with(PLACEHOLDER)));
    }

    #[test]
    fn a_list_without_a_real_key_accepts_nothing() {
        let err = verify_sums(b"x", "not a signature", "v1.0.0", &builtin_keys()).unwrap_err();
        assert!(err.contains("no public key"), "{err}");
        let err = verify_sums(b"x", "x", "v1", &["not base64 at all".into()]).unwrap_err();
        assert!(err.contains("no public key"), "{err}");
    }

    #[test]
    fn the_comment_names_the_tag() {
        assert_eq!(comment_for("v0.6.0-rc.9"), "rada v0.6.0-rc.9");
    }
}
