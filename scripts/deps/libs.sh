#!/usr/bin/env bash
# Builds the three libraries the Linux FFmpeg and the carried video driver are built against, from pinned sources:
# the Vulkan headers, libdrm and libva. They are shipped with the client, so what runs is what was tested, not whatever
# the installing machine's distro carries; libva is static, linked into every binary that opens a VA-API device. Its
# own recipe, stamped on its own, so a change to FFmpeg's flags leaves them built.
#
# Usage: scripts/deps/libs.sh        (needs gcc, meson, ninja, cmake, pkg-config, git; scripts/deps/ffmpeg.sh runs it)
#        scripts/deps/libs.sh --recipe   the digests of what it builds from, which $PREFIX/.libs-recipe holds
# Output: build/deps (headers, libdrm shared, libva static, pkg-config files, each project's licence in share/licenses).
set -euo pipefail

# Each pin is a tag AND the commit it must be, so a moved tag fails the build instead of changing it.
LIBDRM_TAG=libdrm-2.4.134;      LIBDRM_COMMIT=e984d448b8b17aab853369e6c203e53719f46de1
LIBVA_TAG=2.24.1;               LIBVA_COMMIT=710eb465c6277ee2cac3e6948767b01eebe7e77a
VULKAN_HEADERS_TAG=v1.4.368;    VULKAN_HEADERS_COMMIT=88487f687f3100ebe1d3559a8a34d971ee85b977

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
recipe() {
  (cd "$ROOT" && sha256sum scripts/deps/libs.sh scripts/deps/source.sh scripts/package/linux.env \
    scripts/package/va-system-drivers.txt)
  # An install names its absolute prefix (its pkg-config files), so a tree moved elsewhere builds it again.
  echo "in $ROOT"
}
[ "${1:-}" = --recipe ] && { recipe; exit 0; }
# shellcheck source=../package/linux.env
. "$ROOT/scripts/package/linux.env"
PREFIX=${PREFIX:-$ROOT/build/deps}
SRC=${SRC:-$ROOT/build/deps-src}
mkdir -p "$PREFIX" "$SRC"
[ "$(cat "$PREFIX/.libs-recipe" 2>/dev/null)" = "$(recipe)" ] && { echo "libs are current in $PREFIX"; exit 0; }
export PKG_CONFIG_PATH=$PREFIX/lib/pkgconfig:$PREFIX/share/pkgconfig
export PKG_CONFIG_LIBDIR=$PKG_CONFIG_PATH   # only ours: the host's libva and libdrm must not win
# shellcheck source=source.sh
. "$ROOT/scripts/deps/source.sh"

fetch vulkan-headers https://github.com/KhronosGroup/Vulkan-Headers.git "$VULKAN_HEADERS_TAG" "$VULKAN_HEADERS_COMMIT"
made_here "$SRC/vulkan-headers/build"
cmake -S "$SRC/vulkan-headers" -B "$SRC/vulkan-headers/build" -G Ninja -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -DVULKAN_HEADERS_ENABLE_MODULE=OFF -DVULKAN_HEADERS_ENABLE_TESTS=OFF >/dev/null
cmake --build "$SRC/vulkan-headers/build" --target install >/dev/null

fetch libdrm https://gitlab.freedesktop.org/mesa/drm.git "$LIBDRM_TAG" "$LIBDRM_COMMIT"
# libdrm_amdgpu is how the carried Mesa video driver reaches an AMD GPU.
made_here "$SRC/libdrm/build"
meson setup --reconfigure "$SRC/libdrm/build" "$SRC/libdrm" --prefix="$PREFIX" --libdir=lib --buildtype=release \
  -Dintel=disabled -Dradeon=disabled -Damdgpu=enabled -Dnouveau=disabled -Dvmwgfx=disabled -Dc_link_args='-Wl,-rpath,$ORIGIN' \
  -Dtests=false -Dman-pages=disabled -Dvalgrind=disabled -Dcairo-tests=disabled >/dev/null
ninja -C "$SRC/libdrm/build" install >/dev/null
# libdrm has no licence file: its text is the header's.
mkdir -p "$PREFIX/share/licenses/libdrm"
sed -n '/Copyright 1999/,/^ \*\/$/p' "$SRC/libdrm/xf86drm.h" > "$PREFIX/share/licenses/libdrm/COPYING"

fetch libva https://github.com/intel/libva.git "$LIBVA_TAG" "$LIBVA_COMMIT"
# The package's driver folders, not the prefix's. Linked in: no system libva can be the one a binary loads. libva's
# meson makes only shared libraries, so its own objects are archived and its pkg-config files name what they need.
made_here "$SRC/libva/build"
meson setup --reconfigure "$SRC/libva/build" "$SRC/libva" --prefix="$PREFIX" --libdir=lib --buildtype=release \
  -Ddriverdir="$CHATKORD_VA_DRIVERS" -Dwith_x11=no -Dwith_glx=no -Dwith_wayland=no >/dev/null
ninja -C "$SRC/libva/build" install >/dev/null
rm -f "$PREFIX"/lib/libva*.so* "$PREFIX"/lib/libva*.a
ar rcs "$PREFIX/lib/libva.a" "$SRC"/libva/build/va/libva.so.*.p/*.o
ar rcs "$PREFIX/lib/libva-drm.a" "$SRC"/libva/build/va/libva-drm.so.*.p/*.o
sed -i 's/^Libs: -L${libdir} -lva$/Libs: -L${libdir} -lva -ldl/' "$PREFIX/lib/pkgconfig/libva.pc"
sed -i 's/^Libs: -L${libdir} -lva-drm$/Libs: -L${libdir} -lva-drm -ldrm/' "$PREFIX/lib/pkgconfig/libva-drm.pc"
grep -q -- '-lva -ldl$' "$PREFIX/lib/pkgconfig/libva.pc" && grep -q -- '-lva-drm -ldrm$' "$PREFIX/lib/pkgconfig/libva-drm.pc"
licence libva COPYING
recipe > "$PREFIX/.libs-recipe"
echo "vulkan-headers, libdrm and libva built into $PREFIX"
