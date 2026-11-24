#!/usr/bin/env bash
# The fuzz targets linted, never run, in the builder: from fuzz/, as cargo-fuzz builds them (`--cfg fuzzing`, which the
# client crates' entries need), in a tree of their own.
set -euo pipefail
cd "$(dirname "$0")/../../fuzz"
CARGO_TARGET_DIR="$PWD/../build/target/fuzz-lint" RUSTFLAGS="--cfg fuzzing" \
  cargo clippy --locked --all-targets -- -D warnings
