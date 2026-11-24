#!/usr/bin/env bash
# Builds the one video driver the Linux package carries: Mesa's VA-API driver for AMD GPUs (radeonsi) with the codecs a
# share sends, H.264 and AV1, since a distribution may build its own without H.264 (Fedora does). Only that driver: the window
# and Vulkan video use the system's. LLVM is linked in, so the driver asks the system for nothing a fresh install
# lacks; it reaches the GPU through the libdrm_amdgpu scripts/deps/libs.sh built.
#
# Usage: scripts/deps/mesa.sh   (after scripts/deps/libs.sh; needs meson, ninja, bison, flex, Python's mako and
#        yaml, LLVM's static libraries, libelf, zlib, zstd and expat)
# Output: build/deps/lib/dri (the driver and the library it is), Mesa's licences in build/deps/share/licenses/mesa.
set -euo pipefail

MESA_TAG=mesa-26.3.0; MESA_COMMIT=0a6fa60abb737d26ac8a316a4aad7c9ddf64f9e4

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PREFIX=${PREFIX:-$ROOT/build/deps}
SRC=${SRC:-$ROOT/build/deps-src}
mkdir -p "$PREFIX" "$SRC"
export PKG_CONFIG_PATH=$PREFIX/lib/pkgconfig:$PREFIX/share/pkgconfig
# shellcheck source=source.sh
. "$ROOT/scripts/deps/source.sh"

fetch mesa https://gitlab.freedesktop.org/mesa/mesa.git "$MESA_TAG" "$MESA_COMMIT"
# The driver finds the library it is beside it and libdrm in the folder above.
made_here "$SRC/mesa/build"
meson setup --reconfigure "$SRC/mesa/build" "$SRC/mesa" --prefix="$PREFIX" --libdir=lib --buildtype=release \
  -Dgallium-drivers=radeonsi -Dvulkan-drivers= -Dgallium-va=enabled -Dvideo-codecs=h264dec,h264enc,av1dec,av1enc \
  -Dplatforms= -Dglx=disabled -Degl=disabled -Dgbm=disabled -Dopengl=false -Dgles1=disabled -Dgles2=disabled \
  -Dllvm=enabled -Dshared-llvm=disabled -Dgallium-rusticl=false -Dgallium-mediafoundation=disabled -Dteflon=false \
  -Dxmlconfig=disabled -Dvalgrind=disabled -Dlibunwind=disabled -Dlmsensors=disabled -Dbuild-tests=false \
  -Dc_link_args='-Wl,-rpath,$ORIGIN:$ORIGIN/..' -Dcpp_link_args='-Wl,-rpath,$ORIGIN:$ORIGIN/..' >/dev/null
ninja -C "$SRC/mesa/build" install >/dev/null
licence mesa docs/license.rst licenses
echo "Mesa's VA-API driver ($MESA_TAG) built into $PREFIX/lib/dri"
