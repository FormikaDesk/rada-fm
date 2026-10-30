#!/usr/bin/env bash
# Move the whole repository to a new version number.
#
#   scripts/set-version.sh 0.6.0-rc.11
#
# Rewrites the version of the workspace (Cargo.toml, which also holds the exact versions the
# crates ask of each other in [workspace.dependencies] for crates.io, and the three workspace
# crates in Cargo.lock) and every place that names the version that is current (the download
# example and the cargo install line in docs/install.md, the cargo install line in README.md, the
# version in docs/platforms.md and docs/updates.md, the install line in the README of the rada
# crate). The CHANGELOG is not touched: its sections are written
# by hand. Run it from anywhere; it works on the repository the script lives in.
#
#   scripts/set-version.sh --check
#
# Changes nothing and fails if the commands a person copies are out of date: a version in
# docs/install.md, or in the Install section of README.md, that is not the one of Cargo.toml
# (the version without its suffix is let through: "with 0.6.0 ... will be enough"), or a public
# key there that verify.md does not have, or `version` used as the name of a variable (fish keeps
# that name for its own version: the commands call it `rv`). `scripts/test-set-version` tries both
# modes.
set -euo pipefail

if [[ "${1:-}" == "--check" ]]; then
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    current="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
    [[ -n "$current" ]] || { echo "no version found in $root/Cargo.toml" >&2; exit 1; }
    base="${current%%-*}"
    status=0
    # install.md whole; of README.md only the section between "## Install" and "## Quick start".
    check_text() {
        local file="$1" text="$2" v key
        while IFS= read -r v; do
            [[ -z "$v" || "$v" == "$current" || "$v" == "$base" ]] && continue
            echo "$file names $v, but the version is $current" >&2
            status=1
        done < <(grep -oE '[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?' <<<"$text" | sort -u || true)
        if ! grep -qF "$current" <<<"$text"; then
            echo "$file does not name $current at all" >&2
            status=1
        fi
        if grep -qE '(^|[[:space:];])(version=|set +version([[:space:]]|$))|[$]version\b' <<<"$text"; then
            echo "$file uses version as the name of a variable (fish keeps it for itself): use rv" >&2
            status=1
        fi
        while IFS= read -r key; do
            [[ -z "$key" ]] && continue
            grep -qF "$key" "$root/docs/verify.md" || {
                echo "$file has a public key that docs/verify.md does not have: $key" >&2
                status=1
            }
        done < <(grep -oE 'RW[A-Za-z0-9+/]{54}' <<<"$text" || true)
    }
    check_text docs/install.md "$(cat "$root/docs/install.md")"
    check_text README.md "$(sed -n '/^## Install$/,/^## Quick start$/p' "$root/README.md")"
    [[ $status -eq 0 ]] && echo "versions and keys in docs/install.md and README.md are current ($current)"
    exit $status
fi

new="${1:-}"
if [[ ! "$new" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
    echo "usage: $0 <version> | --check   (for example 0.6.0-rc.11)" >&2
    exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
old="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
if [[ -z "$old" ]]; then
    echo "no version found in $root/Cargo.toml" >&2
    exit 1
fi
if [[ "$old" == "$new" ]]; then
    echo "already at $new"
    exit 0
fi

# Dots in the old version are literal.
esc="${old//./\\.}"
for f in Cargo.toml Cargo.lock README.md docs/install.md docs/platforms.md docs/updates.md crates/rada/README.md; do
    sed -i "s/$esc/$new/g" "$root/$f"
done
echo "$old -> $new"
grep -rnF "$old" "$root/Cargo.toml" "$root/Cargo.lock" "$root/README.md" "$root/docs/install.md" \
    "$root/docs/platforms.md" "$root/docs/updates.md" "$root/crates/rada/README.md" && exit 1
exit 0
