#!/usr/bin/env bash
# The third-party notices a package carries, in its licences folder: the carried projects' own
# texts as files, our licence's text, and notices.json, which the About page reads (scripts/package/notices.py writes
# it, failing on any text that is missing or any Rust package whose licence cargo-about cannot read).
#
# Usage: scripts/package/notices.sh <folder> <version> <package manifest>...
#        after a release build of each package, whose dependencies cargo-about then reads offline.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
(( $# >= 3 )) || { echo "usage: notices.sh <folder> <version> <manifest>..."; exit 2; }
folder=$1
mkdir -p "$folder"
cp -r "$ROOT/build/deps/share/licenses/." "$folder/"
install -D -m 644 "$ROOT/build/deps-src/webrtc/prebuilt/LICENSE.md" "$folder/libwebrtc/LICENSE.md"
install -D -m 644 "$ROOT/protocol/proto/data/LICENSE-SecLists.txt" "$folder/SecLists/LICENSE"
for text in "$ROOT"/client/shell/ui/assets/fonts/LICENSE-*.txt "$ROOT"/client/shell/ui/assets/icons/LICENSE-*.txt; do
  install -m 644 "$text" "$folder/${text##*/LICENSE-}"
done
install -m 644 "$ROOT/LICENSE-GPL-3.0.txt" "$folder/"
# shellcheck source=../../protocol/scripts/find-python.sh
. "$ROOT/protocol/scripts/find-python.sh"
[ -n "$PYTHON" ] || { echo "FAIL: no Python $PYTHON_FLOOR+ to write the notices"; exit 1; }
$PYTHON -I "$ROOT/scripts/package/notices.py" "$@"
