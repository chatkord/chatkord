# The software codecs both FFmpeg recipes build (scripts/deps/ffmpeg.sh, scripts/deps/ffmpeg-windows.sh), sourced by
# each. A tag is pinned with the commit it must be, so a moved tag fails the build; x264 publishes no release tags, so
# it is pinned by its stable branch's commit (scripts/deps/released.py says when the branch moves on).
DAV1D_TAG=1.5.5;                DAV1D_COMMIT=6c7bee685677fd997a431302be4fea8ea4017606
SVT_AV1_TAG=v4.3.0;             SVT_AV1_COMMIT=e3d63405f9ba8219913a7276c2c47d10e5d022fe
X264_BRANCH=stable;             X264_COMMIT=b35605ace3ddf7c1a5d67a2eb553f034aef41d55

# How each is configured on both systems: static, no programs, the live coding a share uses. A system adds its own
# compiler, runtime and position-independence flags.
DAV1D_FLAGS=(--libdir=lib --buildtype=release --default-library=static -Denable_tools=false -Denable_tests=false
  -Denable_examples=false)
SVT_AV1_FLAGS=(-DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_LIBDIR=lib -DBUILD_SHARED_LIBS=OFF -DBUILD_APPS=OFF
  -DBUILD_TESTING=OFF -DRTC_BUILD=ON -DSVT_AV1_LTO=OFF)
X264_FLAGS=(--enable-static --disable-cli --bit-depth=8 --chroma-format=420 --disable-lavf --disable-swscale
  --disable-ffms --disable-gpac --disable-lsmash --disable-opencl)
