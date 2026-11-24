#!/usr/bin/env bash
# What a cargo call in client/ needs first, made only when its recipe changed: the pinned engine crates with
# patches/webrtc applied (the client's [patch] points at the result), and FFmpeg. Run by the gate and by the pre-commit
# clippy before either calls cargo.
#
# Usage: scripts/deps/native.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
scripts/deps/webrtc.sh > /dev/null
ffmpeg=scripts/deps/ffmpeg.sh
case "$(uname -s)" in MINGW* | MSYS*) ffmpeg=scripts/deps/ffmpeg-windows.sh ;; esac
if [[ "$(cat build/deps/.recipe 2> /dev/null)" != "$("$ffmpeg" --recipe)" ]]; then
  "$ffmpeg" > /dev/null
fi
# FFmpeg's static libraries are bundled into ffmpeg-sys-next's rlib, which cargo does not rebuild when they change, so
# the client drops it once after any build of them. `clean -p` drops one profile: the release window keeps its own.
if ! cmp -s build/deps/.recipe build/deps/.linked; then
  (cd client && cargo clean -p ffmpeg-sys-next && cargo clean --release -p ffmpeg-sys-next)
  cp build/deps/.recipe build/deps/.linked
fi
