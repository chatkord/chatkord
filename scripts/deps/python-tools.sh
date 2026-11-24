#!/usr/bin/env bash
# Fetches the Python tools the Windows FFmpeg build installs as wheels, each checked against the digest python-tools.txt
# holds, into one folder; the pip that build runs installs from that folder alone (ffmpeg-windows.sh).
#
# Usage: scripts/deps/python-tools.sh          fetch for this system (skipped while the file is unchanged)
#        scripts/deps/python-tools.sh --lock   write the digests PyPI publishes for the versions in the file
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
LOCK=$ROOT/scripts/deps/python-tools.txt
OUT=${WHEELS:-$ROOT/build/deps-src/wheels}
# shellcheck source=../../protocol/scripts/find-python.sh
. "$ROOT/protocol/scripts/find-python.sh"
[ -n "$PYTHON" ] || { echo "FAIL: no Python $PYTHON_FLOOR+ to fetch the build's Python tools"; exit 1; }

if [ "${1:-}" = --lock ]; then
  # Every wheel of the pinned version a build of ours can take: any system, Linux x86-64, Windows x64.
  $PYTHON -I - "$LOCK" <<'EOF'
import json, re, sys, urllib.request
path = sys.argv[1]
head, pins = [], []
for line in open(path, encoding="utf-8"):
    line = line.rstrip("\n")
    if line.startswith("#") and not pins:
        head.append(line)
    elif re.match(r"^[A-Za-z0-9_.-]+==", line):
        pins.append(line.rstrip(" \\"))
wanted = re.compile(r"-(py3-none-any|.*manylinux.*x86_64|.*win_amd64)\.whl$")
out = head[:]
for pin in pins:
    name, version = re.match(r"^([A-Za-z0-9_.-]+)==([^ ;]+)", pin).groups()
    release = json.load(urllib.request.urlopen(f"https://pypi.org/pypi/{name}/{version}/json"))
    digests = sorted(f["digests"]["sha256"] for f in release["urls"] if wanted.search(f["filename"]))
    if not digests:
        sys.exit(f"FAIL: {name} {version} has no wheel for these systems")
    out.append(pin + " \\")
    out += [f"    --hash=sha256:{d}" + (" \\" if i < len(digests) - 1 else "") for i, d in enumerate(digests)]
open(path, "w", encoding="utf-8").write("\n".join(out) + "\n")
EOF
  echo "wrote $LOCK"
  exit 0
fi

stamp=$(sha256sum "$LOCK" | cut -d' ' -f1)
[ "$(cat "$OUT/.lock" 2>/dev/null)" = "$stamp" ] && exit 0
# Only what this file names, each checked: a wheel left from another lock is not one.
rm -rf "$OUT"
PIP_NO_INDEX= PIP_FIND_LINKS= $PYTHON -m pip download -q --require-hashes --no-deps --only-binary :all: -r "$LOCK" -d "$OUT"
echo "$stamp" > "$OUT/.lock"
echo "Python tools checked into $OUT"
