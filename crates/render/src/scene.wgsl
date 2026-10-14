// The scene (M1c.md, step 3 plan): spheres ray-cast per pixel, and dots
// of a fixed size in pixels (points for bodies under a pixel, markers).
// Every position arrives in view space, already relative to the eye in
// 64 bits on the CPU: x right, y up, looking down -z. Depth is reversed
// with no far plane: depth = near / distance along -z.

struct Frame {
    // 1 / tan(fov_y / 2), the viewport's width / height, the near plane
    // (km), and tan(fov_y / 2).
    focal: f32,
    aspect: f32,
    near: f32,
    tan_half: f32,
    // Pixels.
    viewport: vec2<f32>,
    _pad: vec2<f32>,
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
    _pad0: f32,
    // Unit, toward the Sun.
    light: vec3<f32>,
    _pad1: f32,
    // The quad, view space (unused when full-screen).
    corners: array<vec4<f32>, 4>,
}

struct Dot {
    center: vec3<f32>,
    // Pixels across.
    size: f32,
    color: vec3<f32>,
    // 1 for a round dot, 0 square.
    round: f32,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> spheres: array<Sphere>;
@group(0) @binding(2) var<storage, read> dots: array<Dot>;

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

@fragment
fn sphere_fs(in: SphereOut) -> SphereFragment {
    let s = spheres[in.index];
    let d = normalize(in.ray);
    let b = dot(d, s.center);
    // |c|² - r², from the altitude: exact where it is small.
    let q = s.altitude * (2.0 * s.radius + s.altitude);
    let disc = b * b - q;
    if (disc < 0.0 || b <= 0.0) {
        discard;
    }
    // The near hit, without b - √disc's cancellation.
    let t = q / (b + sqrt(disc));
    let p = d * t;
    var out: SphereFragment;
    if (s.emissive > 0.5) {
        out.color = vec4<f32>(s.color, 1.0);
    } else {
        let n = normalize(p - s.center);
        out.color = vec4<f32>(s.color * s.irradiance * max(dot(n, s.light), 0.0), 1.0);
    }
    out.depth = frame.near / max(-p.z, frame.near);
    return out;
}

struct DotOut {
    @builtin(position) position: vec4<f32>,
    // -1..1 across the dot.
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) color: vec3<f32>,
    @location(2) @interpolate(flat) round: f32,
}

@vertex
fn dot_vs(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> DotOut {
    let p = dots[index];
    let k = corner_index(vertex);
    let uv = vec2<f32>(f32(k & 1u) * 2.0 - 1.0, f32(k >> 1u) * 2.0 - 1.0);
    var clip = clip_of(p.center);
    // A fixed size in pixels: an offset in NDC, scaled by w.
    clip = vec4<f32>(clip.xy + uv * p.size / frame.viewport * clip.w, clip.zw);
    var out: DotOut;
    out.position = clip;
    out.uv = uv;
    out.color = p.color;
    out.round = p.round;
    return out;
}

@fragment
fn dot_fs(in: DotOut) -> @location(0) vec4<f32> {
    if (in.round > 0.5 && dot(in.uv, in.uv) > 1.0) {
        discard;
    }
    return vec4<f32>(in.color, 1.0);
}
