# shellcheck shell=bash
# Sourced by the packaging and release scripts, which run from the tree's root: the channel they build, CHATKORD_CHANNEL
# (`stable` when unset), read once from client/channel/channels/<channel>.env, the one owner of every name a package
# carries. The Rust build reads the same variable, so the binary and the package agree; it is exported for it.
channel="${CHATKORD_CHANNEL:-stable}"
channel_env="client/channel/channels/$channel.env"
if [[ ! "$channel" =~ ^[a-z]+$ || ! -f "$channel_env" ]]; then
  echo "FAIL: no channel $channel (CHATKORD_CHANNEL); see client/channel/channels/"
  exit 1
fi
# A checkout may have made the file CRLF on Windows.
# shellcheck disable=SC1090 # the channel's file, by its name
. <(tr -d '\r' < "$channel_env")
export CHATKORD_CHANNEL="$channel"
