// The planes of a YUV picture, painted as the colours they stand for by the matrix and range it carries. The values
// are already display-encoded, so the target must not be an sRGB format, which would encode them a second time.

@group(0) @binding(0) var luma: texture_2d<f32>;
@group(0) @binding(1) var blue: texture_2d<f32>;
@group(0) @binding(2) var red: texture_2d<f32>;
@group(0) @binding(3) var linear: sampler;
// The same picture as an NV12 texture's two plane views: luma as R8, blue and red interleaved as Rg8.
@group(0) @binding(4) var luma_plane: texture_2d<f32>;
@group(0) @binding(5) var chroma_plane: texture_2d<f32>;
// An RGB picture (a captured screen, or one's own share's); its alpha is not shown.
@group(0) @binding(6) var rgb_picture: texture_2d<f32>;
// NV12's interleaved blue and red as an Rg8 texture of their own, half the picture's size, beside `luma`: a picture
// in memory uploaded as it lies.
@group(0) @binding(9) var chroma_rows: texture_2d<f32>;

// The share of an imported texture its picture fills from the top left, and the last place a sample may take, half a
// texel inside it: of the texture's own size, and of a plane at half its size (NV12's chroma).
struct Shown {
    extent: vec2<f32>,
    limit: vec2<f32>,
    half_limit: vec2<f32>,
    unused: vec2<f32>,
};

@group(0) @binding(7) var<uniform> shown: Shown;

// How a YUV picture's samples become RGB, made on the CPU from its matrix and range (`conversion.rs`): in `levels`
// the luma sample of black, the factor that takes luma above it to 0..1, the factor that takes a colour difference
// around its middle to -0.5..0.5, and that middle; in `from_red` and `from_blue` what a unit of each colour
// difference adds to red, green and blue.
struct Conversion {
    levels: vec4<f32>,
    from_red: vec4<f32>,
    from_blue: vec4<f32>,
};

@group(0) @binding(8) var<uniform> conversion: Conversion;

// The part of the picture the viewport shows, its origin and size in texture space: (0,0) and (1,1) for all of it;
// and the rounded rectangle it is shown within, in the target's pixels, its corners' radius beside it.
struct Framing {
    origin: vec2<f32>,
    extent: vec2<f32>,
    area: vec4<f32>,
    radius: vec4<f32>,
};

@group(1) @binding(0) var<uniform> framing: Framing;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) at: vec2<f32>,
};

// One triangle that covers the viewport: (0,0), (2,0), (0,2) before the framing.
@vertex
fn vertex(@builtin(vertex_index) i: u32) -> Vertex {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    // Built whole: naga gives a function variable of this type its output's offsets, which SPIR-V forbids.
    return Vertex(vec4<f32>(at.x * 2.0 - 1.0, 1.0 - at.y * 2.0, 0.0, 1.0), framing.origin + at * framing.extent);
}

// How much of the pixel at `at` (the target's pixels) lies within the shape, from 0 to 1: a rounded box's signed
// distance, its edge a pixel wide.
fn covered(at: vec2<f32>) -> f32 {
    let half = framing.area.zw * 0.5;
    let radius = min(framing.radius.x, min(half.x, half.y));
    let q = abs(at - (framing.area.xy + half)) - half + vec2<f32>(radius);
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    return clamp(0.5 - distance, 0.0, 1.0);
}

// `colour` within the shape, premultiplied by how much of the pixel it covers.
fn shaped(colour: vec3<f32>, at: vec2<f32>) -> vec4<f32> {
    let share = covered(at);
    return vec4<f32>(colour * share, share);
}

fn rgb(y_sample: f32, cb_sample: f32, cr_sample: f32) -> vec4<f32> {
    let y = (y_sample - conversion.levels.x) * conversion.levels.y;
    let cb = (cb_sample - conversion.levels.w) * conversion.levels.z;
    let cr = (cr_sample - conversion.levels.w) * conversion.levels.z;
    let rgb = vec3<f32>(y) + cr * conversion.from_red.xyz + cb * conversion.from_blue.xyz;
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// One bilinear sample reaches 2x2 texels, so a pixel whose footprint is wider aliases: up to 4 by 4 samples spread
// over it are averaged. Gradients come from the caller, where control flow is uniform, since a loop may not take them.
const NO_LIMIT = vec2<f32>(3.0e38);
const WHOLE = vec2<f32>(1.0);

fn plane(
    picture: texture_2d<f32>,
    at: vec2<f32>,
    across_x: vec2<f32>,
    across_y: vec2<f32>,
    extent: vec2<f32>,
    limit: vec2<f32>,
) -> vec4<f32> {
    let span = max(abs(across_x), abs(across_y));
    let taps = clamp(ceil(span * extent * vec2<f32>(textureDimensions(picture)) * 0.5), vec2<f32>(1.0), vec2<f32>(4.0));
    var sum = vec4<f32>(0.0);
    for (var row = 0.0; row < taps.y; row += 1.0) {
        for (var column = 0.0; column < taps.x; column += 1.0) {
            let shift = (vec2<f32>(column, row) + vec2<f32>(0.5)) / taps - vec2<f32>(0.5);
            let place = min((at + shift * span) * extent, limit);
            sum += textureSampleGrad(picture, linear, place, across_x * extent, across_y * extent);
        }
    }
    return sum / (taps.x * taps.y);
}

@fragment
fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    let across_x = dpdx(in.at);
    let across_y = dpdy(in.at);
    let colour = rgb(
        plane(luma, in.at, across_x, across_y, WHOLE, NO_LIMIT).r,
        plane(blue, in.at, across_x, across_y, WHOLE, NO_LIMIT).r,
        plane(red, in.at, across_x, across_y, WHOLE, NO_LIMIT).r,
    );
    return shaped(colour.rgb, in.position.xy);
}

@fragment
fn fragment_nv12(in: Vertex) -> @location(0) vec4<f32> {
    let across_x = dpdx(in.at);
    let across_y = dpdy(in.at);
    let chroma = plane(chroma_plane, in.at, across_x, across_y, shown.extent, shown.half_limit).rg;
    let y = plane(luma_plane, in.at, across_x, across_y, shown.extent, shown.limit).r;
    return shaped(rgb(y, chroma.r, chroma.g).rgb, in.position.xy);
}

@fragment
fn fragment_two_planes(in: Vertex) -> @location(0) vec4<f32> {
    let across_x = dpdx(in.at);
    let across_y = dpdy(in.at);
    let chroma = plane(chroma_rows, in.at, across_x, across_y, WHOLE, NO_LIMIT).rg;
    let y = plane(luma, in.at, across_x, across_y, WHOLE, NO_LIMIT).r;
    return shaped(rgb(y, chroma.r, chroma.g).rgb, in.position.xy);
}

@fragment
fn fragment_rgb(in: Vertex) -> @location(0) vec4<f32> {
    let colour = plane(rgb_picture, in.at, dpdx(in.at), dpdy(in.at), shown.extent, shown.limit);
    return shaped(colour.rgb, in.position.xy);
}
