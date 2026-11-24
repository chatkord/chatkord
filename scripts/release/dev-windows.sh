#!/usr/bin/env bash
# The Windows half of a dev release (`mise run release:dev:windows -- TAG`), on a Windows machine in Git Bash, as the
# gate runs there: the setup (scripts/package/windows.sh) built at the commit the pre-release TAG names, which HEAD must
# be with a clean tree (nothing is checked out here), then added to that pre-release with its line in SHA256SUMS. The
# setup's version is the tag's (next.py --build).
#
# Usage: scripts/release/dev-windows.sh TAG   (log in to gh first; needs what windows.sh needs)
set -euo pipefail
cd "$(dirname "$0")/../.."
(( $# == 1 )) || { echo "usage: dev-windows.sh TAG"; exit 2; }
tag=$1
# shellcheck source=../../protocol/scripts/find-python.sh
. protocol/scripts/find-python.sh
[[ -n "${PYTHON:-}" ]] || { echo "FAIL: no Python $PYTHON_FLOOR+"; exit 1; }
command -v gh > /dev/null || { echo "FAIL: gh is not installed, so the setup cannot be added to $tag"; exit 1; }

[[ -z "$(git status --porcelain)" ]] || { echo "FAIL: the tree is not clean, so its setup would not be $tag's"; exit 1; }
git rev-parse --quiet --verify "refs/tags/$tag" > /dev/null || git fetch --quiet origin "refs/tags/$tag:refs/tags/$tag"
[[ "$(git rev-parse "$tag^{commit}")" == "$(git rev-parse HEAD)" ]] ||
  { echo "FAIL: HEAD is not $tag's commit; check that out first"; exit 1; }
[[ "$(gh release view "$tag" --json isPrerelease --jq .isPrerelease)" == true ]] ||
  { echo "FAIL: $tag is not a pre-release, so this is no dev release's Windows half"; exit 1; }
version="$($PYTHON -I scripts/release/next.py --build)"
[[ "v$version" == "$tag" ]] || { echo "FAIL: a build here would be $version, not $tag's version"; exit 1; }

scripts/deps/native.sh
scripts/package/windows.sh
shopt -s nullglob
setup=(build/package/*-"$version".exe)
(( ${#setup[@]} == 1 )) || { echo "FAIL: windows.sh made no one setup of $version"; exit 1; }
name="${setup[0]##*/}"

sums=build/release
rm -rf "$sums"; mkdir -p "$sums"
gh release download "$tag" --pattern SHA256SUMS --dir "$sums"
# Git Bash's sha256sum marks a path `*` (binary mode) where Linux's does not: --text writes the AppImage's form.
{ grep -v -F -- "  $name" "$sums/SHA256SUMS" || true; (cd build/package && sha256sum --text -- "$name"); } > "$sums/new"
mv "$sums/new" "$sums/SHA256SUMS"
gh release upload "$tag" "${setup[0]}"
gh release upload "$tag" "$sums/SHA256SUMS" --clobber
rm -rf "$sums"
echo "release:dev:windows: added $name to the pre-release $tag"
