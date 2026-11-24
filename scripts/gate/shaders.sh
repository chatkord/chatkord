#!/usr/bin/env bash
# The SPIR-V and DXIL beside each compute shader are what the pinned compilers write. Runs in the builder, which carries it.
set -euo pipefail
cd "$(dirname "$0")/../.."
fresh="$(mktemp -d)"; trap 'rm -rf "$fresh"' EXIT
scripts/shaders.sh "$fresh" 2> /dev/null
for built in "$fresh"/*.spv.inc "$fresh"/*.dxil.inc; do
  cmp -s "$built" "client/media/src/encode/screen/shader/$(basename "$built")" ||
    { echo "FAIL: $(basename "$built") is not what scripts/shaders.sh compiles"; exit 1; }
done
