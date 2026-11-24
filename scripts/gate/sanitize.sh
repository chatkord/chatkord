#!/usr/bin/env bash
# AddressSanitizer over the code that holds C: `mise run sanitize fuzz [SECONDS]` fuzzes
# the software decoder's stream for SECONDS (60 unless given), `mise run sanitize lab` runs the lab tests of `media`,
# `pw` and `picture` instrumented, on a machine that has the hardware they need. Linux only, never a gate stage: the
# instrumented FFmpeg and dav1d are built into build/deps-asan (SANITIZE=address scripts/deps/ffmpeg.sh) the first
# time, and a pinned nightly is installed if missing. Each run writes its output to build/sanitize/ and keeps the
# exit code of what it ran.
set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD

# The nightly every sanitizer run is built with, one dated toolchain named once.
NIGHTLY=nightly-2026-11-17

[[ "$(uname -s)" == Linux ]] || { echo "FAIL: the sanitizer runs are Linux only"; exit 1; }
mode="${1:-}"
case "$mode" in fuzz | lab) ;; *) echo "usage: sanitize.sh fuzz [SECONDS] | lab"; exit 2 ;; esac

# rustfmt too: mediasoup-sys formats the code it generates.
rustup toolchain install "$NIGHTLY" --profile minimal --component rustfmt
# One AddressSanitizer runtime, clang's: a machine without it is told so here, not deep in a build.
printf 'int main(void) { return 0; }\n' | clang -fsanitize=address -x c - -o /dev/null 2>/dev/null || {
  echo "FAIL: clang cannot link AddressSanitizer: install its runtime (Fedora: compiler-rt; Debian, Ubuntu: libclang-rt-dev)"
  exit 1
}
[[ "$(SANITIZE=address scripts/deps/ffmpeg.sh --recipe)" == "$(cat build/deps-asan/.recipe 2>/dev/null)" ]] ||
  SANITIZE=address scripts/deps/ffmpeg.sh
# cargo-fuzz at the version mise.toml pins, for a machine without mise.
fuzz_version=$(sed -n 's/^"cargo:cargo-fuzz" = { version = "\([^"]*\)".*/\1/p' mise.toml)
cargo fuzz --version 2>/dev/null | grep -qF "$fuzz_version" || cargo install --locked cargo-fuzz --version "$fuzz_version"

mkdir -p build/sanitize
log="build/sanitize/$mode.log"
# Only the instrumented FFmpeg and dav1d are seen, as the client's own cargo config sees only build/deps.
export PKG_CONFIG_PATH="$root/build/deps-asan/lib/pkgconfig"
export LK_CUSTOM_WEBRTC="$root/build/deps-src/webrtc/prebuilt"
export ASAN_OPTIONS=detect_leaks=0
# cc gives the build scripts' C Rust's target triple, under which Fedora's clang has no sanitizer runtime,
# so clang's own triple is passed last; cc drops its default warnings once CFLAGS is set, so they are restated.
export CC=clang CXX=clang++
export CFLAGS_x86_64_unknown_linux_gnu="--target=$(clang -print-target-triple) -Wall -Wextra"
export CXXFLAGS_x86_64_unknown_linux_gnu="--target=$(clang -print-target-triple) -Wall -Wextra"
# A tree apart from the gate's, so neither rebuilds the other.
export CARGO_TARGET_DIR="$root/build/target/asan"

status=0
case "$mode" in
  fuzz)
    seconds="${2:-60}"
    mkdir -p fuzz/corpus/decode_bitstream
    cd fuzz
    cargo "+$NIGHTLY" fuzz run --fuzz-dir . --sanitizer address decode_bitstream \
      corpus/decode_bitstream seeds/decode_bitstream -- -max_total_time="$seconds" -timeout=10 \
      >"$root/$log" 2>&1 || status=$?
    ;;
  lab)
    cd client
    # What client/.cargo/config.toml sets for this target, restated: RUSTFLAGS replaces it.
    RUSTFLAGS="-Zsanitizer=address -C link-arg=-fuse-ld=lld" \
      cargo "+$NIGHTLY" nextest run -p media -p pw -p picture --profile lab --target x86_64-unknown-linux-gnu \
      -E 'not test(/::timing::/)' >"$root/$log" 2>&1 || status=$?
    ;;
esac
tail -n 20 "$root/$log"
echo "sanitize $mode: exit $status (the whole output is in $log)"
exit "$status"
