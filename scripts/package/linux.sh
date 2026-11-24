#!/usr/bin/env bash
# The Linux AppImage: build/package/Chatkord-<version>-x86_64.AppImage. The one
# binary, the window and the command line, is built in release in the builder image, against the libraries it built,
# with the run path that finds them beside it, and held to scripts/size-budget.txt; the AppDir is that folder with the
# desktop's files and every carried licence, checked, then made one squashfs behind AppImage's runtime (pinned below
# by its release's SHA-256). The layout is scripts/package/linux.env's, the name and summary the desktop entry's, the
# version scripts/release/next.py's (a release's at its tag, else the next release's pre-release, as the image's).
#
# Usage: scripts/package/linux.sh                  (needs podman or docker)
#        scripts/package/linux.sh appimage VERSION in the builder: built, staged, checked and made
# CHATKORD_UPDATES, when set, is the update channel the binary is built with;
# a build without it, as any local one, has none.
set -euo pipefail
cd "$(dirname "$0")/../.."
# shellcheck source=linux.env
. scripts/package/linux.env
# shellcheck source=budget.sh
. scripts/package/budget.sh
RUNTIME_RELEASE=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
runtime="build/tools/appimage-runtime-$RUNTIME_RELEASE"
entry=client/shell/assets/chatkord.desktop
name="$(sed -n 's/^Name=//p' "$entry")"
summary="$(sed -n 's/^Comment=//p' "$entry")"

# appstreamcli refuses a component id that is not a reverse domain name and a missing homepage, both kept for now
# (the id is the window's app id; the repository has no public page): each is let through by name, nothing else.
metainfo_checked() {
  local said
  said=$(appstreamcli validate --no-net "$1" || true)
  echo "$said"
  ! grep -E '^[WE]: ' <<< "$said" | grep -vE ': (cid-desktopapp-is-not-rdns|url-homepage-missing)( |$)' | grep -q .
}

appimage() {
  local version=$1 dir=build/package/AppDir home rpath
  # Arrays from --config join the config files', so the linker flag lld needs stays.
  rpath=(--config 'target.x86_64-unknown-linux-gnu.rustflags=["-C", "link-arg=-Wl,-rpath,$ORIGIN"]')
  # CHATKORD_PACKAGE: the carried video driver is the one beside the binary, not the tree's. CHATKORD_VERSION: the
  # binary says the version the AppImage is named by (terms::BUILD).
  (cd client && CHATKORD_PACKAGE=linux CHATKORD_VERSION="$version" \
    cargo build --locked --release -p shell "${rpath[@]}")
  rm -rf "$dir" && home="$dir$CHATKORD_HOME" && mkdir -p "$home/dri" "$dir/usr/bin"
  install -m 755 "$CARGO_TARGET_DIR/release/shell" "$home/chatkord"
  within_budget "$home/chatkord" linux
  for lib in libdrm.so.2 libdrm_amdgpu.so.1; do
    install -m 755 "$(realpath build/deps/lib/$lib)" "$home/$lib"
  done
  # Mesa's VA driver, by the name libva asks for; its symbols are not needed to run it.
  install -m 755 "$(realpath build/deps/lib/dri/radeonsi_drv_video.so)" "$home/dri/radeonsi_drv_video.so"
  strip --strip-unneeded "$home/dri/radeonsi_drv_video.so"
  ln -s "../..$CHATKORD_HOME/chatkord" "$dir/usr/bin/chatkord"

  # The binary names its own driver folder to libva, wherever the image is mounted.
  cat > "$dir/AppRun" <<EOF
#!/bin/sh
exec "\$(dirname "\$(readlink -f "\$0")")$CHATKORD_HOME/chatkord" "\$@"
EOF
  chmod 755 "$dir/AppRun"
  install -D -m 644 "$entry" "$dir/usr/share/applications/chatkord.desktop"
  cp "$entry" "$dir/chatkord.desktop"
  for png in client/shell/assets/icon/chatkord-*.png; do
    side="${png##*-}"; side="${side%.png}"
    install -D -m 644 "$png" "$dir/usr/share/icons/hicolor/${side}x${side}/apps/chatkord.png"
  done
  cp client/shell/assets/icon/chatkord-256.png "$dir/chatkord.png"
  ln -s chatkord.png "$dir/.DirIcon"
  mkdir -p "$dir/usr/share/metainfo"
  sed -e "s|@NAME@|$name|" -e "s|@SUMMARY@|$summary|" -e "s|@VERSION@|$version|" \
    -e "s|@DATE@|$(git log -1 --format=%cs)|" client/shell/assets/chatkord.metainfo.xml.in \
    > "$dir/usr/share/metainfo/chatkord.metainfo.xml"

  # Every carried project's licence and our own, and the notices the About page reads.
  bash scripts/package/notices.sh "$dir/usr/share/licenses/chatkord" "$version" client/shell/Cargo.toml
  # The recipe that made what the AppImage carries, for anyone who rebuilds it, as the GPL of FFmpeg and x264 asks.
  local recipe="$dir/usr/share/doc/chatkord/recipe"
  mkdir -p "$recipe"
  cp scripts/package/linux.sh scripts/package/linux.env scripts/package/va-system-drivers.txt scripts/package/budget.sh \
    scripts/deps/ffmpeg.sh scripts/deps/libs.sh scripts/deps/ffmpeg-pin.sh scripts/deps/codecs-pin.sh scripts/deps/mesa.sh \
    scripts/deps/source.sh scripts/deps/webrtc.sh "$recipe/"
  cp -r patches/webrtc "$recipe/webrtc-patches"
  cp -r patches/ffmpeg "$recipe/ffmpeg-patches"

  desktop-file-validate "$dir/chatkord.desktop"
  metainfo_checked "$dir/usr/share/metainfo/chatkord.metainfo.xml" ||
    { echo "FAIL: the metainfo has a warning not let through"; exit 1; }
  bash scripts/package/glibc-floor.sh "$dir"
  # Every time and owner in the image is the commit's and root's, so the same commit makes the same AppImage.
  local made="build/package/$name-$version-x86_64.AppImage" epoch
  epoch=$(git log -1 --format=%ct)
  mksquashfs "$dir" build/package/AppDir.squashfs -noappend -all-root -comp zstd -Xcompression-level 19 \
    -mkfs-time "$epoch" -all-time "$epoch" -quiet
  cat "$runtime" build/package/AppDir.squashfs > "$made"
  chmod 755 "$made"
  rm build/package/AppDir.squashfs
  echo "built $made"
}

case "${1:-}" in
  appimage) appimage "$2" ;;
  "")
    # shellcheck source=../../protocol/scripts/find-python.sh
    . protocol/scripts/find-python.sh
    [[ -n "${PYTHON:-}" ]] || { echo "FAIL: no Python $PYTHON_FLOOR+"; exit 1; }
    version="$($PYTHON -I scripts/release/next.py --build)"
    if [[ ! -f "$runtime" ]]; then
      mkdir -p build/tools
      curl -sSfL -o "$runtime" \
        "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_RELEASE/runtime-x86_64"
    fi
    [[ "$(sha256sum "$runtime" | cut -d' ' -f1)" == "$RUNTIME_SHA256" ]] ||
      { rm -f "$runtime"; echo "FAIL: $runtime is not AppImage's runtime $RUNTIME_RELEASE"; exit 1; }
    # The builder's build/ is the tree's build/builder: the runtime goes in, the AppImage comes out.
    mkdir -p build/builder/tools build/package
    cp "$runtime" "build/builder/${runtime#build/}"
    scripts/ci/builder.sh run scripts/package/linux.sh appimage "$version"
    mv "build/builder/package/$name-$version-x86_64.AppImage" build/package/
    echo "built build/package/$name-$version-x86_64.AppImage" ;;
  *) echo "usage: linux.sh [appimage VERSION]"; exit 2 ;;
esac
