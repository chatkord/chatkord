#!/usr/bin/env bash
# protocol/ is the commit this repository pins, unchanged: every check that runs the protocol's scripts or builds its
# crates reads it, so a missing, moved or edited checkout would gate something other than what is committed. Its own
# file, not the protocol's: it must run when protocol/ is empty.
#
# Usage: scripts/gate/pinned.sh [--self-test]
set -euo pipefail

# git in the submodule, without what a hook exports for this repository (GIT_INDEX_FILE among them), as git's own
# submodule commands clear it.
in_protocol() {
  local tree=$1; shift
  # shellcheck disable=SC2046 # the names, one word each
  (unset $(git rev-parse --local-env-vars); git -C "$tree/protocol" "$@")
}

# The one sentence for the tree at $1 (empty when protocol/ is the pinned commit, unchanged).
fault() {
  local pin head changes
  pin=$(git -C "$1" ls-files --stage -- protocol | awk '$1 == "160000" { print $2 }')
  [[ -n "$pin" ]] || { echo "protocol/ is not a submodule in the index: restore it with git checkout -- protocol"; return; }
  # An empty protocol/ has no .git of its own, and git there would answer for this repository.
  if [[ ! -e "$1/protocol/.git" ]] || ! head=$(in_protocol "$1" rev-parse --verify --quiet HEAD); then
    echo "protocol/ is empty: run git submodule update --init"
  elif [[ "$head" != "$pin" ]]; then
    echo "protocol/ is at ${head:0:12}, not the pinned ${pin:0:12}: run git submodule update, or stage the new pin with git add protocol"
  elif ! changes=$(in_protocol "$1" status --porcelain); then
    echo "protocol/ cannot be read by git: run git submodule update --init --force"
  elif [[ -n "$changes" ]]; then
    echo "protocol/ has changes of its own: commit them in chatkord-protocol, or discard them"
  fi
}

self_test() {
  local scratch wrong=() answer
  # shellcheck disable=SC2046 # the names, one word each
  unset $(git rev-parse --local-env-vars)
  scratch=$(mktemp -d)
  trap 'rm -rf "$scratch"' RETURN
  quiet() { "$@" > /dev/null 2>&1; }
  commit() { quiet git -C "$1" -c user.name=t -c user.email=t@example.invalid commit --allow-empty -m "$2"; }
  quiet git init "$scratch/protocol-src" && commit "$scratch/protocol-src" one
  echo one > "$scratch/protocol-src/file" && quiet git -C "$scratch/protocol-src" add file && commit "$scratch/protocol-src" two
  quiet git init "$scratch/app"
  quiet git -C "$scratch/app" -c protocol.file.allow=always submodule add "$scratch/protocol-src" protocol
  commit "$scratch/app" pinned
  expect() { # NAME WANT: the answer starts with WANT ("" for none)
    answer=$(fault "$scratch/app")
    if [[ -z "$2" ]]; then [[ -z "$answer" ]] || wrong+=("$1"); else [[ "$answer" == "$2"* ]] || wrong+=("$1"); fi
  }
  expect "the pinned commit, unchanged" ""
  export GIT_INDEX_FILE=.git/index; expect "the pinned commit, from a hook" ""; unset GIT_INDEX_FILE
  echo two > "$scratch/app/protocol/file"; expect "a change in protocol/" "protocol/ has changes"
  quiet git -C "$scratch/app/protocol" checkout -- file
  quiet git -C "$scratch/app/protocol" checkout --detach HEAD~1; expect "another commit checked out" "protocol/ is at"
  quiet git -C "$scratch/app" add protocol; expect "a new pin, staged" ""
  quiet git -C "$scratch/app" submodule deinit --force protocol; expect "an empty protocol/" "protocol/ is empty"
  for name in "${wrong[@]}"; do echo "pinned self-test: wrong answer for $name"; done
  (( ${#wrong[@]} )) && return 1
  echo "pinned self-test: every shape is answered"
}

root=$(cd "$(dirname "$0")/../.." && pwd)
case "${1:-}" in
  --self-test) self_test ;;
  "") answer=$(fault "$root"); [[ -z "$answer" ]] || { echo "FAIL: $answer"; exit 1; } ;;
  *) echo "usage: pinned.sh [--self-test]"; exit 2 ;;
esac
