#!/usr/bin/env bash
# The gate's own rules answer their self-tests: what a change reaches (CI's too) and what a pass records; and how a
# package's notices reflow a licence's text; what the weekly dependency check finds; what the next release is;
# what a commit's clippy lints; and that protocol/ is the pin.
set -euo pipefail
cd "$(dirname "$0")/../.."
# shellcheck source=../../protocol/scripts/find-python.sh
. protocol/scripts/find-python.sh
[[ -n "${PYTHON:-}" ]] || { echo "FAIL: no Python $PYTHON_FLOOR+"; exit 1; }
$PYTHON -I scripts/ci/changes.py --self-test
$PYTHON -I protocol/scripts/gate/record.py --self-test
$PYTHON -I scripts/package/notices.py --self-test
$PYTHON -I scripts/deps/released.py --self-test
$PYTHON -I scripts/release/next.py --self-test
$PYTHON -I scripts/gate/clippy_touched.py --self-test
bash scripts/gate/pinned.sh --self-test
