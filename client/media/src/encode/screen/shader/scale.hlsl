// A screen picture (BGRA, as Windows Graphics Capture draws it) to the two planes of NV12, in one pass, scaled down on
// the way when the planes are smaller: the same maths as rgb2yuv.comp, for a D3D12 compute queue of its own, which a
// game's draws on the 3D engine do not hold up (compute.rs).
//
// The stream is BT.709 in limited range (luma 16..235, chroma 16..240), which is also how the encoder tags it. Chroma
// is the mean of each 2x2 block, sited at its centre. Alpha is ignored: a screen is opaque.

// The group size is `GROUP` in compute.rs, which sizes the dispatch from it.
#define GROUP 8

Texture2D<float4> screen : register(t0);
RWTexture2D<float> luma : register(u0);
RWTexture2D<float2> chroma : register(u1);

// The picture's largest even size: its odd last column or row is never read.
cbuffer Read : register(b0) {
  uint2 extent;
};

static const float3 KR_KG_KB = float3(0.2126, 0.7152, 0.0722);
static const float KB = 0.0722;
static const float KR = 0.2126;

// The colour of the stream's pixel `at` when it covers `scale` of the picture's pixels on each side: the mean of every
// picture pixel under it, each weighted by how much of it is covered. A thin line is kept as the share of the pixel it
// fills, where a filtered read or two between pixels can miss it. Nothing past the even extent is read.
float3 scaled(int2 at, float2 scale) {
  float2 low = float2(at) * scale;
  float2 high = low + scale;
  int2 first = int2(floor(low));
  int2 last = min(int2(ceil(high)) - 1, int2(extent) - 1);
  float3 sum = 0.0;
  float covered = 0.0;
  for (int y = first.y; y <= last.y; y++) {
    float tall = min(high.y, float(y + 1)) - max(low.y, float(y));
    for (int x = first.x; x <= last.x; x++) {
      float weight = tall * (min(high.x, float(x + 1)) - max(low.x, float(x)));
      sum += screen.Load(int3(x, y, 0)).rgb * weight;
      covered += weight;
    }
  }
  return sum / covered;
}

// Each thread's colour, for the chroma of its 2x2 block.
groupshared float3 colours[GROUP][GROUP];

[numthreads(GROUP, GROUP, 1)]
void main(uint3 id : SV_DispatchThreadID, uint3 local : SV_GroupThreadID) {
  // The stream's size: the picture's even size, or smaller when scaled.
  uint width, height;
  luma.GetDimensions(width, height);
  int2 size = int2(width, height);
  int2 at = int2(id.xy);
  // Every thread reaches the barrier: one past the stream reads its last pixel and stores nothing.
  bool inside = at.x < size.x && at.y < size.y;
  int2 read_at = min(at, size - 1);
  float2 scale = float2(extent) / float2(size);
  float3 rgb = scaled(read_at, scale);
  if (inside) {
    luma[at] = 16.0 / 255.0 + (219.0 / 255.0) * dot(KR_KG_KB, rgb);
  }
  colours[local.y][local.x] = rgb;
  GroupMemoryBarrierWithGroupSync();
  if (!inside || (local.x & 1) != 0 || (local.y & 1) != 0) {
    return;
  }
  float3 mean = (colours[local.y][local.x] + colours[local.y][local.x + 1] + colours[local.y + 1][local.x] +
                 colours[local.y + 1][local.x + 1]) * 0.25;
  float y = dot(KR_KG_KB, mean);
  float cb = (mean.b - y) / (2.0 * (1.0 - KB));
  float cr = (mean.r - y) / (2.0 * (1.0 - KR));
  chroma[at / 2] = float2(128.0 / 255.0 + (224.0 / 255.0) * cb, 128.0 / 255.0 + (224.0 / 255.0) * cr);
}
