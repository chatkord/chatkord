#!/usr/bin/env bash
# The builder's entry (scripts/ci/builder.Containerfile, scripts/ci/builder.sh run): the tree is mounted at /src and its
# build/ is the builder's own, so the native dependencies the image built are linked where the tree's recipes expect
# them, cargo writes one target tree of its own, and the command runs with the tools mise.lock pins first on PATH.
set -euo pipefail
cd /src
mkdir -p build/home
# A home of its own: the user Docker runs as may have none.
export HOME=/src/build/home
[[ -e build/deps ]] || ln -s /opt/chatkord/deps build/deps
[[ -e build/deps-src ]] || ln -s /opt/chatkord/deps-src build/deps-src
# The tree's owner on the host is not the container's user under Docker.
git config --global --add safe.directory /src
export CARGO_TARGET_DIR=/src/build/target CHATKORD_IN_BUILDER=1
# The client's test binaries load the libdrm the image built (libva is linked in), which no system directory holds
# here; the fuzz workspace links the FFmpeg and engine the image built through the variables below, as the client's
# cargo config has them.
export LD_LIBRARY_PATH=/src/build/deps/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
export PKG_CONFIG_PATH=/src/build/deps/lib/pkgconfig LK_CUSTOM_WEBRTC=/src/build/deps-src/webrtc/prebuilt
eval "$(mise env --shell bash)"
exec "$@"
