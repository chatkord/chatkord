#!/usr/bin/env bash
# Builds the GLSL compiler that turns the client's compute shaders into the SPIR-V checked in beside
# them (scripts/shaders.sh), from a pinned source, so the gate compares against the same bytes on
# every machine: SPIR-V names its generator, and another version writes other bytes. Nothing of it
# ships; the client carries no shader compiler.
#
# Usage: scripts/deps/glslang.sh        (needs cmake, ninja, a C++ compiler, git)
# Output: build/deps/bin/glslangValidator
set -euo pipefail

# A tag AND the commit it must be, so a moved tag fails the build instead of changing it.
GLSLANG_TAG=16.6.0; GLSLANG_COMMIT=e1b562a8bed273a02f30b59b66a5d499793cede5

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PREFIX=${PREFIX:-$ROOT/build/deps}
SRC=${SRC:-$ROOT/build/deps-src}
mkdir -p "$PREFIX" "$SRC"
# shellcheck source=source.sh
. "$ROOT/scripts/deps/source.sh"

fetch glslang https://github.com/KhronosGroup/glslang.git "$GLSLANG_TAG" "$GLSLANG_COMMIT"
# Without SPIRV-Tools' optimizer (and so without its sources): the shaders are small and the output is not optimised.
made_here "$SRC/glslang/build"
cmake -S "$SRC/glslang" -B "$SRC/glslang/build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -DENABLE_OPT=OFF -DENABLE_HLSL=OFF -DENABLE_GLSLANG_BINARIES=ON -DGLSLANG_TESTS=OFF -DBUILD_EXTERNAL=OFF >/dev/null
cmake --build "$SRC/glslang/build" --target install >/dev/null
sha256sum scripts/deps/glslang.sh > "$PREFIX/.glslang-recipe"
echo "glslang $GLSLANG_TAG built into $PREFIX"
