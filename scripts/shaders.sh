#!/usr/bin/env bash
# Compiles the client's compute shaders to what is checked in beside them, as text (a Rust array of 32-bit words,
# which the guard can read and a diff can show), so a build needs no shader compiler and the client carries none: the
# GLSL ones (media/src/encode/screen/shader/*.comp) to SPIR-V by the pinned compiler that scripts/deps/glslang.sh
# builds, and the HLSL ones (*.hlsl) to DXIL by the pinned compiler that scripts/deps/dxc.sh fetches. The gate runs
# this into a scratch directory and compares, the way it does for the generated wire types.
#
# Usage: scripts/shaders.sh [output-dir]     (default: beside each source)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Rebuilt when its recipe changed, like the FFmpeg the client links.
if [[ "$(cat build/deps/.glslang-recipe 2>/dev/null)" != "$(sha256sum scripts/deps/glslang.sh)" ]]; then
  scripts/deps/glslang.sh > /dev/null
fi
if [[ "$(cat build/deps/.dxc-recipe 2>/dev/null)" != "$(sha256sum scripts/deps/dxc.sh)" ]]; then
  scripts/deps/dxc.sh > /dev/null
fi

for source in client/media/src/encode/screen/shader/*.comp; do
  out="${1:-$(dirname "$source")}"
  mkdir -p "$out"
  words="$(mktemp)"
  build/deps/bin/glslangValidator -V --target-env vulkan1.3 -x -o "$words" "$source" >&2
  { echo "["; cat "$words"; echo "]"; } > "$out/$(basename "${source%.comp}").spv.inc"
  rm -f "$words"
done

# DXIL is a container of 32-bit words; the debug and reflection parts are left out, so the bytes are the shader's alone.
for source in client/media/src/encode/screen/shader/*.hlsl; do
  out="${1:-$(dirname "$source")}"
  mkdir -p "$out"
  binary="$(mktemp)"
  LD_LIBRARY_PATH="$PWD/build/deps/dxc/lib" build/deps/dxc/bin/dxc -T cs_6_0 -E main -O3 -Qstrip_debug -Qstrip_reflect \
    -Fo "$binary" "$source" >&2
  {
    echo "["
    od -An -v -tx4 -w32 "$binary" | sed -E 's/^ +//; s/ +/,0x/g; s/^/\t0x/; s/$/,/'
    echo "]"
  } > "$out/$(basename "${source%.hlsl}").dxil.inc"
  rm -f "$binary"
done
