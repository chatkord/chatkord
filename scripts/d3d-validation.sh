#!/usr/bin/env bash
# A nextest setup script (client/.config/nextest.toml): the tests it covers open their D3D devices under the D3D debug
# layer and end on an error it reports (`tier::d3d_validated`), where Windows has the layer; elsewhere nothing is set.
set -euo pipefail
case "$(uname -s)" in
MINGW* | MSYS* | CYGWIN*) ;;
*) exit 0 ;;
esac
if [ ! -e "${SYSTEMROOT:-C:/Windows}/System32/d3d11_3SDKLayers.dll" ]; then
  echo "D3D's debug layer is not installed: add Graphics Tools in Settings, System, Optional features." >&2
  exit 0
fi
# The message ids passed over, each another project's known issue: FFmpeg's D3D12 download moving a picture to the
# copy-source layout on a copy queue, which takes only the common one (1334).
echo "D3D_VALIDATION=1334" >> "$NEXTEST_ENV"
