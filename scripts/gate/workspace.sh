#!/usr/bin/env bash
# One workspace linted and tested: clippy on the product as shipped, then with every target (which unifies the
# dev-dependencies' features and hides a warning only a plain build shows), the tests under nextest with the tier's
# profile, then the documentation with its warnings denied (a link that leads nowhere).
#
# Usage: scripts/gate/workspace.sh DIR
set -euo pipefail
cd "$(dirname "$0")/../.."
dir="$1"
gate_config="$PWD/scripts/gate/nextest.toml"
profile="${NEXTEST_PROFILE:-lab}"
filter=()
# The client's tests that start a server run it from the image this tree builds. A machine that cannot run Linux
# containers sets GATE_NO_CONTAINERS, and those tests (and the lab ones, which may start one too) are left out.
if [[ "$dir" == client ]]; then
  if [[ -n "${GATE_NO_CONTAINERS:-}" ]]; then
    filter=(-E 'not (group(server) | group(lab))')
    echo "gate: the client's tests that start a server or need hardware skipped (GATE_NO_CONTAINERS)"
  else
    scripts/test-image.sh
  fi
fi
# Under the gate's --keep-going (GATE_KEEP_GOING), each step runs though one before it failed, every test runs, and the
# workspace fails at the end.
keep="${GATE_KEEP_GOING:-}"
failed=0
step() { if [[ -n "$keep" ]]; then "$@" || { echo "FAIL ($dir): $*"; failed=1; }; else "$@"; fi; }
[[ -n "$keep" ]] && filter+=(--no-fail-fast)
cd "$dir"
step cargo clippy --locked -j "${GATE_CHECK_JOBS:-$(nproc)}" --workspace -- -D warnings
step cargo clippy --locked -j "${GATE_CHECK_JOBS:-$(nproc)}" --workspace --all-targets -- -D warnings
step cargo nextest run --locked --workspace --build-jobs "${GATE_BUILD_JOBS:-$(nproc)}" --profile "$profile" --no-tests=warn   --tool-config-file "gate:$gate_config" "${filter[@]}"
RUSTDOCFLAGS="-D warnings" step cargo doc --locked -j "${GATE_CHECK_JOBS:-$(nproc)}" --no-deps --workspace
exit "$failed"
