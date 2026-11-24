#!/usr/bin/env bash
# Unpacks the pinned `webrtc-sys` and `libwebrtc` crates, applies the series in patches/webrtc (the one exception to
# "no patch of our own"), and fetches the prebuilt engine those crates link,
# each checked against a SHA-256 stated here. The engine's FFmpeg is renamed here, once, on both systems, so no build
# does it.
# client/Cargo.toml points its [patch] at the crates and client/.cargo/config.toml points LK_CUSTOM_WEBRTC at the engine.
#
# The series is one file per concern, N-<concern>.patch, each a commit of a git repository made in patched/: the
# pristine crates are its first commit and every patch one more, committed with a fixed identity and date so the
# output is the same everywhere.
#
# Usage: scripts/deps/webrtc.sh            unpack, patch and fetch what is not current
#        scripts/deps/webrtc.sh diff       rewrite the series from patched/'s commits
# To change a patch: edit patched/, then commit it as a fixup of its concern's commit,
#   git -C build/deps-src/webrtc/patched commit -a --fixup=:/^<concern>:
# and run `diff`, which folds the fixups into their commits and writes the files. A new concern is a new commit
# whose subject is `<concern>: what it does`, its body naming the measurement that forced it.
# Output: build/deps-src/webrtc/patched/{webrtc-sys,libwebrtc} and build/deps-src/webrtc/prebuilt
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${OUT:-$ROOT/build/deps-src/webrtc}
CRATES="webrtc-sys libwebrtc"

# The versions the client pins with "=" (client/Cargo.toml, client/*/Cargo.toml), and the SHA-256 of each published .crate. A bump changes
# both here and the series, in one commit; Cargo's --locked refuses a tree whose version differs from these.
declare -A VERSION=([webrtc-sys]=0.3.54 [libwebrtc]=0.4.1)
declare -A SHA256=(
  [webrtc-sys]=7b35efe157521adc1ccc875e0687d5434c422758f44b1306be961447d7e5660c
  [libwebrtc]=814e93897c8689013dac8e4babc08957fdc04823748903f4be2409d5d3b8d9a2
)
# The engine release webrtc-sys-build names (WEBRTC_TAG) for that webrtc-sys, and the SHA-256 of each zip. LiveKit
# names it by the webrtc-sdk commit it built, whose stamp its libwebrtc.a carries (2026-06-01T04:07:44).
ENGINE_BRANCH=m150_release
ENGINE_SOURCE=a545de45259b47dfac5f3635863e47cb601f1bb2
ENGINE_TAG=webrtc-${ENGINE_SOURCE:0:7}
case "$(uname -s)" in
  MINGW* | MSYS*)
    ENGINE_TRIPLE=win-x64-release
    ENGINE_ARCHIVE=lib/webrtc.lib
    ENGINE_SHA256=862403fcb248c7d8803487e4c22b8821c6130e89100cb9c69be95ef45ff46146 ;;
  *)
    ENGINE_TRIPLE=linux-x64-release
    ENGINE_ARCHIVE=lib/libwebrtc.a
    ENGINE_SHA256=2ece73b9a72a2cacd73d0c62ac3a414e202f37885c3c8f49dddb43c1443ac0bd ;;
esac

# One identity and one date for every commit made here, so the series written from them never changes by itself.
export GIT_AUTHOR_NAME=Chatkord GIT_AUTHOR_EMAIL=noreply@example.invalid GIT_COMMITTER_NAME=Chatkord GIT_COMMITTER_EMAIL=noreply@example.invalid
export GIT_AUTHOR_DATE="2026-01-01T00:00:00+0000" GIT_COMMITTER_DATE="2026-01-01T00:00:00+0000"
# The series in its numbered order, which a glob breaks from the tenth patch on (10- sorts before 2-).
ordered() { printf '%s\n' "$1"/*.patch | sort -V; }
concat() { local f; while IFS= read -r f; do cat "$f"; done < <(ordered "$1"); }
g() { git -c core.autocrlf=false -c core.safecrlf=false -c core.hooksPath=/dev/null -c commit.gpgsign=false -C "$repo" "$@"; }

# Writes the commits of the repository in $repo after its first as N-<concern>.patch into $1.
series() {
  local dir=$1 base commit subject concern n=0
  base=$(g rev-list --max-parents=0 HEAD)
  mkdir -p "$dir"
  for commit in $(g rev-list --reverse "$base..HEAD"); do
    subject=$(g log -1 --format=%s "$commit")
    concern=${subject%%:*}
    [[ "$concern" =~ ^[a-z0-9-]+$ ]] || { echo "FAIL: commit '$subject' is not '<concern>: what it does'; fold a fixup with \`diff\`"; exit 1; }
    n=$((n + 1))
    g format-patch --no-signature --zero-commit --no-stat --no-numbered -1 --stdout "$commit" > "$dir/$n-$concern.patch"
  done
}

if [ "${1:-}" = diff ]; then
  repo=$OUT/patched
  [ -d "$repo/.git" ] || { echo "FAIL: $repo is not unpacked; run scripts/deps/webrtc.sh"; exit 1; }
  [ -z "$(g status --porcelain)" ] || { echo "FAIL: $repo has changes not committed; commit them as a fixup of their concern's commit"; exit 1; }
  if [ -n "$(g log --format=%s | grep '^fixup! ' || true)" ]; then
    GIT_SEQUENCE_EDITOR=true g rebase -q -i --autosquash "$(g rev-list --max-parents=0 HEAD)"
  fi
  rm -rf "$OUT/series" && series "$OUT/series"
  rm -f "$ROOT"/patches/webrtc/*.patch && cp "$OUT"/series/*.patch "$ROOT/patches/webrtc/" && rm -rf "$OUT/series"
  concat "$ROOT/patches/webrtc" | sha256sum > "$OUT/.applied"
  echo "wrote patches/webrtc/*.patch"
  exit 0
fi

# The FFmpeg inside the engine's archive carries FFmpeg's own names, and a program that links the host's as well gets
# whichever the linker meets first, which moved with the crate graph (every hardware path then answered "device
# refused"). An object of the archive is FFmpeg's when it defines a symbol under these prefixes; its other symbols
# are FFmpeg's too, whatever they are called.
RENAME_PREFIXES="av_ avcodec_ avformat_ avio_ avpriv_ avutil_ avfilter_ swr_ sws_ ff_ ffio_ ffurl_"

# An LLVM tool for Windows' COFF archive: on PATH, else in LLVM's own install directory.
llvm_tool() {
  command -v "$1" 2>/dev/null && return
  [ -x "/c/Program Files/LLVM/bin/$1.exe" ] && echo "/c/Program Files/LLVM/bin/$1.exe"
  return 0
}

# Renames FFmpeg's symbols in $1 to a `webrtc_` prefix, in the archive and every reference to them. A subshell, so its
# failure stops the run. Linux's GNU tools read ELF; Windows' LLVM tools read COFF.
rename_ffmpeg() (
  lib=$(basename "$1"); map=$lib.symbols
  cd "$(dirname "$1")"
  if [ "$ENGINE_TRIPLE" = linux-x64-release ]; then
    nm=nm; objcopy=objcopy; strip_file=
  else
    for tool in llvm-nm llvm-objcopy llvm-readobj llvm-ar; do
      [ -n "$(llvm_tool "$tool")" ] || { echo "FAIL: $tool is needed to rename the engine's FFmpeg; install LLVM (winget install LLVM.LLVM)"; exit 1; }
    done
    nm=$(llvm_tool llvm-nm); objcopy=$(llvm_tool llvm-objcopy)
    # NASM's objects carry a `.file` record with an empty name, and llvm-objcopy then writes the object with no symbols
    # at all; the record is debug only.
    strip_file=--strip-symbol=.file
  fi
  "$nm" -A -g --defined-only "$lib" 2>/dev/null | tr -d '\r' | awk -v prefixes="$RENAME_PREFIXES" '
    BEGIN { prefix_count = split(prefixes, prefix, " ") }
    NF >= 3 {
      symbol = $NF; owner = $0
      sub(/ [^ ]+ [^ ]+$/, "", owner); sub(/:[^:]*$/, "", owner); sub(/^.*:/, "", owner)
      count++; members[count] = owner; symbols[count] = symbol
      if (owner ~ /^autorename_/) ffmpeg[owner] = 1
      for (i = 1; i <= prefix_count; i++) if (index(symbol, prefix[i]) == 1) ffmpeg[owner] = 1
    }
    END { for (i = 1; i <= count; i++) if (ffmpeg[members[i]]) print symbols[i] " webrtc_" symbols[i] }
  ' | sort -u > "$map"
  [ -s "$map" ] || { echo "FAIL: the engine no longer carries an FFmpeg to rename; drop the rename from $0"; exit 1; }
  # shellcheck disable=SC2086
  "$objcopy" $strip_file --redefine-syms="$map" "$lib" "$lib.renamed" && mv "$lib.renamed" "$lib"
  [ "$ENGINE_TRIPLE" = linux-x64-release ] || unlist_includes "$lib" "$map"
  rm -f "$map"
)

# clang keeps FFmpeg's `av_used` tables in Windows' archive with `/INCLUDE:<name>` linker directives, which objcopy does
# not rename, so they would ask for names nothing defines any more. Each such directive, in the objects that carry one,
# is blanked to its own length (a section cannot grow): FFmpeg's assembly refers to those tables under their new names,
# which keeps them in the link. Runs in the archive's directory; $2 is the rename's map.
unlist_includes() {
  local lib=$1 map=$2 member size
  local ar objcopy readobj
  ar=$(llvm_tool llvm-ar); objcopy=$(llvm_tool llvm-objcopy); readobj=$(llvm_tool llvm-readobj)
  "$readobj" --coff-directives "$lib" | tr -d '\r' | awk -v map="$map" '
    BEGIN { while ((getline line < map) > 0) { split(line, field, " "); renamed[field[1]] = 1 } }
    /^File: / { member = $0; sub(/^.*\(/, "", member); sub(/\)$/, "", member); next }
    { line = $0; sub(/^[ \t]+/, "", line) }
    line ~ /^Directive\(s\):/ {
      sub(/^Directive\(s\):/, "", line)
      count = split(line, token, " ")
      for (i = 1; i <= count; i++) if (substr(token[i], 1, 9) == "/INCLUDE:" && (substr(token[i], 10) in renamed)) { print member; break }
    }
  ' | sort -u > "$lib.members"
  while IFS= read -r member; do
    rm -rf "$lib.work" && mkdir -p "$lib.work/$(dirname "$member")"
    # `x` writes the member under its file name; `rP` puts it back in its place, matched by its whole path.
    (cd "$lib.work" && "$ar" x "../$lib" "$member")
    [ "$(basename "$member")" = "$member" ] || mv "$lib.work/$(basename "$member")" "$lib.work/$member"
    "$objcopy" --dump-section=.drectve="$lib.drectve" "$lib.work/$member"
    awk -v map="$map" '
      BEGIN { while ((getline line < map) > 0) { split(line, field, " "); renamed[field[1]] = 1 } }
      { count = split($0, token, / /); out = ""
        for (i = 1; i <= count; i++) {
          t = token[i]
          if (substr(t, 1, 9) == "/INCLUDE:" && (substr(t, 10) in renamed)) t = sprintf("%" length(t) "s", "")
          out = out (i > 1 ? " " : "") t
        }
        printf "%s", out }
    ' "$lib.drectve" > "$lib.drectve.new"
    size=$(wc -c < "$lib.drectve")
    [ "$size" = "$(wc -c < "$lib.drectve.new")" ] || { echo "FAIL: the linker directives of $member changed length"; exit 1; }
    "$objcopy" --update-section=.drectve="$lib.drectve.new" "$lib.work/$member"
    (cd "$lib.work" && "$ar" rP "../$lib" "$member")
  done < "$lib.members"
  rm -rf "$lib.work" "$lib.members" "$lib.drectve" "$lib.drectve.new"
}

# What the unpacked engine in $1 must read as: the zip's digest, the rename's, and the renamed archive's.
engine_stamp() {
  echo "$ENGINE_SHA256 $(printf '%s' "$RENAME_PREFIXES" | sha256sum | cut -d' ' -f1) $(sha256sum "$1/$ENGINE_ARCHIVE" 2>/dev/null | cut -d' ' -f1)"
}

# The engine: fetched here rather than by webrtc-sys-build, which checks no digest.
if [ "$(cat "$OUT/prebuilt/.sha256" 2>/dev/null)" != "$(engine_stamp "$OUT/prebuilt")" ]; then
  zip=$OUT/download/$ENGINE_TAG-$ENGINE_TRIPLE.zip
  mkdir -p "$OUT/download"
  [ -f "$zip" ] || curl -sSfL -o "$zip" "https://github.com/livekit/rust-sdks/releases/download/$ENGINE_TAG/webrtc-$ENGINE_TRIPLE.zip"
  [ "$(sha256sum "$zip" | cut -d' ' -f1)" = "$ENGINE_SHA256" ] || { echo "FAIL: $zip is not the published engine $ENGINE_TAG"; exit 1; }
  # shellcheck source=../../protocol/scripts/find-python.sh
  . "$ROOT/protocol/scripts/find-python.sh"
  [ -n "$PYTHON" ] || { echo "FAIL: no Python $PYTHON_FLOOR+ to unpack the engine"; exit 1; }
  # Unpacked beside the engine in use and swapped in whole, so a failed unpack leaves that one in place.
  rm -rf "$OUT/prebuilt.tmp"
  $PYTHON -I -m zipfile -e "$zip" "$OUT/prebuilt.tmp"
  # The zip's root is the triple; dropping it keeps LK_CUSTOM_WEBRTC one path on every target.
  rename_ffmpeg "$OUT/prebuilt.tmp/$ENGINE_TRIPLE/$ENGINE_ARCHIVE"
  engine_stamp "$OUT/prebuilt.tmp/$ENGINE_TRIPLE" > "$OUT/prebuilt.tmp/$ENGINE_TRIPLE/.sha256"
  rm -rf "$OUT/prebuilt" && mv "$OUT/prebuilt.tmp/$ENGINE_TRIPLE" "$OUT/prebuilt" && rm -rf "$OUT/prebuilt.tmp"
fi

recipe=$({ cat "$ROOT/scripts/deps/webrtc.sh"; concat "$ROOT/patches/webrtc"; } | sha256sum | cut -d' ' -f1)
[ "$(cat "$OUT/.recipe" 2>/dev/null)" = "$recipe" ] && { echo "webrtc patched sources are current"; exit 0; }

# Never throw away edits that are not in the series yet: `patched/` is where the series is written from.
repo=$OUT/patched
if [ -d "$repo/.git" ]; then
  # The series `patched/` was built from: a series that changed since (a pull, a rebase) is not an unsaved edit.
  applied=$(cat "$OUT/.applied" 2>/dev/null || true)
  saved=$(concat "$ROOT/patches/webrtc" | sha256sum)
  rm -rf "$OUT/series" && series "$OUT/series"
  mine=$(concat "$OUT/series" | sha256sum)
  rm -rf "$OUT/series"
  if [ -n "$(g status --porcelain)" ] || { [ "$mine" != "$saved" ] && [ "$mine" != "$applied" ]; }; then
    echo "FAIL: $repo has changes the series lacks; commit them as a fixup and run scripts/deps/webrtc.sh diff to keep them, or delete it to drop them"
    exit 1
  fi
fi
# Unpacked and patched beside the tree in use and swapped in whole, as the engine is, so a run cut short leaves it, or
# none, never a half that reads as an unsaved edit.
next=$OUT/next
rm -rf "$next"
mkdir -p "$next/patched" "$OUT/download"
for crate in $CRATES; do
  version=${VERSION[$crate]}; want=${SHA256[$crate]}
  file=$OUT/download/$crate-$version.crate
  [ -f "$file" ] || curl -sSfL -o "$file" "https://static.crates.io/crates/$crate/$crate-$version.crate"
  [ "$(sha256sum "$file" | cut -d' ' -f1)" = "$want" ] || { echo "FAIL: $crate $version is not the published crate"; exit 1; }
  # On stdin: GNU tar reads a Windows drive (`D:`) in an archive's name as a remote host.
  mkdir -p "$next/patched/$crate"; tar -xzf - -C "$next/patched/$crate" --strip-components=1 < "$file"
done
# The pristine crates are the repository's first commit and each patch one more; `am` applies without fuzz.
repo=$next/patched
git -c init.defaultBranch=main init -q "$repo"
g add -A -f && g commit -q -m "base"
while IFS= read -r p; do
  g am -q --no-gpg-sign "$p" || { echo "FAIL: $(basename "$p") does not apply to the pinned crates"; exit 1; }
done < <(ordered "$ROOT/patches/webrtc")
rm -rf "$OUT/pristine" "$OUT/patched" && mv "$next/patched" "$OUT/" && rmdir "$next"
concat "$ROOT/patches/webrtc" | sha256sum > "$OUT/.applied"
echo "$recipe" > "$OUT/.recipe"
echo "webrtc-sys and libwebrtc patched into $OUT/patched, engine $ENGINE_TAG (webrtc-sdk $ENGINE_BRANCH at $ENGINE_SOURCE) in $OUT/prebuilt"
