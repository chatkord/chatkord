#!/usr/bin/env bash
# The pre-commit hook's clippy: every target of the packages a commit touches and of
# those that depend on them (scripts/gate/clippy_touched.py), warnings denied, where the gate lints each workspace:
# the fuzz targets in the builder, the client on this machine after the native dependencies.
set -euo pipefail
cd "$(dirname "$0")/../.."
# shellcheck source=../../protocol/scripts/find-python.sh
. protocol/scripts/find-python.sh
[[ -n "${PYTHON:-}" ]] || { echo "FAIL: no Python $PYTHON_FLOOR+"; exit 1; }
builder() { if [[ -n "${CHATKORD_IN_BUILDER:-}" ]]; then "$@"; else scripts/ci/builder.sh run "$@"; fi; }

lines="$($PYTHON -I scripts/gate/clippy_touched.py)"
[[ -n "$lines" ]] || { echo "clippy: no package touched"; exit 0; }
if grep -q '^client ' <<< "$lines"; then scripts/deps/native.sh; fi
# The lines on their own descriptor: the builder's container would read the loop's standard input.
while read -r -u 3 workspace packages; do
  echo "clippy: $workspace: $packages"
  selected=--workspace
  # shellcheck disable=SC2086 # a word list of packages
  [[ "$packages" == --workspace ]] || selected=$(printf -- '-p %s ' $packages)
  # As the gate leaves them where the builder cannot run, to a machine where it can.
  if [[ -n "${GATE_NO_CONTAINERS:-}" && "$workspace" == fuzz ]]; then
    echo "clippy: $workspace skipped (GATE_NO_CONTAINERS: the builder cannot run here)"
    continue
  fi
  case "$workspace" in
    fuzz) builder scripts/gate/fuzz-lint.sh ;;
    # shellcheck disable=SC2086
    *) (cd "$workspace" && cargo clippy --locked $selected --all-targets -- -D warnings) ;;
  esac
done 3<<< "$lines"
