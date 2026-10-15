// From the scene's radiance, exposed (scene.wgsl: 1.0 is the SDR white),
// to what the app shows (M1c.md, step 6 plan, item 1, and part 10): the
// viewer's eye's glare of the light past what the output shows, a
// tonemap to the output's headroom, and on SDR a dither. The glare runs
// down Hushfold's chain (13-tap downsample, tent upsample) and back up
// it, each level weighed by the share of the light the eye scatters as
// far as that level spreads it (glare.rs).

struct Post {
    // The share of the light past the output's that the levels carry.
    glare: f32,
    // The output's peak over the SDR white (1 on SDR).
    headroom: f32,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_clamp: sampler;
@group(0) @binding(2) var glare_level: texture_2d<f32>;
@group(0) @binding(3) var<uniform> post: Post;

struct FullOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One triangle over the screen.
@vertex
fn full_vs(@builtin(vertex_index) vertex: u32) -> FullOut {
    let x = f32((vertex << 1u) & 2u);
    let y = f32(vertex & 2u);
    var out: FullOut;
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

// f16's largest finite value, less a margin: sums of taps stay finite.
const HALF_MAX: f32 = 60000.0;

fn tap(uv: vec2<f32>, texel: vec2<f32>, dx: f32, dy: f32) -> vec3<f32> {
    return min(textureSample(source, linear_clamp, uv + texel * vec2<f32>(dx, dy)).rgb, vec3<f32>(HALF_MAX));
}

// The 13-tap downsample (Jimenez 2014): weights sum to 1.
fn down(c: vec3<f32>, outer: vec3<f32>, cross: vec3<f32>, inner: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(c * 0.125 + outer * 0.03125 + cross * 0.0625 + inner * 0.125, 1.0);
}

@fragment
fn down_fs(in: FullOut) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let uv = in.uv;
    let outer = tap(uv, t, -2.0, -2.0) + tap(uv, t, 2.0, -2.0) + tap(uv, t, -2.0, 2.0) + tap(uv, t, 2.0, 2.0);
    let cross = tap(uv, t, 0.0, -2.0) + tap(uv, t, -2.0, 0.0) + tap(uv, t, 2.0, 0.0) + tap(uv, t, 0.0, 2.0);
    let inner = tap(uv, t, -1.0, -1.0) + tap(uv, t, 1.0, -1.0) + tap(uv, t, -1.0, 1.0) + tap(uv, t, 1.0, 1.0);
    return down(tap(uv, t, 0.0, 0.0), outer, cross, inner);
}

// The first level: the downsample of the light past what the output
// shows (what the tonemap takes off), taken of each texel before the
// taps' 2×2 means (of their mean, a lit texel among dark ones would lose
// what it has past the knee).
@fragment
fn excess_fs(in: FullOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(source));
    // The texel corner between the 2×2 texels under the pixel: the taps
    // reach 3 texels before it and 2 after. At an odd size the pixel's uv
    // lands up to a texel past it (the levels' sizes round down), which
    // later passes read the level by; rounding uv instead jumped 3 texels
    // at the image's center, losing up to half a point's glare there.
    let corner = 2 * vec2<i32>(in.position.xy) + 1;
    var e: array<vec3<f32>, 36>;
    for (var j = 0; j < 6; j++) {
        for (var i = 0; i < 6; i++) {
            let p = clamp(corner + vec2<i32>(i - 3, j - 3), vec2<i32>(0), size - 1);
            let c = min(textureLoad(source, p, 0).rgb, vec3<f32>(HALF_MAX));
            e[j * 6 + i] = c - tonemap(c, post.headroom);
        }
    }
    let outer = quad(&e, -2, -2) + quad(&e, 2, -2) + quad(&e, -2, 2) + quad(&e, 2, 2);
    let cross = quad(&e, 0, -2) + quad(&e, -2, 0) + quad(&e, 2, 0) + quad(&e, 0, 2);
    let inner = quad(&e, -1, -1) + quad(&e, 1, -1) + quad(&e, -1, 1) + quad(&e, 1, 1);
    return down(quad(&e, 0, 0), outer, cross, inner);
}

// The mean of the 2×2 texels a tap `d` texels off the corner covers, of
// `excess_fs`'s 6×6.
fn quad(e: ptr<function, array<vec3<f32>, 36>>, dx: i32, dy: i32) -> vec3<f32> {
    let i = (dy + 2) * 6 + dx + 2;
    return ((*e)[i] + (*e)[i + 1] + (*e)[i + 6] + (*e)[i + 7]) * 0.25;
}

// A 3×3 tent, blended onto the next level up: each level ends holding
// the weighted mean of itself and those below it, within f16 where their
// sum would not be (post.rs weighs the blend).
@fragment
fn up_fs(in: FullOut) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let uv = in.uv;
    var c = tap(uv, t, 0.0, 0.0) * 4.0;
    c += (tap(uv, t, -1.0, 0.0) + tap(uv, t, 1.0, 0.0) + tap(uv, t, 0.0, -1.0) + tap(uv, t, 0.0, 1.0)) * 2.0;
    c += tap(uv, t, -1.0, -1.0) + tap(uv, t, 1.0, -1.0) + tap(uv, t, -1.0, 1.0) + tap(uv, t, 1.0, 1.0);
    return vec4<f32>(c / 16.0, 1.0);
}

// Linear up to the knee, then a shoulder with slope 1 there that
// approaches the headroom: continuous in value and slope. On the largest
// channel, the others scaled with it: a color keeps its hue and
// saturation however bright (a planet's point, its sunlit side), where
// each channel's own shoulder would take it toward white.
fn tonemap(x: vec3<f32>, headroom: f32) -> vec3<f32> {
    let m = max(x.r, max(x.g, x.b));
    let knee = 0.5 * headroom;
    if (m <= knee) {
        return x;
    }
    let room = headroom - knee;
    let shoulder = knee + room * (1.0 - exp(-(m - knee) / room));
    return x * (shoulder / m);
}

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// A hash of the pixel, uniform in [0, 1).
fn noise(p: vec2<f32>) -> f32 {
    var h = (bitcast<u32>(i32(p.x)) * 0x8da6b343u) ^ (bitcast<u32>(i32(p.y)) * 0xd8163841u);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777216.0;
}

@fragment
fn tonemap_fs(in: FullOut) -> @location(0) vec4<f32> {
    let scene = min(textureLoad(source, vec2<i32>(in.position.xy), 0).rgb, vec3<f32>(HALF_MAX));
    let glare = textureSample(glare_level, linear_clamp, in.uv).rgb * post.glare;
    let radiance = scene + glare;
    var c = tonemap(max(radiance, vec3<f32>(0.0)), post.headroom);
    if (post.headroom <= 1.0) {
        // Triangular noise of one 8-bit step, where the output is encoded.
        let n = noise(in.position.xy) + noise(in.position.xy + vec2<f32>(7919.0, 104729.0)) - 1.0;
        c = srgb_decode(clamp(srgb_encode(c) + n / 255.0, vec3<f32>(0.0), vec3<f32>(1.0)));
    }
    return vec4<f32>(c, 1.0);
}

// A texture's next mip level: the 2×2 texels under the pixel, averaged
// (in linear light, for an sRGB texture).
@fragment
fn blit_fs(in: FullOut) -> @location(0) vec4<f32> {
    return textureSample(source, linear_clamp, in.uv);
}
