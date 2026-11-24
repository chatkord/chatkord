#!/usr/bin/env bash
# The builder image: where the gate's Linux stages and CI run.
#
# Usage: scripts/ci/builder.sh tag                  the recipe's digest, which names the image
#        scripts/ci/builder.sh build [--push]       build it here (and push it, to the registry CHATKORD_BUILDER names)
#        scripts/ci/builder.sh run CMD...           run CMD in it, the tree mounted at /src
# The image is `localhost/chatkord-builder:<digest>`, or `$CHATKORD_BUILDER:<digest>` when that names a registry's,
# pulled when absent. A container runtime is the machine's: `CHATKORD_CONTAINER`, else podman, else docker.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
DOCKERFILE=scripts/ci/builder.Containerfile
runtime="${CHATKORD_CONTAINER:-$(command -v podman > /dev/null && echo podman || echo docker)}"

# The recipe is what the builder and the stages it is made of read: the Containerfile and every source its COPY lines
# name, so the list is the Containerfile's own and nothing here restates it.
tag() {
  local sources
  sources=$(awk '/^COPY / && !/--from/ { for (i = 2; i < NF; i++) print $i }' "$DOCKERFILE")
  # Git Bash's sha256sum marks each path `*` (binary mode) where Linux's does not: --text names one recipe alike on both.
  # shellcheck disable=SC2086 # a word list of paths
  # A source in the protocol submodule is listed only through it: --recurse-submodules takes tracked files alone.
  { sha256sum --text "$DOCKERFILE"
    { git ls-files -z --cached --recurse-submodules -- $sources; git ls-files -z --others --exclude-standard -- $sources; } |
      sort -z -u | xargs -0 sha256sum --text; } | sha256sum | cut -c1-32
}

image() { echo "${CHATKORD_BUILDER:-localhost/chatkord-builder}:$(tag)"; }

# On Windows, Podman runs in a Linux machine: the default one (Podman's to say, by any name) is started when stopped.
# Making one is the machine's own set-up.
machine() {
  local list default running name
  list=$(podman machine list --format '{{.Default}} {{.Running}} {{.Name}}') ||
    { echo "FAIL: Podman could not list its machines"; exit 1; }
  while read -r default running name; do
    [[ "$default" == true ]] || continue
    name="${name%\*}" # the list marks the default's name with a star
    [[ "$running" == true ]] || podman machine start "$name" > /dev/null ||
      { echo "FAIL: Podman's machine $name did not start"; exit 1; }
    return
  done <<< "$list"
  echo "FAIL: Podman has no machine here; make one with `podman machine init`"
  exit 1
}
case "$(uname -s)" in MINGW* | MSYS*) if [[ "$runtime" == podman ]]; then machine; fi ;; esac

case "${1:-}" in
  tag) tag ;;
  build)
    shift
    format=(); [[ "$runtime" == podman ]] && format=(--format docker)
    "$runtime" build "${format[@]}" --target builder --tag "$(image)" --file "$DOCKERFILE" .
    [[ "${1:-}" == --push ]] && "$runtime" push "$(image)"
    echo "builder: $(image)" ;;
  run)
    shift; (( $# )) || { echo "usage: builder.sh run CMD..."; exit 2; }
    name=$(image)
    "$runtime" image exists "$name" 2> /dev/null || "$runtime" image inspect "$name" > /dev/null 2>&1 ||
      if [[ -n "${CHATKORD_BUILDER:-}" ]]; then "$runtime" pull "$name"; else "$0" build; fi
    # Windows' runtime takes a drive path, and Git Bash would rewrite /src as one.
    host="$ROOT"; vm=0
    case "$(uname -s)" in MINGW* | MSYS*) host="$(cygpath -m "$ROOT")"; export MSYS_NO_PATHCONV=1; vm=1 ;; esac
    args=(--rm --init --network host --security-opt label=disable -v "$host:/src" -v "$host/build/builder:/src/build")
    if (( vm )); then
      # The machine reaches the host's drive over 9p, which sets a file's time to the whole second: cargo's stamps then
      # read older than what a build script wrote just before, so a unit and all above it rebuild at random.
      tree=$(printf %s "$host" | sha256sum | cut -c1-12)
      args+=(-v "chatkord-target-$tree:/src/build/target" -v chatkord-cargo-registry:/usr/local/cargo/registry
        -v chatkord-cargo-git:/usr/local/cargo/git)
    else
      mkdir -p build/builder/cargo-registry
      args+=(-v "$host/build/builder/cargo-registry:/usr/local/cargo/registry")
    fi
    # The container's tests start the test server's image through this machine's runtime.
    if [[ "$runtime" == podman ]]; then
      socket=$(podman info --format '{{.Host.RemoteSocket.Path}}' 2> /dev/null || true)
      [[ -S "$socket" ]] && args+=(-v "$socket:/var/run/docker.sock")
    else
      args+=(-v /var/run/docker.sock:/var/run/docker.sock --user "$(id -u):$(id -g)" --group-add "$(stat -c %g /var/run/docker.sock)")
    fi
    # Passed by name, so a value (the privacy terms, the release's signing key) is never on a command line.
    passed=(GATE_BUILD_JOBS GATE_CHECK_JOBS NEXTEST_PROFILE CARGO_INCREMENTAL CARGO_PROFILE_DEV_DEBUG CARGO_LOG
      GATE_NO_CONTAINERS GATE_WORKSPACES GATE_KEEP_GOING CI PRIVACY_TERMS PRIVACY_TERMS_UNAVAILABLE CHATKORD_UPDATES
      CHATKORD_RELEASE_KEY CHATKORD_VERSION CHATKORD_SERVER_BASE)
    for var in "${passed[@]}"; do
      [[ -n "${!var:-}" ]] && args+=(-e "$var")
    done
    [[ -t 0 ]] && args+=(-it)
    exec "$runtime" run "${args[@]}" "$name" "$@" ;;
  *) echo "usage: builder.sh tag | build [--push] | run CMD..."; exit 2 ;;
esac
