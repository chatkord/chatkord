# syntax=docker/dockerfile:1
# The builder: `scripts/ci/builder.sh build` from the repository root. Where the
# gate's Linux stages and CI run, with the native dependencies one stage each, so the toolchain and every recipe are
# named once.

# The toolchain rust-toolchain.toml names, checked below.
FROM docker.io/library/rust:1.100.0-trixie@sha256:e19840879045c951c905d0eaa9887f3439c62000e429963371b376bac8d38c31 AS base
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY rust-toolchain.toml ./
# A toolchain file naming another toolchain than the image's fails here, where rustup may not fetch one. Then the
# components the file names (rustfmt and clippy), installed once: a later
# rustup or cargo call would install them implicitly, and one that also adds them can collide with that install.
RUN RUSTUP_AUTO_INSTALL=0 rustc --version && rustup toolchain install --no-self-update
# Where mise keeps the tools mise.lock pins, for the stages that install them.
ENV MISE_DATA_DIR=/opt/mise MISE_CACHE_DIR=/opt/mise/cache MISE_TRUSTED_CONFIG_PATHS=/src MISE_YES=1

# mise at the release mise.toml's floor names, checked by its digest: a stage of its own, copied in late by each stage
# that runs it, so a bump of mise leaves the slow layers before it alone.
FROM base AS mise
ARG MISE_VERSION=2026.10.25
ARG MISE_SHA256=5d362ecc578dc7c146679b841dda949d6472cbf86ff5635db7afdca1b6b3fd87
RUN curl -fsSL -o /usr/local/bin/mise \
        "https://github.com/jdx/mise/releases/download/v$MISE_VERSION/mise-v$MISE_VERSION-linux-x64" \
    && echo "$MISE_SHA256  /usr/local/bin/mise" | sha256sum -c - \
    && chmod 0755 /usr/local/bin/mise

# The native dependencies, one stage per recipe, each COPYing exactly the files its recipe's stamp hashes, so a change to
# one rebuilds it and what is built on it and nothing else. They build as a workstation's gate builds them (gcc), under
# /opt/chatkord, and delete the sources the packages do not need.
FROM base AS tools
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake meson ninja-build nasm pkg-config git \
    && rm -rf /var/lib/apt/lists/*

# libdrm, libva and the Vulkan headers (scripts/deps/libs.sh), which FFmpeg and Mesa build against.
FROM tools AS libs
COPY scripts/deps/libs.sh scripts/deps/source.sh scripts/deps/
COPY scripts/package/linux.env scripts/package/va-system-drivers.txt scripts/package/
RUN PREFIX=/opt/chatkord/deps SRC=/opt/chatkord/deps-src scripts/deps/libs.sh \
    && cd /opt/chatkord/deps-src && rm -rf libdrm libva vulkan-headers

# The FFmpeg the client links and the codecs under it. `.linked` says the client's FFmpeg bindings were never built
# against another.
FROM libs AS ffmpeg
COPY scripts/deps/ffmpeg.sh scripts/deps/ffmpeg-pin.sh scripts/deps/codecs-pin.sh scripts/deps/
COPY patches/ffmpeg patches/ffmpeg
RUN PREFIX=/opt/chatkord/deps SRC=/opt/chatkord/deps-src scripts/deps/ffmpeg.sh \
    && cp /opt/chatkord/deps/.recipe /opt/chatkord/deps/.linked \
    && cd /opt/chatkord/deps-src && rm -rf dav1d ffmpeg

# The video driver the Linux package carries, with LLVM linked in (Debian's static LLVM asks for Polly too).
FROM libs AS mesa
RUN apt-get update \
    && apt-get install -y --no-install-recommends llvm-22-dev libpolly-22-dev bison flex python3-mako python3-yaml \
        libelf-dev libzstd-dev libexpat1-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
COPY scripts/deps/mesa.sh scripts/deps/
RUN PATH=/usr/lib/llvm-22/bin:$PATH PREFIX=/opt/chatkord/deps SRC=/opt/chatkord/deps-src scripts/deps/mesa.sh \
    && rm -rf /opt/chatkord/deps-src/mesa

# The shader compiler.
FROM tools AS glslang
COPY scripts/deps/glslang.sh scripts/deps/source.sh scripts/deps/
RUN PREFIX=/opt/chatkord/deps SRC=/opt/chatkord/deps-src scripts/deps/glslang.sh \
    && cd /opt/chatkord/deps-src && rm -rf glslang

# Microsoft's HLSL compiler for the D3D12 pass, a pinned release.
FROM tools AS dxc
COPY scripts/deps/dxc.sh scripts/deps/source.sh scripts/deps/
RUN PREFIX=/opt/chatkord/deps SRC=/opt/chatkord/deps-src scripts/deps/dxc.sh \
    && rm -f /opt/chatkord/deps-src/linux_dxc_*.tar.gz

# The pinned engine crates with our patches, and the engine with its FFmpeg renamed (scripts/deps/webrtc.sh).
FROM tools AS webrtc
COPY scripts/deps/webrtc.sh scripts/deps/
COPY protocol/scripts/find-python.sh protocol/scripts/
COPY patches/webrtc patches/webrtc
RUN OUT=/opt/chatkord/deps-src/webrtc scripts/deps/webrtc.sh

# Where the gate's Linux stages and CI run: the tools mise.lock pins and the stages'
# results above, which scripts/ci/builder.sh links into the tree. The compilers are trixie's own: the pinned engine
# needs clang 21 or later, and trixie carries 22.
FROM base AS builder
# Without buildx the docker CLI falls back to the legacy builder, which refuses a `RUN --mount`.
# With what makes and checks the Linux AppImage (scripts/package/linux.sh).
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        clang-22 libclang-22-dev lld-22 cmake meson ninja-build nasm pkg-config git \
        libpipewire-0.3-dev libpipewire-0.3-modules libglib2.0-dev docker-cli docker-buildx \
        squashfs-tools desktop-file-utils appstream \
    && rm -rf /var/lib/apt/lists/*
# Every tool mise.lock pins.
COPY --from=mise /usr/local/bin/mise /usr/local/bin/mise
COPY mise.toml mise.lock ./
RUN mise install --quiet && rm -rf /opt/mise/cache
# The stages' results last, each a copy: one recipe's edit rebuilds its stage and the copies from it on, no install.
COPY --from=ffmpeg /opt/chatkord /opt/chatkord
COPY --from=mesa /opt/chatkord /opt/chatkord
COPY --from=glslang /opt/chatkord /opt/chatkord
COPY --from=dxc /opt/chatkord /opt/chatkord
COPY --from=webrtc /opt/chatkord /opt/chatkord
# The pinned engine's C++ needs clang 21 or later; trixie carries 22. bindgen reads the same LLVM.
ENV CC=clang-22 CXX=clang++-22 LIBCLANG_PATH=/usr/lib/llvm-22/lib PATH=/usr/lib/llvm-22/bin:$PATH
COPY scripts/ci/builder-entry.sh /usr/local/bin/builder-entry
ENTRYPOINT ["builder-entry"]
