#!/bin/sh
# Publish artifacts to Forgejo: the RPM into its package-registry group and
# everything onto the git tag's release page.
#
# Usage:
#   scripts/publish.sh <rpm-file> [<extra-file>...]     (tag read from git)
#
# Required environment:
#   FORGEJO_TOKEN   access token with package + release write scopes
# Settings (shown with defaults):
#   FORGEJO_URL   https://git.home          FORGEJO_OWNER  hnariman
#   FORGEJO_GROUP ''                          (e.g. 'el9' to isolate rocky/alma)
#   FORGEJO_RELEASE_CREATE  1                 set 0 to only touch the registry
set -eu

ok() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

tmp_rel_json="$(mktemp)"
trap 'rm -f "$tmp_rel_json"' EXIT

[ -n "${FORGEJO_TOKEN:-}" ] ||
    die "FORGEJO_TOKEN not set; pass an access token with package write scope"

forgejo_url="${FORGEJO_URL:-https://git.home}"
owner="${FORGEJO_OWNER:-hnariman}"
repo="fjo"
group="${FORGEJO_GROUP:-}"

base_url="$forgejo_url/api/packages/$owner/rpm"
[ -n "$group" ] && base_url="$base_url/$group"

rpm_file="$1"; shift || true
[ -f "$rpm_file" ] || die "no such rpm: $rpm_file"

# rpm version/architecture from the filename: fjo-<v>-1.<dist>.<arch>.rpm
rpm_name="$(basename "$rpm_file")"
rpm_version="$(printf '%s' "$rpm_name" | cut -d- -f2)"
rpm_arch="$(printf '%s' "$rpm_name" | rev | cut -d. -f3 | rev)"
[ -n "$rpm_version" ] && [ -n "$rpm_arch" ] ||
    die "cannot parse version/arch from $rpm_name"

# --- registry -----------------------------------------------------------------
ok "publishing $rpm_name to registry (group: '${group:-<none>}')"
code="$(curl -sS --user "$owner:${FORGEJO_TOKEN}" \
    --upload-file "$rpm_file" \
    -o /dev/null -w '%{http_code}' \
    "$base_url/upload")" ||
    die "upload to $base_url failed (network)"
case "$code" in
201) ok "registry: created" ;;
409) ok "registry: same file already there, leaving as-is" ;;
*)   die "registry upload -> HTTP $code (bad rpm? 401 means token scope issue)" ;;
esac

# --- release page -------------------------------------------------------------
if [ "${FORGEJO_RELEASE_CREATE:-1}" = "0" ]; then
    ok "release creation disabled, registry done"
    exit 0
fi

tag="$rpm_version"
api="$forgejo_url/api/v1/repos/$owner/$repo"

release_id="$(curl -sS --user "$owner:${FORGEJO_TOKEN}" \
    "$api/releases/tags/$tag" -o "$tmp_rel_json" -w '%{http_code}')"
if [ "$release_id" = "200" ]; then
    release_id="$(sed -n 's/.*"id"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' \
        "$tmp_rel_json" | head -1)"
    ok "release for $tag exists (id $release_id)"
else
    tmp_post="$(mktemp)"
    code="$(curl -sS --user "$owner:${FORGEJO_TOKEN}" \
        -H 'Content-Type: application/json' \
        -d "{\"tag_name\":\"$tag\",\"name\":\"fjo $tag\",\"draft\":false,\"prerelease\":false}" \
        -o "$tmp_post" -w '%{http_code}' "$api/releases")"
    [ "$code" = "201" ] || die "release create -> HTTP $code"
    release_id="$(sed -n 's/.*"id"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' "$tmp_post" | head -1)"
    rm -f "$tmp_post"
    [ -n "$release_id" ] || die "release created but id unreadable"
    ok "release $tag created (id $release_id)"
fi

for asset in "$rpm_file" "$@"; do
    [ -f "$asset" ] || continue
    asset_name="$(basename "$asset")"
    ok "attaching $asset_name to release $tag"
    code="$(curl -sS --user "$owner:${FORGEJO_TOKEN}" \
        -F "attachment=@$asset" \
        -o /dev/null -w '%{http_code}' \
        "$api/releases/$release_id/assets?name=$asset_name")" ||
        die "asset upload failed (network)"
    case "$code" in
        201) ok "asset attached" ;;
        *)   die "asset upload -> HTTP $code" ;;
    esac
done

ok "published:"
printf '    dnf repo: %s/api/packages/%s/rpm%s.repo\n' \
    "$forgejo_url" "$owner" "$([ -n "$group" ] && printf "/$group")"
printf '    release:  %s/%s/%s/releases/tag/%s\n' \
    "$forgejo_url" "$owner" "$repo" "$tag"
