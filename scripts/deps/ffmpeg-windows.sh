#!/usr/bin/env bash
# Builds the FFmpeg the Windows client loads, from pinned sources: DLLs built by MSVC against the static CRT
# (as the engine is), GPL (x264's, as the client's licence takes), hardware encoders (NVENC, AMF, QSV through
# libvpl), D3D12VA decode on the window's own device and D3D11VA beside it, SVT-AV1 and x264 where no hardware
# encodes AV1 or H.264, the decoders that can fall back to software, no programs. Linux's is scripts/deps/ffmpeg.sh.
#
# Usage: scripts/deps/ffmpeg-windows.sh   (from Git Bash; needs Visual Studio's C++ build tools, MSYS2 with
#        make, nasm, pkgconf and diffutils, git and Python at protocol/scripts/find-python.sh's floor)
#        scripts/deps/ffmpeg-windows.sh --recipe   the digests of what it builds from, which build/deps/.recipe holds
# Output: build/deps (headers, import libraries, DLLs in bin/, pkg-config files, each project's licence in
# share/licenses).
set -euo pipefail

# A step's output kept and shown only when it fails, so a log says why without carrying every build's.
quiet() {
  local log
  log=$(mktemp)
  if "$@" > "$log" 2>&1; then rm -f "$log"; else local failed=$?; cat "$log"; rm -f "$log"; return "$failed"; fi
}

# Each pin is a tag AND the commit it must be, so a moved tag fails the build instead of changing it.
NVCODEC_TAG=n13.1.15.0;         NVCODEC_COMMIT=0a6fba9a2820628b8103464f4c8753ee05838baa
AMF_TAG=v1.5.3;                 AMF_COMMIT=57ee0cfb0e1a08d9041f77435653794c1b012be0
LIBVPL_TAG=v2.17.0;             LIBVPL_COMMIT=d77f9195cf495b937631607333288fd917ae8939

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
recipe() {
  # --text: Git Bash marks each path `*` (binary mode), and MSYS2's bash, which writes .recipe, is checked from it.
  (cd "$ROOT" && sha256sum --text scripts/deps/ffmpeg-windows.sh scripts/deps/ffmpeg-pin.sh scripts/deps/codecs-pin.sh \
    scripts/deps/source.sh patches/ffmpeg/*.patch)
  # An install names its absolute prefix (its pkg-config files), so a tree moved elsewhere builds it again.
  echo "in $ROOT"
}
[ "${1:-}" = --recipe ] && { recipe; exit 0; }
# shellcheck source=ffmpeg-pin.sh
. "$ROOT/scripts/deps/ffmpeg-pin.sh"
# shellcheck source=codecs-pin.sh
. "$ROOT/scripts/deps/codecs-pin.sh"

if [ -z "${VCToolsInstallDir:-}" ]; then
  # Run again inside MSYS2's bash with the compiler's environment: FFmpeg's configure needs MSYS2's make,
  # and Git Bash's PATH has a coreutils `link` where MSVC's linker must be. Git Bash also exports
  # ORIGINAL_PATH, which MSYS2's profile would take instead of the PATH vcvars made.
  installer="/c/Program Files (x86)/Microsoft Visual Studio/Installer"
  vs=$("$installer/vswhere.exe" -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
  [ -n "$vs" ] || { echo "FAIL: no Visual Studio with the C++ build tools"; exit 1; }
  msys2=${MSYS2:-C:/msys64}
  [ -x "$msys2/usr/bin/bash.exe" ] || { echo "FAIL: no MSYS2 at $msys2 (set MSYS2)"; exit 1; }
  mkdir -p "$ROOT/build/deps-src"
  run=$ROOT/build/deps-src/msvc-msys2.cmd
  cat > "$run" <<EOF
@call "$vs\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
@set MSYS2_PATH_TYPE=inherit
@set ORIGINAL_PATH=
@set CHERE_INVOKING=1
@set MSYSTEM=MSYS
@"$(cygpath -w "$msys2")\usr\bin\bash.exe" -l %*
EOF
  PATH="$installer:$PATH" exec cmd //c "$(cygpath -w "$run")" "$(cygpath -u "$0")"
fi

for tool in make nasm pkgconf diff; do
  command -v "$tool" >/dev/null || { echo "FAIL: $tool is needed (pacman -S make nasm pkgconf diffutils)"; exit 1; }
done
# Mixed paths (D:/...) are read the same by MSYS2's tools and by MSVC's.
PREFIX=$(cygpath -m "${PREFIX:-$ROOT/build/deps}")
SRC=$(cygpath -m "${SRC:-$ROOT/build/deps-src}")
JOBS=${JOBS:-$(nproc)}
mkdir -p "$PREFIX" "$SRC"
# MSYS2's pkgconf splits this on ':', so it takes the MSYS form (/d/...), not the mixed one.
PKG_CONFIG_PATH=$(cygpath -u "$PREFIX/lib/pkgconfig")
export PKG_CONFIG_PATH
export PKG_CONFIG_LIBDIR=$PKG_CONFIG_PATH   # only ours: nothing else on this machine may win
# shellcheck source=source.sh
. "$ROOT/scripts/deps/source.sh"

# shellcheck source=../../protocol/scripts/find-python.sh
. "$ROOT/protocol/scripts/find-python.sh"
[ -n "$PYTHON" ] || { echo "FAIL: no Python $PYTHON_FLOOR+ for meson"; exit 1; }
# The build tools, from the wheels scripts/deps/python-tools.sh checked, into a venv of their own made again when they change.
"$ROOT/scripts/deps/python-tools.sh"
WHEELS=$(cygpath -m "$ROOT/build/deps-src/wheels")
made_here "$SRC/tools"
if ! cmp -s "$WHEELS/.lock" "$SRC/tools/.lock"; then
  rm -rf "$SRC/tools"
  $PYTHON -m venv "$SRC/tools"
  "$SRC/tools/Scripts/python.exe" -m pip -q install --no-index --find-links "$WHEELS" meson ninja cmake
  cp "$WHEELS/.lock" "$SRC/tools/.lock"
fi
# MSVC's own bin first: MSYS2's /usr/bin carries a coreutils `link` too.
PATH="$(cygpath -u "$VCToolsInstallDir")bin/Hostx64/x64:$(cygpath -u "$SRC/tools/Scripts"):$PATH"
export PATH

# NVENC: the headers only; the driver is loaded at run time.
fetch nv-codec-headers https://github.com/FFmpeg/nv-codec-headers.git "$NVCODEC_TAG" "$NVCODEC_COMMIT"
quiet make -s -C "$SRC/nv-codec-headers" PREFIX="$PREFIX" install
# nv-codec-headers has no licence file: its text is the header's.
mkdir -p "$PREFIX/share/licenses/nv-codec-headers"
sed -n '/This copyright notice/,/^ \*\/$/p' "$SRC/nv-codec-headers/include/ffnvcodec/nvEncodeAPI.h" \
  > "$PREFIX/share/licenses/nv-codec-headers/COPYING"

# AMF: the headers only (FFmpeg includes them as <AMF/...>); the driver is loaded at run time.
fetch amf https://github.com/GPUOpen-LibrariesAndSDKs/AMF.git "$AMF_TAG" "$AMF_COMMIT" amf/public/include
rm -rf "$PREFIX/include/AMF" && mkdir -p "$PREFIX/include/AMF" && cp -r "$SRC/amf/amf/public/include/." "$PREFIX/include/AMF/"
licence amf LICENSE.txt

# QSV: libvpl's dispatcher (MIT), linked into FFmpeg; it finds the Intel runtime at run time. Its vpl.pc names
# the system libraries it needs only for MinGW, so FFmpeg's --extra-libs below names them; it sets CMP0091 only
# under `if(MSVC)` before project() defines MSVC, so its static runtime needs the policy from here.
fetch libvpl https://github.com/intel/libvpl.git "$LIBVPL_TAG" "$LIBVPL_COMMIT"
made_here "$SRC/libvpl/build"
quiet cmake -S "$SRC/libvpl" -B "$SRC/libvpl/build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -DCMAKE_C_COMPILER=cl -DCMAKE_CXX_COMPILER=cl -DUSE_MSVC_STATIC_RUNTIME=ON -DCMAKE_POLICY_DEFAULT_CMP0091=NEW -DBUILD_SHARED_LIBS=OFF \
  -DBUILD_EXAMPLES=OFF -DBUILD_TESTS=OFF -DINSTALL_EXAMPLES=OFF -DCMAKE_INSTALL_SYSTEM_RUNTIME_LIBS_SKIP=ON
quiet cmake --build "$SRC/libvpl/build" --target install
licence libvpl LICENSE

# AV1's software fallback, as on Linux (ffmpeg.sh says why the mirror and why nasm).
fetch dav1d https://github.com/videolan/dav1d.git "$DAV1D_TAG" "$DAV1D_COMMIT"
made_here "$SRC/dav1d/build-msvc"
quiet env CC=cl meson setup --reconfigure "$SRC/dav1d/build-msvc" "$SRC/dav1d" --prefix="$PREFIX" "${DAV1D_FLAGS[@]}" \
  -Db_vscrt=mt
quiet ninja -C "$SRC/dav1d/build-msvc" install
# meson names a static library libdav1d.a with MSVC too; MSVC's link looks for dav1d.lib.
cp "$PREFIX/lib/libdav1d.a" "$PREFIX/lib/dav1d.lib"
licence dav1d COPYING

# AV1's software encoder, as on Linux (ffmpeg.sh says why the RTC build).
fetch svt-av1 https://gitlab.com/AOMediaCodec/SVT-AV1.git "$SVT_AV1_TAG" "$SVT_AV1_COMMIT"
made_here "$SRC/svt-av1/build-msvc"
quiet cmake -S "$SRC/svt-av1" -B "$SRC/svt-av1/build-msvc" -G Ninja -DCMAKE_INSTALL_PREFIX="$PREFIX" "${SVT_AV1_FLAGS[@]}" \
  -DCMAKE_C_COMPILER=cl -DCMAKE_CXX_COMPILER=cl -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded
quiet cmake --build "$SRC/svt-av1/build-msvc" --target install
licence svt-av1 LICENSE.md PATENTS.md

# H.264's software encoder, as on Linux (ffmpeg.sh says why the stable branch), against the static CRT: its configure
# names no C library of its own for MSVC, so -MT is passed as every library here is built.
fetch_branch x264 https://code.videolan.org/videolan/x264.git "$X264_BRANCH" "$X264_COMMIT"
(cd "$SRC/x264" && quiet env CC=cl ./configure --prefix="$PREFIX" "${X264_FLAGS[@]}" --extra-cflags=-MT &&
  quiet make -j "$JOBS" && quiet make install-lib-static)
licence x264 COPYING

fetch_ffmpeg "$SRC/ffmpeg"
cd "$SRC/ffmpeg"
quiet ./configure --prefix="$PREFIX" --toolchain=msvc --target-os=win64 --arch=x86_64 --pkg-config=pkgconf \
  --extra-cflags="-MT -I$PREFIX/include" --extra-libs="ole32.lib advapi32.lib gdi32.lib uuid.lib" \
  --enable-shared --disable-static "${FFMPEG_FLAGS[@]}" \
  --enable-d3d11va --enable-d3d12va --enable-ffnvcodec --enable-nvenc --enable-amf --enable-libvpl \
  "${FFMPEG_SOFTWARE_FLAGS[@]}" \
  --enable-encoder=h264_nvenc,av1_nvenc,h264_amf,av1_amf,h264_qsv,av1_qsv,$FFMPEG_SOFTWARE_ENCODERS \
  --enable-decoder=$FFMPEG_DECODERS \
  --enable-hwaccel=h264_d3d11va,h264_d3d11va2,av1_d3d11va,av1_d3d11va2,h264_d3d12va,av1_d3d12va \
  --enable-parser=$FFMPEG_PARSERS --enable-bsf=$FFMPEG_BSFS \
  --enable-filter=$FFMPEG_FILTERS
quiet make -j "$JOBS"
quiet make install
# FFmpeg puts the import libraries beside the DLLs; its .pc files name lib/.
for lib in avcodec avfilter avutil swscale; do mv -f "$PREFIX/bin/$lib.lib" "$PREFIX/lib/"; done
licence ffmpeg COPYING.GPLv2 LICENSE.md
recipe > "$PREFIX/.recipe"
echo "$FFMPEG_NAME built into $PREFIX"
