# How the recipes take a pinned source, build it where it is and keep its licence (scripts/deps/libs.sh,
# scripts/deps/ffmpeg.sh, scripts/deps/ffmpeg-windows.sh, scripts/deps/mesa.sh, scripts/deps/glslang.sh); sourced after
# SRC and PREFIX are set.

# A recipe sends each step's output to /dev/null, so a step that fails says which it was.
set -E
trap 'echo "FAIL: ${BASH_SOURCE[0]##*/}:$LINENO: $BASH_COMMAND" >&2' ERR

made_here () {  # made_here <build dir>: a build tree is bound to the absolute path it was made at, so one made at
  # another (a moved or cloned build/) is dropped and made again
  [ -f "$1.made-in" ] && [ "$(< "$1.made-in")" = "$1" ] && return 0
  rm -rf "$1" && printf '%s\n' "$1" > "$1.made-in"
}

fetch () {  # fetch <dir> <git url> <tag> <commit> [sparse path]: a folder already there is checked too
  if [ ! -d "$SRC/$1" ] && [ -n "${5:-}" ]; then
    git -c advice.detachedHead=false clone -q --depth 1 --filter=blob:none --sparse --branch "$3" "$2" "$SRC/$1"
    git -C "$SRC/$1" sparse-checkout set "$5"
  fi
  [ -d "$SRC/$1" ] || git -c advice.detachedHead=false clone -q --depth 1 --branch "$3" "$2" "$SRC/$1"
  [ "$(git -C "$SRC/$1" rev-parse HEAD)" = "$4" ] || { echo "FAIL: $1 $3 is not $4"; exit 1; }
}

fetch_branch () {  # fetch_branch <dir> <git url> <branch> <commit>: the commit the branch held when pinned, which a
  # project with no release tags (x264) publishes as its branch; scripts/deps/released.py says when the branch moves on
  [ -d "$SRC/$1" ] || { git init -q "$SRC/$1" && git -C "$SRC/$1" fetch -q --depth 1 "$2" "$4" \
    && git -C "$SRC/$1" -c advice.detachedHead=false checkout -q FETCH_HEAD; }
  [ "$(git -C "$SRC/$1" rev-parse HEAD)" = "$4" ] || { echo "FAIL: $1 is not $4 ($3)"; exit 1; }
}

licence () {  # licence <project> <file or folder in its source>...: kept beside what it built, for the packages
  for file in "${@:2}"; do
    mkdir -p "$PREFIX/share/licenses/$1" && cp -r "$SRC/$1/$file" "$PREFIX/share/licenses/$1/"
  done
}
