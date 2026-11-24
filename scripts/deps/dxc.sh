#!/usr/bin/env bash
# Fetches the HLSL compiler that turns the client's D3D12 compute shaders into the DXIL checked in beside them
# (scripts/shaders.sh), from a pinned release, so the gate compares against the same bytes on every machine: DXIL
# names its compiler, and another version writes other bytes. Microsoft's Linux release carries libdxil, which signs
# the shader on Linux too. Nothing of it ships; the client carries no shader compiler.
#
# Usage: scripts/deps/dxc.sh        (needs curl, tar, sha256sum)
# Output: build/deps/dxc/bin/dxc, with its libraries in build/deps/dxc/lib
set -euo pipefail

# A release AND the digest its Linux archive must have, so a moved asset fails the fetch instead of changing it.
DXC_TAG=v1.9.2609
DXC_FILE=linux_dxc_2026_09_28.x86_x64.tar.gz
DXC_SHA256=96faadc7f5c282d2ffda49804beb4c3ee38127bc252b723234e3c5cdf7aa39a1

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PREFIX=${PREFIX:-$ROOT/build/deps}
SRC=${SRC:-$ROOT/build/deps-src}
mkdir -p "$PREFIX" "$SRC"
# shellcheck source=source.sh
. "$ROOT/scripts/deps/source.sh"

archive="$SRC/$DXC_FILE"
[ -f "$archive" ] || curl -fsSL -o "$archive" "https://github.com/microsoft/DirectXShaderCompiler/releases/download/$DXC_TAG/$DXC_FILE"
[ "$(sha256sum "$archive" | cut -d' ' -f1)" = "$DXC_SHA256" ] || { rm -f "$archive"; echo "FAIL: $DXC_FILE is not $DXC_SHA256"; exit 1; }
rm -rf "$PREFIX/dxc" && mkdir -p "$PREFIX/dxc"
tar -xzf "$archive" -C "$PREFIX/dxc"
chmod +x "$PREFIX/dxc/bin/dxc"
sha256sum scripts/deps/dxc.sh > "$PREFIX/.dxc-recipe"
echo "dxc $DXC_TAG fetched into $PREFIX/dxc"
