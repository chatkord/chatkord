# The one FFmpeg both systems build (scripts/deps/ffmpeg.sh, scripts/deps/ffmpeg-windows.sh), sourced by each.
# A commit on master, not a release, until FFMPEG_UNTIL replaces it (scripts/deps/released.py watches for it): n9.0.2's D3D12 decode gives a reference picture a new slot
# each time the frame pool hands back a picture it made before, so on a GPU that wants reference-only allocations
# (the RX 9060 XT) decoding stops after about 17 frames. Master fixed it in bcd2c69e08 after release/9.0 was cut.
FFMPEG_URL=https://github.com/FFmpeg/FFmpeg.git
FFMPEG_COMMIT=37515cfd87561e07c5766ea04de5b1ca6f6c0a98
FFMPEG_UNTIL=n9.1
FFMPEG_NAME="FFmpeg master at ${FFMPEG_COMMIT:0:9} (until $FFMPEG_UNTIL)"

# What both systems configure it with: no programs and no device or format code, only the codecs the client uses and the
# software ones where no hardware does the work. Each system adds its own hardware (APIs, encoders, hwaccels, filters)
# and linkage. The command line is built from these arrays and strings, so a flag is stated once.
FFMPEG_FLAGS=(--disable-debug --disable-doc --disable-programs --disable-autodetect --disable-everything
  --disable-avdevice --disable-avformat --disable-swresample --enable-avcodec --enable-avutil --enable-avfilter
  --enable-swscale)
# The software codecs' bindings, GPL as x264 is.
FFMPEG_SOFTWARE_FLAGS=(--enable-libdav1d --enable-libsvtav1 --enable-gpl --enable-libx264)
FFMPEG_SOFTWARE_ENCODERS=libsvtav1,libx264
FFMPEG_DECODERS=h264,av1,libdav1d
FFMPEG_PARSERS=h264,av1
# Each codec's *_metadata filter builds its bitstream reader (cbs_*), which `filter_units` (the encoders' filler,
# client/media/src/encode/filler.rs) needs for that codec; no encoder may be relied on to pull one in.
FFMPEG_BSFS=h264_metadata,extract_extradata,av1_metadata,filter_units
FFMPEG_FILTERS=null,format,scale,hwupload,hwdownload,hwmap

# Our patches to it (patches/ffmpeg), each written for FFmpeg's own list and kept until a release has its change.
FFMPEG_PATCHES=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../patches/ffmpeg" && pwd)

# Puts the pinned commit's tree in <dir>, fetching only that commit, cleaned of the last build and with our patches
# applied in their order, so what is built is always the pin and its patches.
fetch_ffmpeg () {  # fetch_ffmpeg <dir>
  [ -d "$1/.git" ] || git init -q "$1"
  if [ "$(git -C "$1" rev-parse -q --verify HEAD 2>/dev/null)" != "$FFMPEG_COMMIT" ]; then
    git -C "$1" fetch -q --depth 1 "$FFMPEG_URL" "$FFMPEG_COMMIT"
    git -C "$1" -c advice.detachedHead=false checkout -q --force FETCH_HEAD
  fi
  [ "$(git -C "$1" rev-parse HEAD)" = "$FFMPEG_COMMIT" ] || { echo "FAIL: FFmpeg is not $FFMPEG_COMMIT"; exit 1; }
  git -C "$1" reset -q --hard && git -C "$1" clean -qfdx
  for p in "$FFMPEG_PATCHES"/*.patch; do git -C "$1" apply "$p" || { echo "FAIL: $p does not apply"; exit 1; }; done
}
