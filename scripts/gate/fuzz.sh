#!/usr/bin/env bash
# The parsers an untrusted peer feeds, fuzzed: `mise run fuzz [TARGET] [SECONDS]` runs
# TARGET, or every target the fuzz workspace lists, for SECONDS each (60 unless given), from its tracked seeds, the
# frames the schema makes for every op (`proto::fuzz::made`) and this machine's working corpus (fuzz/corpus,
# ignored), then minimises that corpus.
# Never a gate stage: the gate lints the targets and replays the seeds in their owners' tests. In the builder, from
# fuzz/.
set -euo pipefail
cd "$(dirname "$0")/../.."
[[ -n "${CHATKORD_IN_BUILDER:-}" ]] || exec scripts/ci/builder.sh run scripts/gate/fuzz.sh "$@"
seconds="${2:-60}"
cd fuzz
# Built for an explicit target with coverage flags: a tree apart from the gate's, so neither rebuilds the other.
export CARGO_TARGET_DIR=/src/build/target/fuzz
if [[ -n "${1:-}" ]]; then targets=("$1"); else mapfile -t targets < <(cargo fuzz list --fuzz-dir .); fi
for target in "${targets[@]}"; do
  # A safe-Rust parser needs no sanitizer, so the stable toolchain builds it; an input over 10 s is a finding.
  mkdir -p "corpus/$target"
  # One frame for every op a peer sends, made from the schema and so never kept as files.
  cargo run --quiet --manifest-path ../protocol/proto/Cargo.toml --features testing --example seeds -- \
    "$target" "corpus/$target"
  cargo fuzz run --fuzz-dir . --sanitizer none "$target" "corpus/$target" "seeds/$target" -- \
    -max_total_time="$seconds" -timeout=10
  # cmin swaps the corpus folder, which a Windows host's mount refuses: merge into one of the builder's, copy back.
  kept="$CARGO_TARGET_DIR/kept/$target"
  rm -rf "$kept" && mkdir -p "$kept"
  cargo fuzz run --fuzz-dir . --sanitizer none "$target" "$kept" "corpus/$target" -- -merge=1
  find "corpus/$target" -type f -delete
  cp "$kept"/* "corpus/$target/"
done
