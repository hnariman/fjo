#!/bin/sh
# Package the release binary into an RPM using fjo.spec.
#
# Usage:
#   scripts/package.sh [output-dir]    (default: target/rpm)
#
# The release binary must already be built: target/release/fjo.
# Version consistency (Cargo.toml vs spec) is enforced; exit 1 on mismatch.
set -eu

ok() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v rpmbuild >/dev/null 2>&1 || die "rpmbuild not found"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
out_dir="${1:-$repo_root/target/rpm}"
tmp="$(mktemp -d /tmp/opencode/fjo-rpm.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT

# --- version consistency -----------------------------------------------------
cargo_version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' \
    "$repo_root/Cargo.toml" | head -1)"
spec_version="$(sed -n 's/^Version:[[:space:]]*\(.*\)/\1/p' \
    "$repo_root/fjo.spec")"
[ -n "$cargo_version" ] || die "cannot read version from Cargo.toml"
[ -n "$spec_version" ] || die "cannot read Version: from fjo.spec"
[ "$cargo_version" = "$spec_version" ] ||
    die "version drift: Cargo.toml=$cargo_version, fjo.spec=$spec_version (pause before syncing either)"
ok "version $cargo_version"

[ -x "$repo_root/target/release/fjo" ] ||
    die "no release binary at target/release/fjo; run: cargo build --release"

# --- stage the source tarball the spec expects -------------------------------
stage="$tmp/stage"
mkdir -p "$stage"
cp -r "$repo_root/skills" "$stage/skills"
cp "$repo_root/README.md" "$stage/README.md"
cp "$repo_root/target/release/fjo" "$stage/fjo"
chmod 0755 "$stage/fjo"

# %prep %autosetup -c creates fjo-<version>/ itself, so the tarball is
# content-only (no top-level prefix).
# --- rpm build ----------------------------------------------------------------
rpmd="$tmp/rpmbuild"
mkdir -p "$rpmd/BUILD" "$rpmd/RPMS" "$rpmd/SOURCES" "$rpmd/SPECS"
cp "$repo_root/fjo.spec" "$rpmd/SPECS/"

# %prep %autosetup -c creates fjo-<version>/ itself, so the tarball is
# content-only (no top-level prefix).
tar -C "$stage" -czf "$rpmd/SOURCES/fjo-$spec_version.tar.gz" .

ok "building rpm"
rpmbuild -bb \
    --define "_topdir $rpmd" \
    --define "_sourcedir $rpmd/SOURCES" \
    "$rpmd/SPECS/fjo.spec"

dist="$(rpmbuild --eval '%{?dist}' | sed 's/^\.//')"
rpm_file_rpm="$rpmd/RPMS/$(uname -m)/fjo-$spec_version-1.$dist.$(uname -m).rpm"
[ -f "$rpm_file_rpm" ] || die "build done but rpm missing: $rpm_file_rpm"

mkdir -p "$out_dir"
mv "$rpm_file_rpm" "$out_dir/"
ok "rpm -> $out_dir/$(basename "$rpm_file_rpm")"
