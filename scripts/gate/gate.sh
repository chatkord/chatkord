#!/usr/bin/env bash
# The gate (`mise run gate`): the fast checks, the native dependencies, then every
# workspace. The fuzz targets' lint and the shaders run in the builder image, as CI runs them; the client's run
# on this machine, whose hardware its lab tests need. A tool that is absent fails the gate; nothing here reports a pass
# it did not earn. A pass on a clean tree is recorded by its tree's id with what it left out
# (protocol/scripts/gate/record.py), for the next gate and for a push.
set -euo pipefail
cd "$(dirname "$0")/../.."
# A reference image is written instead of compared while UPDATE_REFS is set, so every such test would pass.
[[ -z "${UPDATE_REFS+set}" ]] || { echo "FAIL: UPDATE_REFS is set, so the reference images would be written, not compared"; exit 1; }
case "$(uname -s)" in MINGW* | MSYS*) windows=1; system=windows ;; *) windows=0; system=linux ;; esac
in_builder="${CHATKORD_IN_BUILDER:-}"

# The gate shares the machine with whatever else is running. The defaults are this machine's own: GATE_MEM is five
# eighths of its memory; clippy runs one rustc per core; a test build links ~500 MB binaries at about 1.1 GB each, so
# GATE_BUILD_JOBS is half the cores and never more than one per 2 GB of that ceiling. Each is an override when set.
# Windows commits memory without overcommit, so twelve links beside Podman's machine ran out (os error 1455).
mem_gb=$(( $(awk '/^MemTotal:/ {print $2}' /proc/meminfo) * 5 / 8 / 1024 / 1024 ))
build_jobs=$(( $(nproc) / 2 < mem_gb / 2 ? $(nproc) / 2 : mem_gb / 2 ))
set_check_jobs="${GATE_CHECK_JOBS:-}" set_build_jobs="${GATE_BUILD_JOBS:-}"
export GATE_CHECK_JOBS="${GATE_CHECK_JOBS:-$(nproc)}"
export GATE_BUILD_JOBS="${GATE_BUILD_JOBS:-$(( build_jobs > 0 ? build_jobs : 1 ))}"
# On Linux it gets its own memory ceiling too: inside a cgroup scope the kernel kills the gate's own heaviest process
# when it goes over, never something outside it. A runner or the builder is single-purpose and sets GATE_SCOPED.
if (( ! windows )); then
  if [[ -z "${GATE_SCOPED:-}" && -z "$in_builder" ]]; then
    for tool in systemd-run ld.lld; do
      command -v "$tool" > /dev/null || { echo "FAIL: $tool is required (the memory ceiling, the client's linker)"; exit 1; }
    done
    export GATE_SCOPED=1
    exec systemd-run --user --scope --quiet --collect \
      -p "MemoryMax=${GATE_MEM:-${mem_gb}G}" -p MemorySwapMax=0 -- "$0" "$@"
  fi
fi
# In the builder already (CI), a stage runs here; elsewhere it runs in the builder. On Windows that is Podman's machine,
# another machine, which sizes its own jobs unless a person set them.
builder() {
  if [[ -n "$in_builder" ]]; then
    "$@"
  elif (( windows )); then
    GATE_CHECK_JOBS="$set_check_jobs" GATE_BUILD_JOBS="$set_build_jobs" scripts/ci/builder.sh run "$@"
  else
    scripts/ci/builder.sh run "$@"
  fi
}

started=$SECONDS
all=0; keep=0
for arg in "$@"; do
  case "$arg" in --all) all=1 ;; --keep-going) keep=1 ;; *) echo "usage: gate.sh [--all] [--keep-going]"; exit 2 ;; esac
done
# --keep-going: each step runs though one before it failed, every failure is named at the end, and nothing is recorded.
failed=()
step() { if (( keep )); then "$@" || failed+=("$*"); else "$@"; fi; }
(( keep )) && export GATE_KEEP_GOING=1
bash scripts/gate/pinned.sh
# shellcheck source=../../protocol/scripts/find-python.sh
. protocol/scripts/find-python.sh
[[ -n "${PYTHON:-}" ]] || { echo "FAIL: no Python $PYTHON_FLOOR+"; exit 1; }
clean_tree() { [[ -z "$(git status --porcelain)" ]] && git rev-parse 'HEAD^{tree}'; }
tree="$(clean_tree || true)"
# What to run: every workspace, those CI's change reaches (GATE_WORKSPACES), or those the change since the nearest
# commit whose tree passed whole on this system reaches (CI's own rule), so an unreached workspace stands on that pass.
left=(); base=-
[[ -n "${NEXTEST_PROFILE:-}" ]] && left+=("the tests as profile $NEXTEST_PROFILE runs them")
if [[ -n "${GATE_WORKSPACES+set}" ]]; then
  left+=("every workspace but: ${GATE_WORKSPACES:-none}")
elif (( ! all )); then
  commit="$($PYTHON -I protocol/scripts/gate/record.py base "$system")"
  if [[ -n "$commit" ]]; then
    GATE_WORKSPACES="$($PYTHON -I scripts/ci/changes.py "$commit")"; export GATE_WORKSPACES
    base="$(git rev-parse "$commit^{tree}")"
    echo "gate: ${commit:0:12} passed; this change reaches: ${GATE_WORKSPACES:-no workspace}"
  fi
fi

if (( keep )); then step mise run --continue-on-error gate:fast; else mise run gate:fast; fi

# The native dependencies whose recipes changed, before any cargo call in client/.
scripts/deps/native.sh
# Windows finds the FFmpeg DLLs on PATH (cargo adds only directories inside target/), named /d/... as a drive's colon
# would split it.
(( windows )) && PATH="$(cygpath -u "$PWD")/build/deps/bin:$PATH"

# Every workspace, or those CI's change reaches (GATE_WORKSPACES, scripts/ci/changes.py).
workspaces=" ${GATE_WORKSPACES-$($PYTHON -I scripts/ci/changes.py --all)} "
wanted() { [[ "$workspaces" == *" $1 "* ]]; }
if [[ -n "${GATE_NO_CONTAINERS:-}" ]]; then
  echo "gate: the shaders and the fuzz targets' lint skipped (GATE_NO_CONTAINERS: the builder cannot run here)"
  left+=("the shaders, the fuzz targets' lint and the client's server and lab tests (GATE_NO_CONTAINERS)")
else
  if wanted client; then step builder scripts/gate/shaders.sh; else echo "gate: the shaders not reached by this change, skipped"; fi
  if wanted fuzz; then step builder scripts/gate/fuzz-lint.sh; else echo "gate: fuzz not reached by this change, skipped"; fi
  # Windows builds the client for Windows; the builder lints its Linux code too, so a commit made here is gated whole
  # but for what needs a Linux machine's hardware (VA-API, PipeWire, a GPU), which that machine's gate runs.
  if (( windows )) && wanted client; then
    step builder bash -c 'cd client && cargo clippy --locked --workspace --all-targets -- -D warnings'
    echo "gate: the client's tests on Linux's hardware are left to a Linux machine's gate"
  fi
fi
if wanted client; then step scripts/gate/workspace.sh client; else echo "gate: client not reached by this change, skipped"; fi

# Recorded only for the tree it ran on from start to end: a commit made meanwhile is not what it tested.
if (( ${#failed[@]} )); then
  echo "GATE: FAIL, ${#failed[@]} step(s), not recorded ($(( SECONDS - started )) s):"
  printf '  %s\n' "${failed[@]}"
  exit 1
fi
if [[ -n "$tree" && "$(clean_tree || true)" == "$tree" ]]; then
  $PYTHON -I protocol/scripts/gate/record.py write "$tree" "$system" "$base" "${left[@]}"
  echo "GATE: pass, tree $tree, recorded${left[*]:+, leaving out: ${left[*]}} ($(( SECONDS - started )) s)"
else
  echo "GATE: pass, but the tree was not clean or moved while it ran, so not recorded for a push ($(( SECONDS - started )) s)"
fi
