#!/usr/bin/env bash
# Builds the image the client's server tests start, `localhost/chatkord-server:test`: a server image (the runtime and
# the release server) with sqlite3 and the test relay (client/testserver) on it, which the gate makes before the
# client's workspace (scripts/gate/workspace.sh, in CI too). The base is, in this order: `$CHATKORD_SERVER_BASE` when it
# names an image; else, with the server's repository beside this one, what its `scripts/image.sh --test` builds
# (`localhost/chatkord-server:test-base`); else the private server image of the branch checked out here,
# `ghcr.io/chatkord/chatkord-server:latest` on `main` and `:develop` on any other.
# A container runtime is assumed present and set up: `CHATKORD_CONTAINER`, else podman, else docker.
# The relay is built in the builder (scripts/ci/builder.sh) in release, incrementally, so an unchanged tree is a cache hit.
#
# Usage: scripts/test-image.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
NAME=localhost/chatkord-server
# On every image this builds, so a prune finds only its own leftovers.
MARK=org.chatkord.build=test-image

(( $# == 0 )) || { echo "usage: test-image.sh"; exit 2; }
runtime="${CHATKORD_CONTAINER:-$(command -v podman > /dev/null && echo podman || echo docker)}"
"$runtime" info > /dev/null 2>&1 || {
  echo "FAIL: no container runtime answers ($runtime); start one, or name it in CHATKORD_CONTAINER"
  exit 1
}
# Podman writes OCI images by default, which drop the health check; Docker's format keeps it.
format=(); [[ "$runtime" == podman ]] && format=(--format docker)

base="${CHATKORD_SERVER_BASE:-}"
if [[ -z "$base" ]]; then
  if [[ -d "$ROOT/../chatkord-server" ]]; then
    "$ROOT/../chatkord-server/scripts/image.sh" --test
    base=localhost/chatkord-server:test-base
  else
    [[ "$(git -C "$ROOT" branch --show-current)" == main ]] && tag=latest || tag=develop
    base=ghcr.io/chatkord/chatkord-server:$tag
    "$runtime" pull --quiet "$base" > /dev/null 2>&1 ||
      { echo "FAIL: the server's test image could not be pulled: log in to ghcr.io, or set CHATKORD_SERVER_BASE"; exit 1; }
  fi
fi

build_relay='cd client && cargo build --locked --release -p testserver --bin chatkord-test-relay &&
  cp "$CARGO_TARGET_DIR/release/chatkord-test-relay" ../build/chatkord-test-relay'
# The builder's build/ is build/builder outside it, and its target tree may be a volume.
if [[ -n "${CHATKORD_IN_BUILDER:-}" ]]; then
  bash -c "$build_relay"; built="$ROOT/build"
else
  "$ROOT/scripts/ci/builder.sh" run bash -c "$build_relay"; built="$ROOT/build/builder"
fi
context="$ROOT/build/test-image"; rm -rf "$context"; mkdir -p "$context"
cp "$built/chatkord-test-relay" "$context/"
user="$("$runtime" image inspect --format '{{.Config.User}}' "$base")"
[[ -n "$user" ]] || { echo "FAIL: the server image $base names no user, so its tests would run it as root"; exit 1; }
"$runtime" build --quiet "${format[@]}" --label "$MARK" --build-arg "BASE=$base" --build-arg "SERVER_USER=$user" --tag "$NAME:test" \
  --file "$ROOT/client/testserver/Containerfile" "$context" > /dev/null
"$runtime" image prune --force --filter "label=$MARK" > /dev/null
echo "image: $NAME:test is the server base $base with this tree's test relay ($runtime)"
