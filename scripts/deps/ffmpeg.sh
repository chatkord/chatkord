#!/usr/bin/env bash
# Builds the FFmpeg the Linux client links, from pinned sources: static, GPL (x264's, as the client's licence takes),
# hardware encoders (VA-API and Vulkan video), SVT-AV1 and x264 where no hardware encodes AV1 or H.264, the decoders
# that fall back to software, no programs. It is built against the libdrm and libva scripts/deps/libs.sh builds and
# the client ships, so what runs is what was tested. Nothing is taken from the host but a compiler and build tools.
#
# Usage: scripts/deps/ffmpeg.sh        (needs gcc, make, meson, ninja, cmake, pkg-config, git, and nasm on x86-64)
#        scripts/deps/ffmpeg.sh --recipe   the digests of what it builds from, which build/deps/.recipe holds
# Output: build/deps (headers, static libraries, pkg-config files, each project's licence in share/licenses).
#
# SANITIZE=address builds FFmpeg (--toolchain=clang-asan) and dav1d (-Db_sanitize=address) instrumented, into
# build/deps-asan from sources in build/deps-asan-src, for the sanitizer runs; the
# default build is unchanged. It needs clang and its AddressSanitizer runtime as well.
set -euo pipefail
SANITIZE=${SANITIZE:-}
case "$SANITIZE" in
  "" | address) ;;
  *) echo "FAIL: SANITIZE is empty or address"; exit 1 ;;
esac

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
recipe() {
  (cd "$ROOT" && sha256sum scripts/deps/ffmpeg.sh scripts/deps/ffmpeg-pin.sh scripts/deps/codecs-pin.sh scripts/deps/source.sh patches/ffmpeg/*.patch)
  # libva is linked in with FFmpeg's libraries, so a rebuilt libva is relinked as they are.
  "$ROOT/scripts/deps/libs.sh" --recipe
  [ -z "$SANITIZE" ] || echo "sanitize $SANITIZE"
  # An install names its absolute prefix (its pkg-config files), so a tree moved elsewhere builds it again.
  echo "in $ROOT"
}
[ "${1:-}" = --recipe ] && { recipe; exit 0; }
# shellcheck source=ffmpeg-pin.sh
. "$ROOT/scripts/deps/ffmpeg-pin.sh"
# shellcheck source=codecs-pin.sh
. "$ROOT/scripts/deps/codecs-pin.sh"
if [ -n "$SANITIZE" ]; then
  PREFIX=${PREFIX:-$ROOT/build/deps-asan}
  SRC=${SRC:-$ROOT/build/deps-asan-src}
  FFMPEG_TOOLCHAIN=--toolchain=clang-asan
  DAV1D_SANITIZE=-Db_sanitize=address
  # One AddressSanitizer runtime in a process: Rust links LLVM's, so every part is built by clang, never GCC's libasan.
  command -v clang >/dev/null || { echo "FAIL: clang is needed for the sanitizer build"; exit 1; }
  export CC=clang CXX=clang++
  # A build tree remembers its compiler, so a changed recipe starts the instrumented build from nothing.
  if [ -f "$PREFIX/.recipe" ] && [ "$(recipe)" != "$(cat "$PREFIX/.recipe")" ]; then rm -rf "$PREFIX" "$SRC"; fi
else
  PREFIX=${PREFIX:-$ROOT/build/deps}
  SRC=${SRC:-$ROOT/build/deps-src}
  FFMPEG_TOOLCHAIN=
  DAV1D_SANITIZE=
fi
JOBS=${JOBS:-$(nproc)}
mkdir -p "$PREFIX" "$SRC"
export PKG_CONFIG_PATH=$PREFIX/lib/pkgconfig:$PREFIX/share/pkgconfig
export PKG_CONFIG_LIBDIR=$PKG_CONFIG_PATH   # only ours: the host's libva and libdrm must not win
# shellcheck source=source.sh
. "$ROOT/scripts/deps/source.sh"

# The libraries FFmpeg is built against, built once and stamped on their own.
PREFIX=$PREFIX SRC=$SRC "$ROOT/scripts/deps/libs.sh" >/dev/null

# AV1 has no software decoder in FFmpeg itself, so dav1d (BSD-2) is the fallback where no hardware decodes it.
# Without nasm dav1d and FFmpeg build without their assembly, and a 4K H.264 picture took 31.5 ms in software against
# 14.1 (Machine B), so that is a failure, not a default.
if [ "$(uname -m)" = x86_64 ]; then command -v nasm >/dev/null || { echo "FAIL: nasm is needed (decoding without assembly is too slow)"; exit 1; }; fi
# dav1d's origin is code.videolan.org, and that host sits behind Anubis, an anti-scraper proof-of-work wall whose
# page a plain git client cannot pass: from a build machine the clone either dies on the challenge or refuses the
# connection outright (`fatal: unable to access 'https://code.videolan.org/videolan/dav1d.git/': Failed to connect
# to code.videolan.org port 443 after 332 ms`, the step ending at exit 128), and it does so intermittently -- the
# same host served a run fetching it an hour and a half earlier the same evening. This clones VideoLAN's own read-only
# mirror of the same repository instead, and the commit check below is the pin: a mirror that does not carry
# DAV1D_COMMIT under $DAV1D_TAG fails the build rather than changing what is built.
fetch dav1d https://github.com/videolan/dav1d.git "$DAV1D_TAG" "$DAV1D_COMMIT"
made_here "$SRC/dav1d/build"
meson setup --reconfigure "$SRC/dav1d/build" "$SRC/dav1d" --prefix="$PREFIX" "${DAV1D_FLAGS[@]}" \
  ${DAV1D_SANITIZE:+"$DAV1D_SANITIZE"} >/dev/null
ninja -C "$SRC/dav1d/build" install >/dev/null
licence dav1d COPYING

# AV1's software encoder where no hardware makes it (BSD-3-Clause-Clear and AOM's patent licence). The RTC build
# keeps only the live coding a share uses, and nasm is its assembly as it is dav1d's.
fetch svt-av1 https://gitlab.com/AOMediaCodec/SVT-AV1.git "$SVT_AV1_TAG" "$SVT_AV1_COMMIT"
made_here "$SRC/svt-av1/build"
cmake -S "$SRC/svt-av1" -B "$SRC/svt-av1/build" -G Ninja -DCMAKE_INSTALL_PREFIX="$PREFIX" "${SVT_AV1_FLAGS[@]}" \
  -DCMAKE_POSITION_INDEPENDENT_CODE=ON >/dev/null
cmake --build "$SRC/svt-av1/build" --target install >/dev/null
licence svt-av1 LICENSE.md PATENTS.md

# H.264's software encoder where no hardware makes it (GPL-2.0-or-later). x264 publishes no release tags: its stable
# branch is the release. Its own FFmpeg and swscale inputs are off, so it never reads the FFmpeg built below.
fetch_branch x264 https://code.videolan.org/videolan/x264.git "$X264_BRANCH" "$X264_COMMIT"
(cd "$SRC/x264" && ./configure --prefix="$PREFIX" "${X264_FLAGS[@]}" --enable-pic \
  >/dev/null && make -j "$JOBS" >/dev/null && make install-lib-static >/dev/null)
licence x264 COPYING

fetch_ffmpeg "$SRC/ffmpeg"
cd "$SRC/ffmpeg"
# `mjpeg`: a UVC camera gives 720p30 only as MJPEG (Machine B's sends YUY2 at 720p ten times a second, measured
# 2026-11-11).
./configure --prefix="$PREFIX" --pkg-config-flags=--static ${FFMPEG_TOOLCHAIN:+"$FFMPEG_TOOLCHAIN"} \
  --enable-static --disable-shared --enable-pic "${FFMPEG_FLAGS[@]}" \
  --enable-vaapi --enable-libdrm --enable-vulkan "${FFMPEG_SOFTWARE_FLAGS[@]}" \
  --enable-encoder=h264_vaapi,av1_vaapi,h264_vulkan,av1_vulkan,$FFMPEG_SOFTWARE_ENCODERS \
  --enable-decoder=$FFMPEG_DECODERS,mjpeg \
  --enable-hwaccel=h264_vaapi,av1_vaapi,h264_vulkan,av1_vulkan \
  --enable-parser=$FFMPEG_PARSERS --enable-bsf=$FFMPEG_BSFS \
  --enable-filter=$FFMPEG_FILTERS,scale_vaapi >/dev/null
make -j "$JOBS" >/dev/null
make install >/dev/null
licence ffmpeg COPYING.GPLv2 LICENSE.md
recipe > "$PREFIX/.recipe"
echo "$FFMPEG_NAME built into $PREFIX"
