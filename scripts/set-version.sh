#!/usr/bin/env bash
# Move the whole repository to a new version number.
#
#   scripts/set-version.sh 0.6.0-rc.11
#
# Rewrites the version of the workspace (Cargo.toml, which also holds the exact versions the
# crates ask of each other in [workspace.dependencies] for crates.io, and the three workspace
# crates in Cargo.lock) and every place in the docs that names the version that is current (the download example in
# docs/install.md, the version in docs/platforms.md and docs/updates.md). The CHANGELOG is not
# touched: its sections are written by hand. Run it from anywhere; it works on the repository the
# script lives in.
set -euo pipefail

new="${1:-}"
if [[ ! "$new" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
    echo "usage: $0 <version>   (for example 0.6.0-rc.11)" >&2
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
for f in Cargo.toml Cargo.lock docs/install.md docs/platforms.md docs/updates.md; do
    sed -i "s/$esc/$new/g" "$root/$f"
done
echo "$old -> $new"
grep -rnF "$old" "$root/Cargo.toml" "$root/Cargo.lock" "$root/docs/install.md" \
    "$root/docs/platforms.md" "$root/docs/updates.md" && exit 1
exit 0
