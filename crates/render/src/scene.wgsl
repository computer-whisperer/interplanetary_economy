// The scene (M1c.md, step 3 plan): spheres ray-cast per pixel, their
// edges antialiased, and dots of a fixed size in pixels (spots of light
// for bodies under a pixel; markers);
// and over it, paths' lines (step 4 plan, item 1). Spheres and dots
// arrive in view space, already relative to the eye in 64 bits on the
// CPU: x right, y up, looking down -z. Paths arrive in their frames'
// coordinates as high and low f32 parts, and are made relative to the eye
// here, before anything else; near the eye, pieces of them arrive placed
// in view space by the CPU. Depth is reversed with no far plane:
// depth = near / distance along -z.

// A pose seen from the eye: the eye in the frame's coordinates (high and
// low parts), and the map from a vector in them to view space.
struct Slot {
    eye_hi: vec3<f32>,
    eye_lo: vec3<f32>,
    to_view: mat3x3<f32>,
}

struct Frame {
    // 1 / tan(fov_y / 2), the viewport's width / height, the near plane
    // (km), and tan(fov_y / 2).
    focal: f32,
    aspect: f32,
    near: f32,
    tan_half: f32,
    // Pixels.
    viewport: vec2<f32>,
    // Lines' width, pixels.
    line_px: f32,
    // Radiance to the shown image's units (`Look::exposure`): this pass
    // writes radiance times it, so that f16 holds the range shown.
    exposure: f32,
    slots: array<Slot, 8>,
}

struct Sphere {
    center: vec3<f32>,
    radius: f32,
    // The eye's height above the surface, |center| - radius, from 64-bit
    // values.
    altitude: f32,
    // 1 for a sphere glowing with `color` itself (the Sun), 0 for one lit.
    emissive: f32,
    // Irradiance from the Sun, in units of 1 au's (lit spheres).
    irradiance: f32,
    // 1 where the quad is the whole screen.
    fullscreen: f32,
    // Albedo (lit) or radiance (emissive).
    color: vec3<f32>,
    // Under 1 while it fades in from its point (scene.rs, `FADE_PX`).
    opacity: f32,
    // Unit, toward the Sun; for a glowing sphere, its limb darkening's
    // coefficients.
    light: vec3<f32>,
    _pad: f32,
    // What a lit sphere's surface reflects is multiplied by, per channel
    // (`Sphere::gain` in scene.rs).
    gain: vec3<f32>,
    _pad2: f32,
    // The quad, view space (unused when full-screen).
    corners: array<vec4<f32>, 4>,
    // The body's x (prime meridian), y (90° east) and z (north) axes, in
    // view space: its maps lie on them (step 6 plan, item 4).
    axes: mat3x3<f32>,
    // Night lights' radiance at full value.
    night: vec3<f32>,
    // FLAG_* below: which maps it has, and how it reflects.
    flags: u32,
    cloud_albedo: f32,
    // Its atmosphere (top 0 for none).
    atmosphere: Atmosphere,
    // What may hide the Sun from it.
    shadows: Shadows,
    // Its rings' inner and outer radii, km (0 for none).
    rings: vec4<f32>,
}

// The Sun and the spheres that may hide part of it from a sphere's
// surface (scene.rs, `ShadowsGpu`): centers in view space, radii (0 for
// none; the Sun's 0 for no eclipses).
struct Shadows {
    sun: vec4<f32>,
    occluders: array<vec4<f32>, 4>,
}

// The share of a disk of angular radius `a` covered by one of radius `b`
// whose center is `t` away (small angles: flat).
fn covered(a: f32, b: f32, t: f32) -> f32 {
    if (t >= a + b) {
        return 0.0;
    }
    if (t <= abs(a - b)) {
        return min(b * b, a * a) / (a * a);
    }
    let x = acos(clamp((t * t + a * a - b * b) / (2.0 * t * a), -1.0, 1.0));
    let y = acos(clamp((t * t + b * b - a * a) / (2.0 * t * b), -1.0, 1.0));
    let k = sqrt(max((-t + a + b) * (t + a - b) * (t - a + b) * (t + a + b), 0.0));
    return (a * a * x + b * b * y - 0.5 * k) / (PI * a * a);
}

// How much of the Sun's disk point `p` (view space) sees past the
// occluders (each taken alone; their shares multiply).
fn sun_seen(sh: Shadows, p: vec3<f32>) -> f32 {
    if (sh.sun.w <= 0.0) {
        return 1.0;
    }
    let to_sun = sh.sun.xyz - p;
    let ds = length(to_sun);
    let a = asin(min(sh.sun.w / ds, 1.0));
    var seen = 1.0;
    for (var k = 0u; k < 4u; k++) {
        let o = sh.occluders[k];
        if (o.w <= 0.0) {
            continue;
        }
        let to_o = o.xyz - p;
        let d = length(to_o);
        if (d >= ds || dot(to_o, to_sun) <= 0.0) {
            continue;
        }
        let b = asin(min(o.w / d, 1.0));
        // The angle between them, well-conditioned when small.
        let t = atan2(length(cross(to_o, to_sun)), dot(to_o, to_sun));
        seen *= 1.0 - covered(a, b, t);
    }
    return seen;
}

// An atmosphere over a sphere (atmosphere.rs; step 6 plan, item 5):
// coefficients per km at the ground, scale heights km.
struct Atmosphere {
    rayleigh: vec3<f32>,
    rayleigh_height: f32,
    mie_scattering: vec3<f32>,
    mie_height: f32,
    mie_absorption: vec3<f32>,
    mie_g: f32,
    // Ozone's absorption at its densest, in a layer falling linearly to
    // zero `absorption_width` either side of `absorption_center`.
    absorption: vec3<f32>,
    absorption_center: f32,
    // The top's radius.
    top: f32,
    absorption_width: f32,
    // Mie scattering's share in its phase function's forward peak, which
    // the scaled medium takes as unscattered (atmosphere.rs), and its
    // mean cosine.
    truncation: f32,
    mie_asymmetry: f32,
}

// `maps::flags` in maps.rs.
const FLAG_RELIEF: u32 = 1u;
const FLAG_CLOUDS: u32 = 2u;
const FLAG_NIGHT: u32 = 4u;
const FLAG_WATER: u32 = 8u;
const FLAG_LUNAR_LAMBERT: u32 = 16u;

struct Dot {
    center: vec3<f32>,
    // Pixels across; a spot's σ.
    size: f32,
    color: vec3<f32>,
    // `Shape` in scene.rs: 0 a spot of light (a Gaussian whose pixels sum
    // to `color`); otherwise a shape with a dark rim, antialiased.
    shape: u32,
}

struct PathVertex {
    hi: vec3<f32>,
    // s to the next sample; 0 where the two are not joined.
    h: f32,
    lo: vec3<f32>,
    path: u32,
    // Frame units per s.
    velocity: vec3<f32>,
}

struct PathInfo {
    pose: u32,
    // The gap's radius where the path is at the scene's moment, line
    // widths; 0 for none.
    gap: f32,
    future: vec4<f32>,
    past: vec4<f32>,
}

// A piece of a segment placed by the CPU, in view space.
struct Piece {
    a: vec3<f32>,
    // The segment's first vertex.
    segment: u32,
    b: vec3<f32>,
    path: u32,
    // The fractions of the segment at the piece's ends.
    fractions: vec2<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> spheres: array<Sphere>;
@group(0) @binding(2) var<storage, read> dots: array<Dot>;
// A sphere's maps (white, flat and empty where it has none):
// equirectangular, east longitude from −180° at the left, north up.
@group(3) @binding(0) var color_map: texture_2d<f32>;
// The surface normal's east and north components.
@group(3) @binding(1) var relief_map: texture_2d<f32>;
@group(3) @binding(2) var cloud_map: texture_2d<f32>;
// sRGB-encoded.
@group(3) @binding(3) var night_map: texture_2d<f32>;
@group(3) @binding(4) var water_map: texture_2d<f32>;
@group(3) @binding(5) var map_sampler: sampler;
// The atmosphere's tables (atmosphere.rs): transmittance to the top (and
// the Mie column, in alpha), Hillaire's multiple scattering, and the
// scaled medium's skylight on level ground.
@group(3) @binding(6) var transmittance_table: texture_2d<f32>;
@group(3) @binding(7) var multiple_table: texture_2d<f32>;
@group(3) @binding(8) var skylight_table: texture_2d<f32>;
@group(3) @binding(9) var table_sampler: sampler;
// The rings' profile across them, inner edge to outer: their particles'
// albedo (rgb) and normal optical depth (a).
@group(3) @binding(10) var ring_map: texture_2d<f32>;
@group(1) @binding(0) var<storage, read> vertices: array<PathVertex>;
@group(1) @binding(1) var<storage, read> paths: array<PathInfo>;
// Per instance, a run of segments: its first vertex, and its segments
// with the pieces each is cut into above them (segments | pieces << 16).
@group(1) @binding(2) var<storage, read> runs: array<vec2<u32>>;
// Per path, where it is at the scene's moment: the first vertex of the
// segment it is on, and the fraction along it (an f32's bits); segments
// before are past, after future. Then where that is on the screen, for a
// path with a gap there (pixels, f32s' bits).
@group(1) @binding(3) var<storage, read> splits: array<vec4<u32>>;
@group(1) @binding(4) var<storage, read> pieces: array<Piece>;

// Two triangles over a quad's corners 0..3 (x, y in {0, 1}).
fn corner_index(vertex: u32) -> u32 {
    let order = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    return order[vertex];
}

fn clip_of(v: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(v.x * frame.focal / frame.aspect, v.y * frame.focal, frame.near, -v.z);
}

struct SphereOut {
    @builtin(position) position: vec4<f32>,
    // Unnormalized, view space.
    @location(0) ray: vec3<f32>,
    @location(1) @interpolate(flat) index: u32,
}

@vertex
fn sphere_vs(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> SphereOut {
    let s = spheres[index];
    let k = corner_index(vertex);
    var out: SphereOut;
    out.index = index;
    if (s.fullscreen > 0.5) {
        let ndc = vec2<f32>(f32(k & 1u) * 2.0 - 1.0, f32(k >> 1u) * 2.0 - 1.0);
        out.position = vec4<f32>(ndc, 0.0, 1.0);
        out.ray = vec3<f32>(ndc.x * frame.tan_half * frame.aspect, ndc.y * frame.tan_half, -1.0);
    } else {
        let c = s.corners[k].xyz;
        out.position = clip_of(c);
        out.ray = c;
    }
    return out;
}

struct SphereFragment {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// f16's largest finite value, less a margin.
const HALF_MAX: f32 = 60000.0;

// `c` within f16, scaled down whole where a channel would pass it: its
// color kept.
fn within_half(c: vec3<f32>) -> vec3<f32> {
    let most = max(c.r, max(c.g, c.b));
    return select(c, c * (HALF_MAX / most), most > HALF_MAX);
}

// What this pass writes for sphere `s`'s radiance `c` (premultiplied):
// exposed, within f16, at its opacity.
fn written(s: Sphere, c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(within_half(c.rgb * frame.exposure), c.a) * s.opacity;
}

// Premultiplied: the sphere over what is behind it, by how much of the
// pixel it covers.
@fragment
fn sphere_fs(in: SphereOut) -> SphereFragment {
    let s = spheres[in.index];
    let d = normalize(in.ray);
    let b = dot(d, s.center);
    // |c|² - r², from the altitude: exact where it is small.
    let q = s.altitude * (2.0 * s.radius + s.altitude);
    let disc = b * b - q;
    // How far the ray passes outside the limb, km (negative inside): the
    // ray's distance from the center is √(r² - disc). In pixels, by its
    // change across them.
    let outside = -disc / (sqrt(max(s.radius * s.radius - disc, 0.0)) + s.radius);
    let per_pixel = length(vec2<f32>(dpdx(outside), dpdy(outside)));
    let coverage = clamp(0.5 - outside / max(per_pixel, 1e-30), 0.0, 1.0);
    // The near hit, without b - √disc's cancellation; just outside, the
    // limb's nearest point.
    var p = d * b;
    if (disc >= 0.0) {
        p = d * (q / (b + sqrt(disc)));
    }
    let up = normalize(p - s.center);
    // Where that is on the maps, and how fast that changes across the
    // screen (before any pixel is discarded), the longitude's change
    // taken across the date line the short way.
    let local = transpose(s.axes) * up;
    let uv = vec2<f32>(0.5 + atan2(local.y, local.x) / (2.0 * PI), 0.5 - asin(clamp(local.z, -1.0, 1.0)) / PI);
    var ddx = dpdx(uv);
    var ddy = dpdy(uv);
    ddx.x -= round(ddx.x);
    ddy.x -= round(ddy.x);
    // Rings in the equator's plane: where the ray crosses it, how far
    // from the center, and that distance's change across the screen (for
    // the profile's level).
    let has_rings = s.rings.y > 0.0;
    let crossing = dot(d, s.axes[2]);
    var ring_t = -1.0;
    var ring_r = 0.0;
    if (has_rings && abs(crossing) > 1e-7) {
        ring_t = dot(s.center, s.axes[2]) / crossing;
        ring_r = length(d * ring_t - s.center);
    }
    let ring_rate = length(vec2<f32>(dpdx(ring_r), dpdy(ring_r)));
    // And where the ray from the ground point toward the Sun crosses
    // them (their shadow), with its change across the screen.
    let shadow_r = ring_shadow_radius(s, p);
    let shadow_rate = length(vec2<f32>(dpdx(shadow_r), dpdy(shadow_r)));
    let in_rings = ring_t > 0.0 && ring_r >= s.rings.x && ring_r <= s.rings.y;
    let ground = coverage > 0.0 && b > 0.0;
    let has_air = s.atmosphere.top > 0.0;
    // Neither the ground nor the air nor rings: nothing here (nor depth,
    // which would hide lines behind).
    let air_top = s.atmosphere.top;
    let in_air = has_air && disc + (air_top - s.radius) * (air_top + s.radius) > 0.0;
    if (!ground && !in_air && !in_rings) {
        discard;
    }
    var surface = vec3<f32>(0.0);
    if (ground) {
        surface = s.color;
        if (s.emissive > 0.5) {
            // The linear law, its disk's mean kept: I₀ = mean / (1 − u/3).
            let mu = max(dot(up, -d), 0.0);
            surface = s.color * (vec3<f32>(1.0) - s.light * (1.0 - mu)) / (vec3<f32>(1.0) - s.light / 3.0);
        } else {
            // Sunlight reaching the ground: past other bodies, and
            // through the air (the scaled medium's direct beam, and its
            // skylight).
            let seen = sun_seen(s.shadows, p);
            var sun = vec3<f32>(seen * ring_shadow(s, shadow_r, shadow_rate));
            var sky = vec3<f32>(0.0);
            if (has_air) {
                let mu_s = dot(up, s.light);
                sun *= scaled(s.atmosphere, through_air(s.atmosphere, s.radius, s.radius, 0.0, mu_s));
                sky = skylight(mu_s) * seen;
            }
            surface = lit(s, up, -d, uv, ddx, ddy, sun, sky);
        }
    }
    var out: SphereFragment;
    out.depth = frame.near / max(-p.z, frame.near);
    // The ground and its air, premultiplied.
    var base = vec4<f32>(surface * coverage, coverage);
    if (has_air) {
        // The air along the ray, to the ground where it meets it: the
        // light it scatters toward the eye, and what it lets through.
        let air = march(s, d, b, disc, ground && disc >= 0.0);
        let through = dot(air.transmittance, vec3<f32>(1.0 / 3.0));
        // The ground, a Lambert surface, seen at cosine μ: by reciprocity
        // through T(μ) + E(μ) / μ, the skylight a beam from there would
        // give it (Chandrasekhar): what the air scatters out of the view
        // it mostly scatters back in from beside it. That share of the
        // whole column that the ray crosses. Against a Monte Carlo, the
        // direct beam alone was 0.70 where the ground was seen by 0.89.
        var seen_by = air.ground;
        if (ground) {
            let mu_v = max(dot(up, -d), 1e-3);
            let column = scaled(s.atmosphere, through_air(s.atmosphere, s.radius, s.radius, 0.0, mu_v));
            let crossed = (vec3<f32>(1.0) - air.ground) / max(vec3<f32>(1.0) - column, vec3<f32>(1e-4));
            seen_by = min(air.ground + skylight(mu_v) / mu_v * min(crossed, vec3<f32>(1.0)), vec3<f32>(1.0));
        }
        base = vec4<f32>(air.light + coverage * seen_by * surface, coverage + (1.0 - coverage) * (1.0 - through));
        if (!ground) {
            out.depth = frame.near / max(air.near, frame.near);
        }
    }
    if (!in_rings) {
        out.color = written(s, base);
        return out;
    }
    // The rings, in front of the ground (or with none behind) or behind
    // it, seen past its edge.
    let ring = ring_light(s, d, ring_t, ring_r, ring_rate);
    let ring_depth = frame.near / max(ring_t * -d.z, frame.near);
    if (!ground || disc < 0.0 || ring_t < q / (b + sqrt(disc))) {
        out.color = vec4<f32>(ring.rgb + ring.a * base.rgb, 1.0 - ring.a * (1.0 - base.a));
        out.depth = ring_depth;
    } else {
        out.color = vec4<f32>(base.rgb + (1.0 - base.a) * ring.rgb, base.a + (1.0 - base.a) * (1.0 - ring.a));
    }
    out.color = written(s, out.color);
    return out;
}

// The rings' profile at `r` km from the center, at `level` (clamped at
// its edges: the maps' sampler wraps), its optical depth at least 0
// (noise in a measured one may take it below, its mean over a pixel not).
fn ring_profile(s: Sphere, r: f32, level: f32) -> vec4<f32> {
    let u = (r - s.rings.x) / (s.rings.y - s.rings.x);
    let k = textureSampleLevel(ring_map, table_sampler, vec2<f32>(u, 0.5), level);
    return vec4<f32>(k.rgb, max(k.a, 0.0));
}

// Where the ray from `p` (view space) toward the Sun crosses the rings'
// plane, km from the center; -1 where it does not.
fn ring_shadow_radius(s: Sphere, p: vec3<f32>) -> f32 {
    let toward = dot(s.light, s.axes[2]);
    if (s.rings.y <= 0.0 || abs(toward) < 1e-6) {
        return -1.0;
    }
    let t = dot(s.center - p, s.axes[2]) / toward;
    if (t <= 0.0) {
        return -1.0;
    }
    return length(p + s.light * t - s.center);
}

// The share of sunlight reaching a point past the rings, its ray toward
// the Sun crossing them `r` km from the center (changing by `rate` a
// pixel: the profile's level, so their fine structure does not alias).
fn ring_shadow(s: Sphere, r: f32, rate: f32) -> f32 {
    if (r < s.rings.x || r > s.rings.y) {
        return 1.0;
    }
    let texels = f32(textureDimensions(ring_map).x);
    let level = max(log2(rate * texels / (s.rings.y - s.rings.x)), 0.0);
    return exp(-ring_profile(s, r, level).a / abs(dot(s.light, s.axes[2])));
}

// The rings where the ray `d` crosses them, `t` along it, `r` from the
// center (changing by `rate` a pixel): the light they send toward the eye
// (rgb) and their transmittance along the ray (a). A slab of normal
// optical depth τ scattering once, its particles Lambert spheres of the
// profile's albedo: from the lit face, μ₀ / (μ₀ + μ) (1 − e^(−τ (1/μ +
// 1/μ₀))); through to the unlit face, μ₀ / (μ − μ₀) (e^(−τ/μ) − e^(−τ/μ₀));
// times ω P(α) / 4, in this file's radiance units. Saturn's shadow and
// other bodies' fall on them.
fn ring_light(s: Sphere, d: vec3<f32>, t: f32, r: f32, rate: f32) -> vec4<f32> {
    let n = s.axes[2];
    let width = s.rings.y - s.rings.x;
    let texels = f32(textureDimensions(ring_map).x);
    let k = ring_profile(s, r, max(log2(rate * texels / width), 0.0));
    let tau = k.a;
    let mu = max(abs(dot(d, n)), 1e-4);
    let mu0 = abs(dot(s.light, n));
    let transmittance = exp(-tau / mu);
    let p = d * t;
    let to_center = s.center - p;
    let along = dot(to_center, s.light);
    var sunlit = sun_seen(s.shadows, p);
    if ((along > 0.0 && dot(to_center, to_center) - along * along < s.radius * s.radius) || mu0 < 1e-4) {
        sunlit = 0.0;
    }
    let alpha = acos(clamp(dot(s.light, -d), -1.0, 1.0));
    let phase = 8.0 / (3.0 * PI) * (sin(alpha) + (PI - alpha) * cos(alpha));
    var share: f32;
    if (dot(s.light, n) * dot(-d, n) > 0.0) {
        share = mu0 / (mu0 + mu) * (1.0 - exp(-tau * (1.0 / mu + 1.0 / mu0)));
    } else if (abs(mu - mu0) < 1e-3) {
        share = tau / mu * exp(-tau / mu);
    } else {
        share = mu0 / (mu - mu0) * (exp(-tau / mu) - exp(-tau / mu0));
    }
    return vec4<f32>(k.rgb * s.irradiance * phase / 4.0 * share * sunlit, transmittance);
}

// What a stretch of atmosphere gives: light scattered toward the eye, the
// transmittance through it (what is behind it is seen by), the scaled
// medium's (the ground is seen by: light the dust scatters forward
// still comes from it), and its near end's distance along -z.
struct Air {
    light: vec3<f32>,
    transmittance: vec3<f32>,
    ground: vec3<f32>,
    near: f32,
}

// Rayleigh's phase function, and Cornette and Shanks's for Mie, by the
// cosine of the scattering angle.
fn rayleigh_phase(c: f32) -> f32 {
    return 3.0 / (16.0 * PI) * (1.0 + c * c);
}

fn henyey_greenstein(g: f32, c: f32) -> f32 {
    return (1.0 - g * g) / (4.0 * PI * pow(1.0 + g * g - 2.0 * g * c, 1.5));
}

fn mie_phase(g: f32, c: f32) -> f32 {
    let g2 = g * g;
    return 3.0 / (8.0 * PI) * (1.0 - g2) * (1.0 + c * c) / ((2.0 + g2) * pow(1.0 + g2 - 2.0 * g * c, 1.5));
}

// Transmittance from radius `r` (over a ground of radius `bottom`; `rr` is
// r² − bottom², exact) along cosine `mu` to the top, and the Mie column
// along the way (alpha): none where the ground is in the way. Bruneton's
// table mapping, as atmosphere.rs.
fn through_air(a: Atmosphere, bottom: f32, r: f32, rr: f32, mu: f32) -> vec4<f32> {
    if (mu < 0.0 && r * r * mu * mu >= rr) {
        return vec4<f32>(0.0);
    }
    let h2 = (a.top - bottom) * (a.top + bottom);
    let h = sqrt(h2);
    let rho = sqrt(max(rr, 0.0));
    let d = max(-r * mu + sqrt(max(h2 - rr + r * r * mu * mu, 0.0)), 0.0);
    let d_min = a.top - r;
    let d_max = rho + h;
    let x_mu = (d - d_min) / max(d_max - d_min, 1e-6);
    let x_r = rho / h;
    let uv = vec2<f32>(0.5 / 256.0 + x_mu * (255.0 / 256.0), 0.5 / 64.0 + x_r * (63.0 / 64.0));
    return textureSampleLevel(transmittance_table, table_sampler, uv, 0.0);
}

// The scaled medium's transmittance, from `through_air`'s: less the
// forward peak's share of Mie scattering along its column.
fn scaled(a: Atmosphere, t: vec4<f32>) -> vec3<f32> {
    return t.rgb * exp(a.truncation * a.mie_scattering * t.a);
}

// The skylight on level ground, the Sun at cosine `mu_s`, for unit
// irradiance (atmosphere.rs, the scaled medium's).
fn skylight(mu_s: f32) -> vec3<f32> {
    let n = f32(textureDimensions(skylight_table).x);
    let u = clamp((mu_s + 1.0) * 0.5, 0.5 / n, 1.0 - 0.5 / n);
    return textureSampleLevel(skylight_table, table_sampler, vec2<f32>(u, 0.5), 0.0).rgb;
}

// Hillaire's multiple scattering at altitude `height`, the Sun at cosine
// `mu_s`: a point scattering σ_s adds σ_s times this (for unit sunlight).
fn multiple_scattering(a: Atmosphere, bottom: f32, height: f32, mu_s: f32) -> vec3<f32> {
    let uv = clamp(vec2<f32>((mu_s + 1.0) * 0.5, height / (a.top - bottom)), vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSampleLevel(multiple_table, table_sampler, uv, 0.0).rgb;
}

// Steps of the march along a ray through an atmosphere.
const AIR_STEPS: u32 = 32u;

// The air along the ray `d` (unit, view space) past sphere `s`: from
// where it enters the atmosphere (or the eye, inside it) to where it
// leaves, or meets the ground if `ground`. `b` is the distance along the
// ray to its closest approach to the center, `disc` b² − (|c|² − r²),
// exact. Positions are taken from the closest approach, s along the ray
// from it, so that altitudes keep their precision far from the eye:
// r² − R² = s² − disc.
fn march(s: Sphere, d: vec3<f32>, b: f32, disc: f32, ground: bool) -> Air {
    let a = s.atmosphere;
    let bottom = s.radius;
    var out: Air;
    out.light = vec3<f32>(0.0);
    out.transmittance = vec3<f32>(1.0);
    out.ground = vec3<f32>(1.0);
    out.near = 0.0;
    // The Mie column crossed, for the scaled medium's transmittance.
    var column = 0.0;
    let disc_top = disc + (a.top - bottom) * (a.top + bottom);
    if (disc_top <= 0.0) {
        return out;
    }
    let half = sqrt(disc_top);
    let s0 = max(-half, -b);
    var s1 = half;
    if (ground) {
        // The ground: b - √disc from the center's side, the near root.
        s1 = min(s1, -sqrt(max(disc, 0.0)));
    }
    if (s1 <= s0) {
        return out;
    }
    out.near = -(d * (b + s0)).z;
    // From the center to the closest approach.
    let across = d * b - s.center;
    let c = dot(d, s.light);
    let phase_r = rayleigh_phase(c);
    let phase_m = mie_phase(a.mie_g, c);
    // Hillaire's multiple scattering, spread as two scatterings spread
    // it: Henyey–Greenstein with the square of the local mean cosine,
    // blended between Mie's (here) and Rayleigh's (isotropic, 0) by Mie's
    // share of the scattering. Isotropic, it was 10× too faint at 40° from
    // the Sun's direction against a Monte Carlo of Mars's dust, 1.2–1.8×
    // too bright at 140°.
    let spread_m = henyey_greenstein(a.mie_asymmetry * a.mie_asymmetry, c) * 4.0 * PI;
    // The Sun's irradiance in this file's radiance units × sr.
    let sun = PI * s.irradiance;
    let dt = (s1 - s0) / f32(AIR_STEPS);
    for (var i = 0u; i < AIR_STEPS; i++) {
        let si = s0 + (f32(i) + 0.5) * dt;
        let rr = si * si - disc;
        let r = sqrt(rr + bottom * bottom);
        let height = rr / (r + bottom);
        let rayleigh = exp(-height / a.rayleigh_height);
        let mie = exp(-height / a.mie_height);
        let ozone = max(1.0 - abs(height - a.absorption_center) / a.absorption_width, 0.0);
        let scattering_r = a.rayleigh * rayleigh;
        let scattering_m = a.mie_scattering * mie;
        let extinction = scattering_r + scattering_m + a.mie_absorption * mie + a.absorption * ozone;
        let up = (d * si + across) / r;
        let mu_s = dot(up, s.light);
        let sunlit = through_air(a, bottom, r, rr, mu_s).rgb * sun_seen(s.shadows, d * (b + si));
        let share = dot(scattering_m, vec3<f32>(1.0)) / max(dot(scattering_r + scattering_m, vec3<f32>(1.0)), 1e-30);
        let multiple = multiple_scattering(a, bottom, height, mu_s) * mix(1.0, spread_m, share);
        let source = sun * ((scattering_r * phase_r + scattering_m * phase_m) * sunlit + (scattering_r + scattering_m) * multiple);
        let step = exp(-extinction * dt);
        // Integrated against the step's own extinction (Hillaire).
        let integral = select(vec3<f32>(dt), (vec3<f32>(1.0) - step) / extinction, extinction > vec3<f32>(1e-12));
        out.light += out.transmittance * source * integral;
        out.transmittance *= step;
        column += mie * dt;
    }
    // The scaled medium's (the ground is seen by): less Mie's forward
    // peak along the column.
    out.ground = out.transmittance * exp(a.truncation * a.mie_scattering * column);
    return out;
}

fn srgb_to_linear(c: f32) -> f32 {
    return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045);
}

// Ocean glint: GGX with Cox and Munk's slopes for a 7 m/s wind (rms
// slope 0.2; α = 0.28), and water's Fresnel reflectance.
const GLINT_ALPHA: f32 = 0.28;

// The radiance of a lit sphere's surface at `up` (unit, from its center),
// seen along `view` (unit, toward the eye), at `uv` on its maps.
// `sun` is the sunlight reaching the ground directly, per channel (through
// its air), and `sky` the skylight on level ground, both over the Sun's
// irradiance above the air.
fn lit(s: Sphere, up: vec3<f32>, view: vec3<f32>, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>, sun: vec3<f32>, sky: vec3<f32>) -> vec3<f32> {
    // East and north there (at a pole, any pair).
    var east = cross(s.axes[2], up);
    if (dot(east, east) < 1e-12) {
        east = s.axes[0];
    }
    east = normalize(east);
    let north = cross(up, east);
    var n = up;
    if ((s.flags & FLAG_RELIEF) != 0u) {
        let t = textureSampleGrad(relief_map, map_sampler, uv, ddx, ddy).rg;
        n = normalize(t.x * east + t.y * north + sqrt(max(1.0 - dot(t, t), 0.0)) * up);
    }
    var albedo = textureSampleGrad(color_map, map_sampler, uv, ddx, ddy).rgb * s.color;
    var clouds = 0.0;
    if ((s.flags & FLAG_CLOUDS) != 0u) {
        clouds = textureSampleGrad(cloud_map, map_sampler, uv, ddx, ddy).r;
        albedo = mix(albedo, vec3<f32>(s.cloud_albedo), clouds);
        n = normalize(mix(n, up, clouds));
    }
    albedo *= s.gain;
    let cos_i = max(dot(n, s.light), 0.0);
    let cos_e = max(dot(n, view), 0.0);
    // Relief tilted toward the Sun past the terminator stays dark, but
    // for peaks catching the light just beyond it.
    let day = smoothstep(-0.05, 0.0, dot(up, s.light));
    var reflected = cos_i;
    if ((s.flags & FLAG_LUNAR_LAMBERT) != 0u) {
        // McEwen 1991: the share L(α) of Lommel–Seeliger by phase angle
        // α (degrees), the rest Lambert.
        let a = degrees(acos(clamp(dot(s.light, view), -1.0, 1.0)));
        let l = clamp(1.0 + a * (-0.019 + a * (2.42e-4 - a * 1.46e-6)), 0.0, 1.0);
        reflected = 2.0 * l * cos_i / max(cos_i + cos_e, 1e-4) + (1.0 - l) * cos_i;
    }
    // The sky's light, on a slope as much of the sky as it faces.
    var color = albedo * s.irradiance * (reflected * day * sun + sky * (0.5 + 0.5 * dot(n, up)));
    if ((s.flags & FLAG_WATER) != 0u) {
        let water = textureSampleGrad(water_map, map_sampler, uv, ddx, ddy).r * (1.0 - clouds);
        let ci = max(dot(up, s.light), 0.0);
        let ce = max(dot(up, view), 1e-4);
        let h = normalize(s.light + view);
        let nh = max(dot(up, h), 0.0);
        let a2 = GLINT_ALPHA * GLINT_ALPHA;
        let k = nh * nh * (a2 - 1.0) + 1.0;
        let distribution = a2 / (PI * k * k);
        let g1 = 2.0 / (1.0 + sqrt(1.0 + a2 * (1.0 / (ci * ci + 1e-6) - 1.0)));
        let g2 = 2.0 / (1.0 + sqrt(1.0 + a2 * (1.0 / (ce * ce) - 1.0)));
        let fresnel = 0.02 + 0.98 * pow(1.0 - max(dot(h, view), 0.0), 5.0);
        // Radiance in this file's units is π × BRDF × irradiance × cos i.
        let brdf = distribution * g1 * g2 * fresnel / (4.0 * ce * max(ci, 1e-4));
        color += water * PI * brdf * ci * s.irradiance * sun;
    }
    if ((s.flags & FLAG_NIGHT) != 0u) {
        let lights = srgb_to_linear(textureSampleGrad(night_map, map_sampler, uv, ddx, ddy).r);
        // The night side, fading in through twilight; dimmed under cloud.
        // (smoothstep's edges in order: reversed, SPIR-V and MSL leave it
        // undefined.)
        let dark = 1.0 - smoothstep(-0.15, 0.05, dot(up, s.light));
        color += s.night * lights * dark * (1.0 - 0.7 * clouds);
    }
    return color;
}

struct DotOut {
    @builtin(position) position: vec4<f32>,
    // Pixels from the dot's center.
    @location(0) offset: vec2<f32>,
    @location(1) @interpolate(flat) color: vec3<f32>,
    @location(2) @interpolate(flat) shape: u32,
    @location(3) @interpolate(flat) size: f32,
}

// Pixels a shape's quad reaches past its size, for its antialiasing.
const DOT_MARGIN: f32 = 1.5;

@vertex
fn dot_vs(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> DotOut {
    let p = dots[index];
    return dot_corner(p.center, p.size, p.color, p.shape, corner_index(vertex));
}

// Corner `k` of a dot's quad at `center` (view space).
fn dot_corner(center: vec3<f32>, size: f32, color: vec3<f32>, shape: u32, k: u32) -> DotOut {
    let uv = vec2<f32>(f32(k & 1u) * 2.0 - 1.0, f32(k >> 1u) * 2.0 - 1.0);
    // A spot reaches 3.5 σ past the pixel its center is in.
    let half = select(size * 0.5 + DOT_MARGIN, 3.5 * size + 0.5, shape == 0u);
    var clip = clip_of(center);
    // A fixed size in pixels: an offset in NDC, scaled by w.
    clip = vec4<f32>(clip.xy + uv * 2.0 * half / frame.viewport * clip.w, clip.zw);
    var out: DotOut;
    out.position = clip;
    // NDC's y is up, pixels' down.
    out.offset = vec2<f32>(uv.x, -uv.y) * half;
    out.color = color;
    out.shape = shape;
    out.size = size;
    return out;
}

// Signed distance to a shape of radius `r` (pixels): negative inside.
fn shape_distance(shape: u32, q: vec2<f32>, r: f32) -> f32 {
    let a = abs(q);
    switch shape {
        case 1u: {
            return length(q) - r;
        }
        case 2u: {
            return abs(length(q) - 0.7 * r) - 0.3 * r;
        }
        case 3u: {
            return (a.x + a.y - r) * 0.70710678;
        }
        case 4u: {
            let arm = min(abs(q.x - q.y), abs(q.x + q.y)) * 0.70710678;
            return max(arm - max(0.75, 0.2 * r), max(a.x, a.y) - 0.8 * r);
        }
        default: {
            return max(a.x, a.y) - 0.8 * r;
        }
    }
}

// The error function (Abramowitz and Stegun 7.1.26: within 1.5e-7).
fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.3275911 * abs(x));
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    return sign(x) * (1.0 - poly * exp(-x * x));
}

// The share of a Gaussian of `sigma` (pixels) falling on the pixel whose
// center is `u` pixels from its center, along one axis.
fn spot_share(u: f32, sigma: f32) -> f32 {
    let k = 1.0 / (sigma * 1.41421356);
    return 0.5 * (erf((u + 0.5) * k) - erf((u - 0.5) * k));
}

// Premultiplied: spots of light are drawn additively, shapes over.
@fragment
fn dot_fs(in: DotOut) -> @location(0) vec4<f32> {
    if (in.shape == 0u) {
        let share = spot_share(in.offset.x, in.size) * spot_share(in.offset.y, in.size);
        return vec4<f32>(in.color * share, 1.0);
    }
    let d = shape_distance(in.shape, in.offset, in.size * 0.5);
    let coverage = clamp(0.5 - d, 0.0, 1.0);
    if (coverage <= 0.0) {
        discard;
    }
    // A pixel-wide dark rim inside the edge.
    let fill = clamp(-0.5 - d, 0.0, 1.0);
    let color = mix(vec3<f32>(0.01), in.color, fill);
    return vec4<f32>(color * coverage, coverage);
}

struct LineOut {
    @builtin(position) position: vec4<f32>,
    // The piece's ends: pixels, their distances along -z, and the
    // fractions of the segment there.
    @location(0) @interpolate(flat) a: vec2<f32>,
    @location(1) @interpolate(flat) b: vec2<f32>,
    @location(2) @interpolate(flat) depths: vec2<f32>,
    @location(3) @interpolate(flat) fractions: vec2<f32>,
    @location(4) @interpolate(flat) segment: u32,
    @location(5) @interpolate(flat) path: u32,
}

// Outside the clip volume: a quad of these draws nothing.
fn hidden() -> LineOut {
    var out: LineOut;
    out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    return out;
}

// The cubic Hermite curve at `u` between `r0` and `r1` (relative to the
// eye) with velocities `v0`, `v1`, `h` s apart.
fn hermite(r0: vec3<f32>, v0: vec3<f32>, r1: vec3<f32>, v1: vec3<f32>, h: f32, u: f32) -> vec3<f32> {
    let u2 = u * u;
    let u3 = u2 * u;
    return (2.0 * u3 - 3.0 * u2 + 1.0) * r0 + (-2.0 * u3 + 3.0 * u2) * r1
        + h * ((u3 - 2.0 * u2 + u) * v0 + (u3 - u2) * v1);
}

// View space to pixels (x right, y down, from the top-left corner).
fn to_pixels(v: vec3<f32>) -> vec2<f32> {
    let ndc = vec2<f32>(v.x * frame.focal / frame.aspect, v.y * frame.focal) / -v.z;
    return vec2<f32>(ndc.x + 1.0, 1.0 - ndc.y) * 0.5 * frame.viewport;
}

// How far past the screen's edges lines are clipped, in screen widths
// and heights from its center: far enough that clipping is never seen,
// near enough that pixel coordinates keep their precision in f32.
const GUARD: f32 = 2.0;

// A piece from `p0` to `q0` (view space), at fractions `fa`, `fb` of its
// segment: clipped at the near plane and at the guard band around the
// screen, and the corner `k` of a capsule around it, the width and a
// pixel of antialiasing past it on every side.
fn capsule(p0: vec3<f32>, q0: vec3<f32>, fa: f32, fb: f32, segment: u32, path: u32, k: u32) -> LineOut {
    // Each plane's side a point must be on: n · (x, y, z, 1) ≥ 0.
    let gx = frame.focal / frame.aspect;
    let gy = frame.focal;
    let planes = array<vec4<f32>, 5>(
        vec4<f32>(0.0, 0.0, -1.0, -frame.near),
        vec4<f32>(-gx, 0.0, -GUARD, 0.0),
        vec4<f32>(gx, 0.0, -GUARD, 0.0),
        vec4<f32>(0.0, -gy, -GUARD, 0.0),
        vec4<f32>(0.0, gy, -GUARD, 0.0),
    );
    var enter = 0.0;
    var leave = 1.0;
    for (var i = 0u; i < 5u; i++) {
        let fp = dot(planes[i], vec4<f32>(p0, 1.0));
        let fq = dot(planes[i], vec4<f32>(q0, 1.0));
        if (fp < 0.0 && fq < 0.0) {
            return hidden();
        }
        if (fp < 0.0) {
            enter = max(enter, fp / (fp - fq));
        } else if (fq < 0.0) {
            leave = min(leave, fp / (fp - fq));
        }
    }
    if (enter > leave) {
        return hidden();
    }
    let p = mix(p0, q0, enter);
    let q = mix(p0, q0, leave);
    let f = vec2<f32>(mix(fa, fb, enter), mix(fa, fb, leave));
    let a = to_pixels(p);
    let b = to_pixels(q);
    let reach = frame.line_px * 0.5 + 1.0;
    let ab = b - a;
    let along = select(vec2<f32>(1.0, 0.0), normalize(ab), dot(ab, ab) > 0.0);
    let across = vec2<f32>(-along.y, along.x);
    let side = f32(k >> 1u) * 2.0 - 1.0;
    let at_b = (k & 1u) == 1u;
    let end = select(a - along * reach, b + along * reach, at_b);
    let corner = end + across * side * reach;
    let w = select(-p.z, -q.z, at_b);
    let ndc = vec2<f32>(corner.x / frame.viewport.x * 2.0 - 1.0, 1.0 - corner.y / frame.viewport.y * 2.0);
    var out: LineOut;
    out.position = vec4<f32>(ndc * w, frame.near, w);
    out.a = a;
    out.b = b;
    out.depths = vec2<f32>(-p.z, -q.z);
    out.fractions = f;
    out.segment = segment;
    out.path = path;
    return out;
}

// Each instance a run of segments, each six vertices a quad over one
// piece of a segment: the segment itself, or one of its pieces along the
// curve.
@vertex
fn line_vs(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> LineOut {
    let run = runs[instance];
    let pieces = run.y >> 16u;
    let piece = (vertex / 6u) % pieces;
    let segment = vertex / (6u * pieces);
    if (segment >= (run.y & 0xffffu)) {
        return hidden();
    }
    let first = run.x + segment;
    let v0 = vertices[first];
    let v1 = vertices[first + 1u];
    if (v0.h <= 0.0) {
        return hidden();
    }
    let slot = frame.slots[paths[v0.path].pose];
    // Relative to the eye first: the high parts' difference is exact
    // where they are close.
    let r0 = (v0.hi - slot.eye_hi) + (v0.lo - slot.eye_lo);
    let r1 = (v1.hi - slot.eye_hi) + (v1.lo - slot.eye_lo);
    let u0 = f32(piece) / f32(pieces);
    let u1 = f32(piece + 1u) / f32(pieces);
    let p = slot.to_view * hermite(r0, v0.velocity, r1, v1.velocity, v0.h, u0);
    let q = slot.to_view * hermite(r0, v0.velocity, r1, v1.velocity, v0.h, u1);
    return capsule(p, q, u0, u1, first, v0.path, corner_index(vertex % 6u));
}

// Each instance a piece the CPU placed.
@vertex
fn piece_vs(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> LineOut {
    let c = pieces[instance];
    return capsule(c.a, c.b, c.fractions.x, c.fractions.y, c.segment, c.path, corner_index(vertex));
}

// Premultiplied coverage of the capsule, in the past or future color by
// where along its segment the fragment is: the fraction across the
// screen made perspective-correct (1/depth is linear on the screen).
@fragment
fn line_fs(in: LineOut) -> @location(0) vec4<f32> {
    let p = in.position.xy;
    let ab = in.b - in.a;
    let length2 = dot(ab, ab);
    var s = 0.0;
    if (length2 > 0.0) {
        s = clamp(dot(p - in.a, ab) / length2, 0.0, 1.0);
    }
    let d = length(p - (in.a + s * ab));
    let split = splits[in.path];
    let info = paths[in.path];
    // Antialiased, and left out within the gap, whose edge is antialiased.
    let gap = length(p - bitcast<vec2<f32>>(split.zw)) - info.gap * frame.line_px;
    let coverage = clamp(frame.line_px * 0.5 + 0.5 - d, 0.0, 1.0) * clamp(gap + 0.5, 0.0, 1.0);
    if (coverage <= 0.0) {
        discard;
    }
    let along = s * in.depths.x / ((1.0 - s) * in.depths.y + s * in.depths.x);
    let u = mix(in.fractions.x, in.fractions.y, along);
    let past = in.segment < split.x || (in.segment == split.x && u < bitcast<f32>(split.y));
    let color = select(info.future.rgb, info.past.rgb, past);
    return vec4<f32>(color * coverage, coverage);
}

// The sky (M1c.md, step 6 plan, item 2), behind everything: the Milky Way
// and the catalog's stars, in the sky's coordinates (equatorial, ICRS).

struct Sky {
    // From the sky's coordinates to view space.
    to_view: mat3x3<f32>,
    // Julian years since J2000, for proper motion.
    years: f32,
    // The sky's exposure: the frame's, lifted (`Look::sky_lift` in
    // scene.rs).
    exposure: f32,
    // What a unit of the Milky Way's texture shows at: its radiance,
    // exposed.
    milky_way: f32,
    // Stars whose brightest pixel would show at less than this are left
    // out.
    faintest: f32,
    // The sky a pixel subtends at the viewer's eye, sr (`Look::pixel`
    // squared); 0: the sky it subtends of the scene.
    viewer: f32,
}

// A catalog star: its direction at J2000, its flux (radiance × sr, the
// largest channel's), its proper motion (radians a year), and its color
// (linear rgb in the low three bytes, the largest 255).
struct Star {
    direction: vec3<f32>,
    flux: f32,
    motion: vec3<f32>,
    color: u32,
}

@group(2) @binding(0) var<uniform> sky: Sky;
@group(2) @binding(1) var<storage, read> stars: array<Star>;
@group(2) @binding(2) var milky_way: texture_2d<f32>;
@group(2) @binding(3) var sky_sampler: sampler;

struct SkyOut {
    @builtin(position) position: vec4<f32>,
    // Unnormalized, view space.
    @location(0) ray: vec3<f32>,
}

// One triangle over the screen, at infinity.
@vertex
fn sky_vs(@builtin(vertex_index) vertex: u32) -> SkyOut {
    let ndc = vec2<f32>(f32((vertex << 1u) & 2u) * 2.0 - 1.0, f32(vertex & 2u) * 2.0 - 1.0);
    var out: SkyOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.ray = vec3<f32>(ndc.x * frame.tan_half * frame.aspect, ndc.y * frame.tan_half, -1.0);
    return out;
}

const PI: f32 = 3.14159265;

// The Milky Way's map is equirectangular: right ascension 0h at its
// center, increasing to the left; north up. Its level is picked from the
// pixel's size on the sky, so the seam at 12h does not show.
@fragment
fn sky_fs(in: SkyOut) -> @location(0) vec4<f32> {
    let v = normalize(in.ray);
    let d = transpose(sky.to_view) * v;
    let ra = atan2(d.y, d.x);
    let dec = asin(clamp(d.z, -1.0, 1.0));
    let uv = vec2<f32>(0.5 - ra / (2.0 * PI), 0.5 - dec / PI);
    let size = vec2<f32>(textureDimensions(milky_way));
    // A pixel's angle here: smaller off the axis, as cos² of the angle.
    let pixel = 2.0 * frame.tan_half / frame.viewport.y * v.z * v.z;
    let level = max(log2(pixel * size.y / PI), 0.0);
    let c = textureSampleLevel(milky_way, sky_sampler, uv, level).rgb * sky.milky_way;
    return vec4<f32>(min(c, vec3<f32>(SKY_MAX)), 1.0);
}

// The most the Milky Way shows at: under f16's largest with two
// saturated stars' brightest pixels (SPOT_PEAK × HALF_MAX each) added
// over it, which past it would be infinite on some GPUs, and NaN under a
// sphere blended over them. Reached where the sky's exposure is ~38
// stops over Earth's.
const SKY_MAX: f32 = 16000.0;

// Each instance a star, a spot carrying its flux.
@vertex
fn star_vs(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> DotOut {
    let s = stars[index];
    let d = sky.to_view * normalize(s.direction + s.motion * sky.years);
    // Its flux over the sky a pixel subtends at the viewer's eye, so that
    // it brings the eye the flux the star would (`Look::point_gain` in
    // scene.rs, for a point); else of the scene there: pixel² cos³θ.
    let pixel = 2.0 * frame.tan_half / frame.viewport.y;
    let cos = -d.z;
    let omega = select(pixel * pixel * cos * cos * cos, sky.viewer, sky.viewer > 0.0);
    let shown = s.flux / max(omega, 1e-30) * sky.exposure;
    // Behind the eye, near its side, or too faint to see.
    if (cos < 0.01 || shown * SPOT_PEAK < sky.faintest) {
        var out: DotOut;
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let rgb = vec3<f32>(f32(s.color & 255u), f32((s.color >> 8u) & 255u), f32((s.color >> 16u) & 255u)) / 255.0;
    return dot_corner(d, SPOT_SIGMA, within_half(rgb * shown), 0u, corner_index(vertex));
}

// A spot's σ, pixels (`SPOT_SIGMA_PX` in scene.rs), and the largest share
// of its flux a pixel can take (centered on it).
const SPOT_SIGMA: f32 = 0.6;
const SPOT_PEAK: f32 = 0.354;
