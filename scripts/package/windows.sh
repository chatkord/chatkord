#!/usr/bin/env bash
# The Windows setup: the release window, held to scripts/size-budget.txt, and FFmpeg's DLLs in one per-user installer
#, build/package/chatkord-setup-<version>.exe. Inno Setup is installed into
# build/tools first, checked against the SHA-256 its release publishes. The product's name is the window's title in
# the English catalog, its version scripts/release/next.py's (a release's at its tag, else the next release's
# pre-release), so neither is restated here or in the recipe.
#
# Usage: scripts/package/windows.sh   (needs build/deps from scripts/deps/ffmpeg-windows.sh at this tree's recipe)
# CHATKORD_UPDATES, when set, is the update channel the window is built with;
# a build without it, as any local one, has none.
set -euo pipefail
cd "$(dirname "$0")/../.."
# shellcheck source=budget.sh
. scripts/package/budget.sh

INNO_VERSION=7.1.1
INNO_SHA256=8a5a6711c3766a6e5fb74f280ba0e3551e719ce94538d901fdbd1f8f9920b3f0
inno="build/tools/inno-$INNO_VERSION"

if [[ ! -x "$inno/ISCC.exe" ]]; then
  mkdir -p build/tools
  setup="build/tools/innosetup-$INNO_VERSION-x64.exe"
  [[ -f "$setup" ]] || curl -sSfL -o "$setup" \
    "https://github.com/jrsoftware/issrc/releases/download/is-${INNO_VERSION//./_}/innosetup-$INNO_VERSION-x64.exe"
  [[ "$(sha256sum "$setup" | cut -d' ' -f1)" == "$INNO_SHA256" ]] || { echo "FAIL: $setup is not Inno Setup $INNO_VERSION"; exit 1; }
  # Git Bash would read the switches as paths.
  MSYS2_ARG_CONV_EXCL='*' "$setup" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER /PORTABLE=1 /NOICONS \
    "/DIR=$(cygpath -w "$PWD/$inno")"
  [[ -x "$inno/ISCC.exe" ]] || { echo "FAIL: Inno Setup $INNO_VERSION did not install into $inno"; exit 1; }
fi

[[ "$(cat build/deps/.recipe 2> /dev/null)" == "$(scripts/deps/ffmpeg-windows.sh --recipe)" ]] ||
  { echo "FAIL: build/deps is not this tree's FFmpeg; run scripts/deps/ffmpeg-windows.sh"; exit 1; }
# shellcheck source=../../protocol/scripts/find-python.sh
. protocol/scripts/find-python.sh
[[ -n "${PYTHON:-}" ]] || { echo "FAIL: no Python $PYTHON_FLOOR+"; exit 1; }
version="$($PYTHON -I scripts/release/next.py --build)"
scripts/deps/webrtc.sh > /dev/null
# The window and its resources say the version the setup is named by (terms::BUILD).
(cd client && CHATKORD_VERSION="$version" cargo build --locked --release -p shell)
within_budget client/target/release/shell.exe windows
name="$(sed -n 's/^window-title = //p' client/words/catalogs/en.ftl | tr -d '\r')"
# Every carried project's licence and our own, and the notices the About page reads, as the setup installs them.
rm -rf build/package/licenses
bash scripts/package/notices.sh build/package/licenses "$version" client/shell/Cargo.toml

MSYS2_ARG_CONV_EXCL='*' "$inno/ISCC.exe" -q "/DName=$name" "/DVersion=$version" scripts/package/windows.iss
echo "built build/package/chatkord-setup-$version.exe"
