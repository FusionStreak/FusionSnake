#!/usr/bin/env bash
# Keeps the three Rust version pins in sync.
#
#   bump-rust-toolchain.sh <version> [repo-root]   Rewrite all pins
#   bump-rust-toolchain.sh --is-newer <new> <pin>  Exit 0 if newer, 1 if not
#
# Exits 2 on a malformed version, and changes nothing on any failure.
set -euo pipefail

valid() { [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; }
usage() {
    echo "error: expected a version like 1.101.0, got '${1:-}'" >&2
    exit 2
}

if [[ "${1:-}" == "--is-newer" ]]; then
    valid "${2:-}" || usage "${2:-}"
    valid "${3:-}" || usage "${3:-}"
    [[ "$2" != "$3" ]] || exit 1
    [[ "$(printf '%s\n%s\n' "$2" "$3" | sort -V | tail -n 1)" == "$2" ]]
    exit
fi

version="${1:-}"
valid "$version" || usage "$version"
root="${2:-.}"

toolchain="$root/rust-toolchain.toml"
manifest="$root/Cargo.toml"
dockerfile="$root/src/Dockerfile"

# Check every pin first so a failure can't leave the files out of sync.
grep -q '^channel = "' "$toolchain" || { echo "error: no channel in $toolchain" >&2; exit 1; }
grep -q '^rust-version = "' "$manifest" || { echo "error: no rust-version in $manifest" >&2; exit 1; }
grep -Eq '^FROM rust:[0-9.]+-slim' "$dockerfile" || { echo "error: no rust image in $dockerfile" >&2; exit 1; }

sed -i "s/^channel = \".*\"/channel = \"$version\"/" "$toolchain"
sed -i "s/^rust-version = \".*\"/rust-version = \"${version%.*}\"/" "$manifest"
sed -i -E "s/^FROM rust:[0-9.]+-slim/FROM rust:$version-slim/" "$dockerfile"
