#!/usr/bin/env bash
# Makes room on a GitHub-hosted Ubuntu runner for the builder image and a client build: removes toolchains no job
# here uses. Run only on such a runner.
set -euo pipefail
sudo rm -rf /usr/local/lib/android /usr/share/dotnet /opt/ghc /usr/local/.ghcup /opt/hostedtoolcache/CodeQL
