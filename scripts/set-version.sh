#!/usr/bin/env bash
# Move the whole repository to a new version number.
#
#   scripts/set-version.sh 0.6.0-rc.11
#
# Rewrites the version of the workspace (Cargo.toml, which also holds the exact versions the
# crates ask of each other in [workspace.dependencies] for crates.io, the manifests of the
# crates, and the entries of those crates in Cargo.lock) and every place that names the version
# that is current (the download example and the cargo install line in docs/install.md, the
# cargo install line in README.md, the version in docs/platforms.md and docs/updates.md, the
# install line in the README of the rada crate). Only the crates of the workspace are moved: a
# crate from the registry with the same number stays, and so does a longer version that
# starts with it ("0.6.0-rc.9" when moving from 0.6.0). The CHANGELOG is not touched: its
# sections are written by hand. Run it from anywhere; it works on the repository the script lives in.
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
    # A suffix is a pre-release one (-rc.2): the "-x86_64" of a file name is not part of the version.
    # install.md whole; of README.md only the section between "## Install" and "## Quick start".
    check_text() {
        local file="$1" text="$2" v key
        while IFS= read -r v; do
            [[ -z "$v" || "$v" == "$current" || "$v" == "$base" ]] && continue
            echo "$file names $v, but the version is $current" >&2
            status=1
        done < <(grep -oE '[0-9]+\.[0-9]+\.[0-9]+(-(alpha|beta|rc|pre)[0-9A-Za-z.]*)?' <<<"$text" | sort -u || true)
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

# The crates of the workspace, by the name in their own Cargo.toml: only these are moved. A
# crate from the registry can have the very same version number and must stay as it is.
crates=()
manifests=("$root/Cargo.toml")
while IFS= read -r member; do
    # A member may be a glob ("crates/*"): left unquoted on purpose.
    for dir in "$root"/$member; do
        [[ -f "$dir/Cargo.toml" ]] || continue
        name="$(sed -n '/^\[package\]/,/^\[/{s/^name = "\(.*\)"$/\1/p;}' "$dir/Cargo.toml" | head -n 1)"
        if [[ -n "$name" ]]; then
            crates+=("$name")
            manifests+=("$dir/Cargo.toml")
        fi
    done
done < <(sed -n '/^members *= *\[/,/\]/p' "$root/Cargo.toml" | grep -oE '"[^"]+"' | tr -d '"')
if [[ ${#crates[@]} -eq 0 ]]; then
    echo "no crate of the workspace found in $root/Cargo.toml" >&2
    exit 1
fi
# What follows a version that is another one: a longer number, or a pre-release suffix.
export OLD="$old" NEW="$new" CRATES="${crates[*]}" NOT_AFTER='(?![0-9]|\.[0-9]|-(?:alpha|beta|rc|pre))'

# A manifest: the version of the package or of the workspace, and the exact version a
# workspace crate asks of another (`rada-core = { path = "...", version = "=X" }`).
rewrite_manifest() {
    perl -pi -e '
        BEGIN { %ours = map { $_ => 1 } split / /, $ENV{CRATES}; $section = "" }
        $section = $1 if /^\[(.*?)\]\s*$/;
        s/^(version\s*=\s*)"\Q$ENV{OLD}\E"/$1"$ENV{NEW}"/
            if $section eq "package" || $section eq "workspace.package";
        s/(version\s*=\s*"=)\Q$ENV{OLD}\E"/$1$ENV{NEW}"/ if /^([\w-]+)\s*=\s*\{/ && $ours{$1};
    ' "$1"
}

# The lock: the version of the entries of our crates, one by one.
rewrite_lock() {
    perl -pi -e '
        BEGIN { %ours = map { $_ => 1 } split / /, $ENV{CRATES}; $mine = 0 }
        $mine = 0 if /^\[\[package\]\]/;
        $mine = 1 if /^name = "([^"]+)"/ && $ours{$1};
        s/^(version = )"\Q$ENV{OLD}\E"/$1"$ENV{NEW}"/ if $mine;
    ' "$1"
}

# Prose: the version where it stands alone. The same number inside a longer one ("0.6.0-rc.9"
# when moving from 0.6.0, "0.6.10") is another version, often one of the past.
rewrite_prose() {
    perl -pi -e 'BEGIN { $re = qr/(?<![0-9.])\Q$ENV{OLD}\E$ENV{NOT_AFTER}/ } s/$re/$ENV{NEW}/g' "$1"
}

# What the old version still marks in the prose, if anything.
old_in_prose() {
    perl -ne 'BEGIN { $re = qr/(?<![0-9.])\Q$ENV{OLD}\E$ENV{NOT_AFTER}/ } print "$ARGV:$.:$_" if /$re/' "$1"
}

# The version the lock gives to the crate called `$1`.
locked_version() {
    WHO="$1" perl -ne '$n = $1 if /^name = "(.*)"/; print "$1\n" if defined $n && $n eq $ENV{WHO} && /^version = "(.*)"/' "$root/Cargo.lock"
}

docs=(README.md docs/install.md docs/platforms.md docs/updates.md crates/rada/README.md)
for f in "${docs[@]}"; do
    rewrite_prose "$root/$f"
done
for manifest in "${manifests[@]}"; do
    rewrite_manifest "$manifest"
done
rewrite_lock "$root/Cargo.lock"
echo "$old -> $new"

# Going from 0.6.0 to 0.6.0-rc.1 leaves "0.6.0" in the new version: nothing to look for then.
[[ "$new" == "$old"* ]] && exit 0
status=0
for f in "${docs[@]}"; do
    left="$(old_in_prose "$root/$f")"
    [[ -z "$left" ]] || { echo "$left" >&2; status=1; }
done
now="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
[[ "$now" == "$new" ]] || { echo "Cargo.toml still says $now" >&2; status=1; }
for name in "${crates[@]}"; do
    locked="$(locked_version "$name")"
    [[ -z "$locked" || "$locked" == "$new" ]] || { echo "Cargo.lock has $name at $locked" >&2; status=1; }
done
exit $status
