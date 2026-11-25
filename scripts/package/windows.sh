#!/usr/bin/env bash
# The Windows setup: the release window, held to scripts/size-budget.txt, and FFmpeg's DLLs in one per-user installer,
# build/package/<CHANNEL_FILE>-setup-<version>.exe. Inno Setup is installed into build/tools first, checked against
# the SHA-256 its release publishes. The names are the channel's
# (scripts/package/channel.sh, which windows.iss reads from the environment), the version CHATKORD_VERSION when set,
# else scripts/release/next.py's (a release's at its tag, else the next release's pre-release), so none is restated.
#
# Usage: scripts/package/windows.sh   (needs build/deps from scripts/deps/ffmpeg-windows.sh at this tree's recipe)
# CHATKORD_CHANNEL names the channel built (stable by default). CHATKORD_UPDATES, when set, is the update channel the
# window is built with; a build without it, as any local one, has none.
set -euo pipefail
cd "$(dirname "$0")/../.."
# shellcheck source=channel.sh
. scripts/package/channel.sh
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

recipe=$(scripts/deps/ffmpeg-windows.sh --recipe)
if [[ "$(cat build/deps/.recipe 2> /dev/null)" != "$recipe" ]]; then
  echo "FAIL: build/deps is not this tree's FFmpeg; run scripts/deps/ffmpeg-windows.sh. Built from (<) and this tree's (>):"
  diff build/deps/.recipe - <<< "$recipe" || true
  exit 1
fi
# shellcheck source=../../protocol/scripts/find-python.sh
. protocol/scripts/find-python.sh
[[ -n "${PYTHON:-}" ]] || { echo "FAIL: no Python $PYTHON_FLOOR+"; exit 1; }
version="${CHATKORD_VERSION:-$($PYTHON -I scripts/release/next.py --build)}"
scripts/deps/webrtc.sh > /dev/null
# The window and its resources say the version the setup is named by (terms::BUILD).
(cd client && CHATKORD_VERSION="$version" cargo build --locked --release -p shell)
within_budget client/target/release/shell.exe windows
# Every carried project's licence and our own, and the notices the About page reads, as the setup installs them.
rm -rf build/package/licenses
bash scripts/package/notices.sh build/package/licenses "$version" client/shell/Cargo.toml

# The recipe reads the channel's names from these, so a value with a space needs no quoting on ISCC's command line.
export CHANNEL_ID CHANNEL_PRODUCT CHANNEL_FILE CHANNEL_APP_USER_MODEL_ID CHANNEL_SETUP_ID
MSYS2_ARG_CONV_EXCL='*' "$inno/ISCC.exe" -q "/DVersion=$version" scripts/package/windows.iss
echo "built build/package/$CHANNEL_FILE-setup-$version.exe"
