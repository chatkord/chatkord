#!/usr/bin/env bash
# A dev release (`mise run release:dev`), one per commit, as the next release's pre-release
# (scripts/release/next.py --dev): the GitHub pre-release `v<version>` at HEAD with the AppImage of the Dev app
# (CHATKORD_CHANNEL=dev, scripts/package/linux.sh) and the SHA256SUMS of what it carries. The Windows setup joins it
# from a Windows machine (scripts/release/dev-windows.sh). It is an unsigned build by hand: no update feed is stamped,
# so it offers no update and no client takes it as one (CI's `Build dev` workflow makes the signed feed). Only a clean
# tree is built, so the build is HEAD's; it asks for no gate, which is main's.
#
# Usage: scripts/release/dev.sh   (log in to gh first; a container runtime as scripts/ci/builder.sh needs)
# Output: the pre-release made.
set -euo pipefail
cd "$(dirname "$0")/../.."
export CHATKORD_CHANNEL=dev
unset CHATKORD_UPDATES CHATKORD_VERSION
# shellcheck source=../package/channel.sh
. scripts/package/channel.sh
# shellcheck source=../../protocol/scripts/find-python.sh
. protocol/scripts/find-python.sh
[[ -n "${PYTHON:-}" ]] || { echo "FAIL: no Python $PYTHON_FLOOR+"; exit 1; }

[[ -z "$(git status --porcelain)" ]] || { echo "FAIL: the tree is not clean, so its build would not be HEAD's"; exit 1; }
head="$(git rev-parse HEAD)"
# Every release and dev release made so far, so the next is counted from the last and HEAD's own is seen.
git fetch --quiet --tags origin
# GitHub tags only a commit it has.
[[ -n "$(git branch --remotes --contains "$head")" ]] || { echo "FAIL: HEAD is not on origin yet; push it first"; exit 1; }
built="$($PYTHON -I scripts/release/next.py --build)"
[[ -z "$(git tag --points-at HEAD --list "v$built")" ]] ||
  { echo "FAIL: HEAD is v$built already, and a commit has one dev release"; exit 1; }
version="$($PYTHON -I scripts/release/next.py --dev)"
tag="v$version"

# The tag is made here first, so the AppImage carries its version (next.py --build), and dropped again unless the
# pre-release that publishes it was made.
git tag "$tag" "$head"
released=0
trap 'if (( ! released )); then git tag --delete "$tag" > /dev/null; fi' EXIT
scripts/package/linux.sh
appimage="build/package/$CHANNEL_FILE-$version-x86_64.AppImage"
[[ -f "$appimage" ]] || { echo "FAIL: linux.sh made no AppImage of $version"; exit 1; }
saved=build/release
rm -rf "$saved"; mkdir -p "$saved"
(cd build/package && sha256sum --text -- "${appimage##*/}") > "$saved/SHA256SUMS"

notes="A dev build of $head, no release: the release it precedes outranks it. The Windows setup joins it by"
notes+=" \`mise run release:dev:windows -- $tag\` on a Windows machine at this commit."
gh release create "$tag" --prerelease --latest=false --target "$head" --title "$tag (dev)" --notes "$notes" \
  "$appimage" "$saved/SHA256SUMS"
released=1
rm -rf "$saved"
echo "release:dev: made the pre-release $tag with the AppImage; the Windows setup is next"
