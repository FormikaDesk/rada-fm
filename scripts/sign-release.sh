#!/usr/bin/env bash
# Sign a draft release made by the release workflow, and publish it.
#
#   scripts/sign-release.sh v0.6.0
#
# The workflow builds the binaries and creates the release as a DRAFT with SHA256SUMS. This
# script, run by hand on the maintainer's own computer, is the only way it gets published:
#   1. downloads the draft's files into a temporary folder,
#   2. checks the hash of every binary against SHA256SUMS (and that nothing is missing or extra),
#   3. signs SHA256SUMS with the main key, trusted comment "rada <tag>" (rsign asks for the
#      passphrase of the key; this script never sees it),
#   4. checks the signature it just made with the matching public key,
#   5. uploads SHA256SUMS.minisig and publishes the release.
# The private key stays in ~/.config/rada-release/ (outside the repository, and out of GitHub).
#
# Options:  --yes          do not ask before uploading and publishing
#           --no-publish   stop after step 4 (nothing is uploaded; the signature is printed)
# Environment: RADA_SIGN_KEY  secret key  (default ~/.config/rada-release/rada-main.key)
#              RADA_SIGN_PUB  public key  (default: the secret key's name with .pub)
#              RADA_REPO      owner/name  (default: the repository of the current folder)
#              RSIGN          the signing program (default: rsign)
#              RSIGN_SIGN_FLAGS  extra options for `rsign sign` (the tests of the script use -W)
#              GH             the GitHub CLI (default: gh)
set -euo pipefail

die() { echo "sign-release: $*" >&2; exit 1; }

yes=0; publish=1; tag=""
for arg in "$@"; do
  case "$arg" in
    --yes) yes=1 ;;
    --no-publish) publish=0 ;;
    -h|--help) awk 'NR > 1 { if (/^#/) print substr($0, 3); else exit }' "$0"; exit 0 ;;
    -*) die "unknown option $arg" ;;
    *) [ -z "$tag" ] || die "one tag only"; tag="$arg" ;;
  esac
done
[[ "$tag" =~ ^v[0-9][0-9A-Za-z.+-]*$ ]] || die "usage: scripts/sign-release.sh vX.Y.Z[-pre] [--yes] [--no-publish]"

key="${RADA_SIGN_KEY:-$HOME/.config/rada-release/rada-main.key}"
pub="${RADA_SIGN_PUB:-${key%.key}.pub}"
rsign="${RSIGN:-rsign}"
gh="${GH:-gh}"
repo=()
[ -z "${RADA_REPO:-}" ] || repo=(-R "$RADA_REPO")

command -v "$gh" > /dev/null || die "the GitHub CLI ($gh) is needed"
command -v "$rsign" > /dev/null || die "rsign is needed: cargo install --locked rsign2"
command -v sha256sum > /dev/null || die "sha256sum is needed"
[ -f "$key" ] || die "no secret key at $key (see docs/release.md)"
[ -f "$pub" ] || die "no public key at $pub"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Only a draft is signed: a published release must not change under the people who have it.
is_draft="$("$gh" release view "$tag" ${repo[@]+"${repo[@]}"} --json isDraft --jq .isDraft)"
[ "$is_draft" = true ] || die "$tag is not a draft release (already published, or it does not exist)"

echo "Downloading the files of the draft $tag..."
"$gh" release download "$tag" ${repo[@]+"${repo[@]}"} --dir "$work" --pattern '*'
cd "$work"

[ -f SHA256SUMS ] || die "the draft has no SHA256SUMS"
[ ! -e SHA256SUMS.minisig ] || die "the draft already has SHA256SUMS.minisig"

# The hashes again, here, from what was downloaded.
sha256sum --check --strict SHA256SUMS || die "a file of the draft does not match SHA256SUMS: not signing"
listed="$(awk '{print $2}' SHA256SUMS | sort)"
present="$(find . -maxdepth 1 -type f -name 'rada-*' | sed 's|^\./||' | sort)"
[ "$listed" = "$present" ] || die "SHA256SUMS and the files of the draft are not the same set:
listed:
$listed
present:
$present"
[ -n "$listed" ] || die "SHA256SUMS lists nothing"

comment="rada $tag"
echo
echo "About to sign SHA256SUMS of $tag with the trusted comment \"$comment\":"
sed 's/^/  /' SHA256SUMS
echo
# shellcheck disable=SC2086
"$rsign" sign -p "$pub" -s "$key" -t "$comment" -x SHA256SUMS.minisig ${RSIGN_SIGN_FLAGS:-} SHA256SUMS

# What was made must verify with the public key, and carry the comment of this tag.
verified="$("$rsign" verify -p "$pub" -x SHA256SUMS.minisig SHA256SUMS 2>&1)" \
  || die "the signature just made does not verify with $pub"
grep -qxF "trusted comment: $comment" SHA256SUMS.minisig || die "the signature does not carry the comment \"$comment\""
echo "$verified"

if [ "$publish" = 0 ]; then
  echo
  echo "Signed and verified; nothing was uploaded (--no-publish). The signature:"
  cat SHA256SUMS.minisig
  exit 0
fi

if [ "$yes" = 0 ]; then
  read -r -p "Upload SHA256SUMS.minisig and PUBLISH $tag? [y/N] " answer
  [ "$answer" = y ] || [ "$answer" = Y ] || die "not published; the draft is as it was"
fi

"$gh" release upload "$tag" ${repo[@]+"${repo[@]}"} SHA256SUMS.minisig
"$gh" release edit "$tag" ${repo[@]+"${repo[@]}"} --draft=false
echo "$tag is signed and published."
