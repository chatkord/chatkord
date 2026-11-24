#!/usr/bin/env bash
# The privacy guard over the tree and its self-test, and the commit-message check's self-test.
set -euo pipefail
cd "$(dirname "$0")/../.."
bash protocol/scripts/privacy/run.sh
bash protocol/scripts/privacy/run.sh --self-test
# shellcheck source=../../protocol/scripts/find-python.sh
. protocol/scripts/find-python.sh
$PYTHON -I protocol/scripts/check_commit_msg.py --self-test
