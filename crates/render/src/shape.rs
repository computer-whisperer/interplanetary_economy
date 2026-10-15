//! Shaped bodies (M1c.md, step 6, part 9 plan, item 1): a body seen whole
//! from its center (star-shaped), as its radius by direction on a cube
//! map's faces, which scene.wgsl's `shape_fs` marches.
//!
//! Each face carries a border past its edges, sampled where its own
//! coordinates put it (on the next faces). A cube texture's seamless
//! filtering would take the next face's texels instead, whose centers
//! drift along a shared edge by up to half a texel: a radius off at first
//! order there, its slope not shrinking with the texels, so normals would
//! crease along every edge. Bordered, each face is bilinear to second
//! order up to its edges, and its levels' borders are its own's means.

use crate::maps::{Shading, f32_to_f16};
use glam::DVec3;

/// A star-shaped body: its radius, km, by direction in its frame
/// (`Sphere::axes`), on a cube map's six faces in wgpu's order (+x, −x,
/// +y, −y, +z, −z) and orientation, each with [`Shape::border`] texels
/// past its edges, row by row from the top. Between texels, bilinear, as
/// the GPU samples it.
#[derive(Clone)]
pub struct Shape {
    /// Texels along a face's side within its edges: a power of two, at
    /// least [`BORDER_SHARE`].
    pub size: u32,
    /// Radii at each face's texel centers, its border's included.
    pub radii: Vec<f32>,
    /// The least and greatest radius, km.
    pub least: f64,
    pub greatest: f64,
    /// The surface's area, km², and the volume within, km³: the map
    /// triangulated between its texels' corners.
    pub area: f64,
    pub volume: f64,
    /// The share of its light its self-shadowing and the parts of it
    /// hiding others leave, averaged over orientations, by phase angle from
    /// 0° every 10° to 180° (`tools/shapes/build.py`); none, all of it.
    pub shadowed: Option<Vec<f64>>,
    /// Its facets for its light as a whole: each one's outward normal
    /// times its area, km², the faces at [`FACET_CELLS`] squares a side.
    facets: Vec<DVec3>,
}

/// Squares along a face's side for [`Shape::lit_area`]'s facets, two
/// triangles each (as `tools/shapes/build.py`'s).
pub const FACET_CELLS: usize = 16;

impl std::fmt::Debug for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shape")
            .field("size", &self.size)
            .field("least", &self.least)
            .field("greatest", &self.greatest)
            .field("area", &self.area)
            .field("volume", &self.volume)
            .field("shadowed", &self.shadowed)
            .finish()
    }
}

/// A face's border is its size over this, each side (scene.wgsl's
/// `SHAPE_BORDER`): through four halvings at least a texel.
pub const BORDER_SHARE: u32 = 16;

/// What a texel's low half float is multiplied by (scene.wgsl's
/// `shape_radius`): the radius between the least and the greatest is the
/// high half plus the low over this.
pub const LOW_SCALE: f32 = 1024.0;

/// The direction through face `face`'s point (u, v), u to the right and v
/// down (wgpu's cube orientation, Vulkan's), ±1 at its edges; not
/// normalized.
pub fn face_direction(face: usize, u: f64, v: f64) -> DVec3 {
    match face {
        0 => DVec3::new(1.0, -v, -u),
        1 => DVec3::new(-1.0, -v, u),
        2 => DVec3::new(u, 1.0, v),
        3 => DVec3::new(u, -1.0, -v),
        4 => DVec3::new(u, -v, 1.0),
        _ => DVec3::new(-u, -v, -1.0),
    }
}

/// The face direction `d` passes through, and where on it: (face, u, v).
pub fn on_face(d: DVec3) -> (usize, f64, f64) {
    let a = d.abs();
    if a.x >= a.y && a.x >= a.z {
        if d.x > 0.0 {
            (0, -d.z / a.x, -d.y / a.x)
        } else {
            (1, d.z / a.x, -d.y / a.x)
        }
    } else if a.y >= a.z {
        if d.y > 0.0 {
            (2, d.x / a.y, d.z / a.y)
        } else {
            (3, d.x / a.y, -d.z / a.y)
        }
    } else if d.z > 0.0 {
        (4, d.x / a.z, -d.y / a.z)
    } else {
        (5, -d.x / a.z, -d.y / a.z)
    }
}

impl Shape {
    /// The shape whose radii, km, are `radii` (as [`Shape::radii`]).
    pub fn new(size: u32, radii: Vec<f32>) -> Shape {
        assert!(size.is_power_of_two() && size >= BORDER_SHARE, "{size}");
        let side = Self::side_of(size);
        assert_eq!(radii.len(), 6 * side * side);
        assert!(radii.iter().all(|&r| r > 0.0), "radii are positive");
        let (least, greatest) = radii.iter().fold((f64::MAX, 0.0f64), |(lo, hi), &r| {
            (lo.min(f64::from(r)), hi.max(f64::from(r)))
        });
        let mut shape = Shape {
            size,
            radii,
            least,
            greatest,
            area: 0.0,
            volume: 0.0,
            shadowed: None,
            facets: Vec::new(),
        };
        (shape.area, shape.volume) = shape.measure();
        shape.facets = shape.facets_of(FACET_CELLS);
        shape
    }

    /// The facets over `cells` squares a face, two triangles each: their
    /// outward normals × areas.
    fn facets_of(&self, cells: usize) -> Vec<DVec3> {
        let at = |k: usize| 2.0 * k as f64 / cells as f64 - 1.0;
        let mut out = Vec::with_capacity(12 * cells * cells);
        for face in 0..6 {
            let point = |i: usize, j: usize| {
                let d = face_direction(face, at(i), at(j)).normalize();
                d * self.radius(d)
            };
            for j in 0..cells {
                for i in 0..cells {
                    let (a, b, c, d) = (
                        point(i, j),
                        point(i + 1, j),
                        point(i + 1, j + 1),
                        point(i, j + 1),
                    );
                    for (p, q, r) in [(a, b, c), (a, c, d)] {
                        let n = (q - p).cross(r - p) / 2.0;
                        // Outward: the faces' orientations wind both ways.
                        out.push(if n.dot(p + q + r) < 0.0 { -n } else { n });
                    }
                }
            }
        }
        out
    }

    /// Its facets' light, lit from `light` and seen from `view` (unit, its
    /// frame), `alpha` apart (radians), each under `shading`'s law as
    /// scene.wgsl's `lit` reflects: Σ area × cos e × the law's reflectance,
    /// km², scaled to its full area (the facets' polyhedron is smaller). A
    /// sphere's is its disk's mean (`disk_mean`) times πr²; any shape's,
    /// averaged over orientations, `area` / 4 times that mean (each facet
    /// counts alone, whatever the law). Its self-shadowing is left to
    /// [`Shape::shadow_share`].
    pub fn lit_area(&self, light: DVec3, view: DVec3, shading: Shading, alpha: f64) -> f64 {
        let l = match shading {
            Shading::Lambert => 0.0,
            Shading::LunarLambert => crate::scene::lunar_lambert_share(alpha),
        };
        let facets: f64 = self.facets.iter().map(|n| n.length()).sum();
        self.area / facets
            * self
                .facets
                .iter()
                .map(|n| {
                    let area = n.length();
                    let (cos_i, cos_e) = (n.dot(light) / area, n.dot(view) / area);
                    if cos_i <= 0.0 || cos_e <= 0.0 {
                        return 0.0;
                    }
                    area * cos_e * (2.0 * l * cos_i / (cos_i + cos_e) + (1.0 - l) * cos_i)
                })
                .sum::<f64>()
    }

    /// The share of its light its self-shadowing leaves at phase angle
    /// `alpha` (radians), averaged over orientations: [`Shape::shadowed`]
    /// between its every 10°.
    pub fn shadow_share(&self, alpha: f64) -> f64 {
        let Some(table) = &self.shadowed else {
            return 1.0;
        };
        let x = (alpha.to_degrees() / 10.0).clamp(0.0, (table.len() - 1) as f64);
        let k = (x.floor() as usize).min(table.len() - 2);
        table[k] + (table[k + 1] - table[k]) * (x - k as f64)
    }

    /// The shape whose radius toward each texel center's direction (unit)
    /// is `radius`, km.
    pub fn from_fn(size: u32, radius: impl Fn(DVec3) -> f64) -> Shape {
        let side = Self::side_of(size);
        let radii = (0..6)
            .flat_map(|face| (0..side * side).map(move |k| (face, k % side, k / side)))
            .map(|(face, i, j)| radius(Self::texel_direction(size, face, i, j)) as f32)
            .collect();
        Shape::new(size, radii)
    }

    /// A face's texels along a side, its border's included.
    fn side_of(size: u32) -> usize {
        (size + 2 * size / BORDER_SHARE) as usize
    }

    fn border(&self) -> usize {
        (self.size / BORDER_SHARE) as usize
    }

    /// The direction (unit) through the center of face `face`'s texel `i`
    /// (from the left), `j` (from the top), on a face `size` texels across
    /// within its edges, counting from its border's first.
    pub fn texel_direction(size: u32, face: usize, i: usize, j: usize) -> DVec3 {
        let border = (size / BORDER_SHARE) as f64;
        let at = |k: usize| 2.0 * (k as f64 - border + 0.5) / f64::from(size) - 1.0;
        face_direction(face, at(i), at(j)).normalize()
    }

    fn texel(&self, face: usize, i: usize, j: usize) -> f64 {
        let side = Self::side_of(self.size);
        f64::from(self.radii[(face * side + j) * side + i])
    }

    /// The radius toward `d` (any length, the body's frame), km: bilinear
    /// on its face.
    pub fn radius(&self, d: DVec3) -> f64 {
        let (face, u, v) = on_face(d);
        let (n, border) = (f64::from(self.size), self.border() as f64);
        let at = |c: f64| (c + 1.0) / 2.0 * n + border - 0.5;
        let (x, y) = (at(u), at(v));
        let (i, j) = (x.floor() as usize, y.floor() as usize);
        let (fx, fy) = (x - i as f64, y - j as f64);
        let top = self.texel(face, i, j) * (1.0 - fx) + self.texel(face, i + 1, j) * fx;
        let bottom = self.texel(face, i, j + 1) * (1.0 - fx) + self.texel(face, i + 1, j + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    /// The radius of the sphere of the same volume, km.
    pub fn equivalent_radius(&self) -> f64 {
        (3.0 * self.volume / (4.0 * std::f64::consts::PI)).cbrt()
    }

    /// The surface points over face `face`'s texel corners within its
    /// edges, (size + 1)² row by row from the top.
    fn corners(&self, face: usize) -> Vec<DVec3> {
        let n = self.size as usize;
        let at = |k: usize| 2.0 * k as f64 / n as f64 - 1.0;
        (0..=n)
            .flat_map(|j| (0..=n).map(move |i| (i, j)))
            .map(|(i, j)| {
                let d = face_direction(face, at(i), at(j)).normalize();
                d * self.radius(d)
            })
            .collect()
    }

    /// Area and volume, the faces' corners joined two triangles a texel.
    fn measure(&self) -> (f64, f64) {
        let n = self.size as usize;
        let (mut area, mut volume) = (0.0, 0.0);
        for face in 0..6 {
            let c = self.corners(face);
            let at = |i: usize, j: usize| c[j * (n + 1) + i];
            for j in 0..n {
                for i in 0..n {
                    let (a, b, cc, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                    for (p, q, r) in [(a, b, cc), (a, cc, d)] {
                        area += (q - p).cross(r - p).length() / 2.0;
                        volume += p.dot(q.cross(r)).abs() / 6.0;
                    }
                }
            }
        }
        (area, volume)
    }

    /// Each texel's bound on the gradient of |p| − r(p̂) at and above the
    /// surface, over the directions within [`Shape::reach`] of it: a step
    /// from there of |p| − r(p̂) over it, going no farther across, never
    /// crosses the surface. The gradient is p̂ − ∇r / |p|, ∇r the radius's
    /// change across directions (per radian): at most √(1 + (|∇r| / r)²).
    /// Over each texel square, the larger difference along each pair of
    /// its sides over the angle between those texels' centers, and its
    /// least radius; each texel takes its four squares' greatest, then the
    /// greatest within a border's width of it (the border holding the next
    /// faces'); with a margin for the GPU's half floats. (One bound for the
    /// whole body was Itokawa's 65, set by the curtain its overhang makes:
    /// every step was tiny, and marches spent their steps far short.)
    pub fn local_slopes(&self) -> Vec<f64> {
        let side = Self::side_of(self.size);
        let reach = self.border();
        let at = |face: usize, i: usize, j: usize| (face * side + j) * side + i;
        let mut squares = vec![0.0f64; 6 * side * side];
        for face in 0..6 {
            for j in 0..side - 1 {
                for i in 0..side - 1 {
                    let d = Self::texel_direction(self.size, face, i, j);
                    let across = d.angle_between(Self::texel_direction(self.size, face, i + 1, j));
                    let down = d.angle_between(Self::texel_direction(self.size, face, i, j + 1));
                    let (a, b) = (self.texel(face, i, j), self.texel(face, i + 1, j));
                    let (c, e) = (self.texel(face, i, j + 1), self.texel(face, i + 1, j + 1));
                    let du = (b - a).abs().max((e - c).abs()) / across;
                    let dv = (c - a).abs().max((e - b).abs()) / down;
                    squares[at(face, i, j)] = du.hypot(dv) / a.min(b).min(c).min(e);
                }
            }
        }
        // Each texel, its squares' greatest; then the greatest within
        // `reach` along rows, then along columns.
        let mut texels = vec![0.0f64; 6 * side * side];
        for face in 0..6 {
            for j in 0..side {
                for i in 0..side {
                    let mut most = 0.0f64;
                    for (di, dj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        if i >= di && j >= dj {
                            most = most.max(squares[at(face, i - di, j - dj)]);
                        }
                    }
                    texels[at(face, i, j)] = most;
                }
            }
        }
        let spread = |values: &[f64], along: bool| {
            let mut out = vec![0.0f64; values.len()];
            for face in 0..6 {
                for j in 0..side {
                    for i in 0..side {
                        let k = if along { i } else { j };
                        let (lo, hi) = (k.saturating_sub(reach), (k + reach).min(side - 1));
                        out[at(face, i, j)] = (lo..=hi)
                            .map(|m| {
                                values[if along {
                                    at(face, m, j)
                                } else {
                                    at(face, i, m)
                                }]
                            })
                            .fold(0.0, f64::max);
                    }
                }
            }
            out
        };
        spread(&spread(&texels, true), false)
            .into_iter()
            .map(|g| (1.0 + g * g).sqrt() * 1.1)
            .collect()
    }

    /// The angle a march's step may cross with [`Shape::local_slopes`]'
    /// bound, radians: a border's width of the smallest texels (a padded
    /// face's corners', 0.28 times its center's).
    pub fn reach(&self) -> f64 {
        self.border() as f64 * 2.0 / f64::from(self.size) * 0.28
    }

    /// The texture's levels for the GPU (`Rgba16Float`, six layers): the
    /// radius between the least and the greatest, as a high half float and
    /// the rest × [`LOW_SCALE`], then [`Shape::local_slopes`]; each level
    /// the mean of the one below (the slopes, the greatest, so that a
    /// coarser level bounds them too), while its border is a texel or more.
    pub fn levels(&self) -> Vec<(u32, Vec<u8>)> {
        let span = self.greatest - self.least;
        let mut radii: Vec<f64> = self
            .radii
            .iter()
            .map(|&r| (f64::from(r) - self.least) / span.max(1e-30))
            .collect();
        let mut slopes = self.local_slopes();
        let mut side = Self::side_of(self.size);
        let mut border = self.border();
        let mut out = Vec::new();
        loop {
            let data = radii
                .iter()
                .zip(&slopes)
                .flat_map(|(&x, &g)| {
                    let high = f32_to_f16(x as f32);
                    let rest = x - f64::from(crate::offscreen::f16_to_f32(high));
                    // Rounded up: a bound.
                    let g = f32_to_f16((g * 1.001) as f32);
                    [high, f32_to_f16((rest * f64::from(LOW_SCALE)) as f32), g, 0]
                })
                .flat_map(u16::to_le_bytes)
                .collect();
            out.push((side as u32, data));
            if border < 2 {
                return out;
            }
            let half = side / 2;
            let children = |values: &[f64], k: usize| {
                let (face, i, j) = (k / (half * half), k % half, k / half % half);
                let at = |x: usize, y: usize| values[(face * side + y) * side + x];
                [
                    at(2 * i, 2 * j),
                    at(2 * i + 1, 2 * j),
                    at(2 * i, 2 * j + 1),
                    at(2 * i + 1, 2 * j + 1),
                ]
            };
            radii = (0..6 * half * half)
                .map(|k| children(&radii, k).iter().sum::<f64>() / 4.0)
                .collect();
            slopes = (0..6 * half * half)
                .map(|k| children(&slopes, k).into_iter().fold(0.0, f64::max))
                .collect();
            side = half;
            border /= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ellipsoid(a: f64, b: f64, c: f64) -> impl Fn(DVec3) -> f64 {
        move |d: DVec3| 1.0 / ((d.x / a).powi(2) + (d.y / b).powi(2) + (d.z / c).powi(2)).sqrt()
    }

    fn random(seed: u64) -> impl FnMut() -> f64 {
        let mut state = seed;
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        }
    }

    /// Each texel's direction within the edges lands on its own face and
    /// texel; the faces' centers are the axes, in wgpu's order.
    #[test]
    fn faces_round_trip() {
        let n = 16;
        let border = (n / BORDER_SHARE) as usize;
        for face in 0..6 {
            for (i, j) in [(0, 0), (3, 5), (15, 15), (15, 0)] {
                let d = Shape::texel_direction(n, face, i + border, j + border);
                let (f, u, v) = on_face(d * 3.0);
                let at = |c: f64| ((c + 1.0) / 2.0 * f64::from(n)).floor() as usize;
                assert_eq!((f, at(u), at(v)), (face, i, j), "{d}");
            }
        }
        let axes = [
            DVec3::X,
            DVec3::NEG_X,
            DVec3::Y,
            DVec3::NEG_Y,
            DVec3::Z,
            DVec3::NEG_Z,
        ];
        for (face, axis) in axes.into_iter().enumerate() {
            assert_eq!(face_direction(face, 0.0, 0.0), axis);
        }
    }

    /// A sphere's area and volume within 5e-4 of 4πr² and 4πr³/3 (the
    /// triangles' chords fall 1.4e-4 inside at 64² a face); an
    /// ellipsoid's volume within 5e-4 of 4πabc/3.
    #[test]
    fn shapes_measure_as_solids() {
        let pi = std::f64::consts::PI;
        let sphere = Shape::from_fn(64, |_| 2.0);
        assert!(
            (sphere.area / (4.0 * pi * 4.0) - 1.0).abs() < 5e-4,
            "{}",
            sphere.area
        );
        assert!((sphere.volume / (4.0 * pi * 8.0 / 3.0) - 1.0).abs() < 5e-4);
        assert!((sphere.equivalent_radius() - 2.0).abs() < 5e-4);
        assert!(sphere.local_slopes().iter().all(|&g| g < 1.2));
        let shape = Shape::from_fn(128, ellipsoid(3.0, 2.0, 1.0));
        let want = 4.0 * pi * 6.0 / 3.0;
        assert!((shape.volume / want - 1.0).abs() < 5e-4, "{}", shape.volume);
        // The texels nearest the axes are half a texel off them.
        assert!((shape.least - 1.0).abs() < 1e-3 && (shape.greatest - 3.0).abs() < 1e-3);
    }

    /// The radius toward any direction is bilinear's to second order up to
    /// the faces' edges and corners: an ellipsoid's within 3e-4 at 128²
    /// a face, and four times nearer at 256². Without the border (the
    /// next face's texels taken across an edge, as a cube texture's
    /// filtering does), 1.6e-3 and 7.8e-4 by the edges.
    #[test]
    fn radii_are_second_order_to_the_edges() {
        let r = ellipsoid(3.0, 2.0, 1.0);
        let worst = |n| {
            let shape = Shape::from_fn(n, &r);
            let mut f = random(7);
            (0..100_000)
                .map(|_| {
                    let d = DVec3::new(f(), f(), f()).normalize();
                    (shape.radius(d) / r(d) - 1.0).abs()
                })
                .fold(0.0, f64::max)
        };
        let (coarse, fine) = (worst(128), worst(256));
        println!("worst {coarse:.2e} at 128², {fine:.2e} at 256²");
        assert!(coarse < 3e-4 && fine < coarse / 3.5, "{coarse} {fine}");
    }

    /// A sphere's facets' light is its disk's (`disk_mean` × πr²) within
    /// 0.5% at phase angles 0° to 120°, Lambert and lunar-Lambert, and
    /// 2% at 150° (the crescent five facets across); an
    /// ellipsoid's at full phase under lunar-Lambert (as bright as the
    /// disk's area seen) is its cross-section across each axis, πbc, πac
    /// and πab, within 1.5% (the facets an inscribed polyhedron's, 0.9%
    /// short across y): its lightcurve.
    #[test]
    fn facets_carry_the_disks_light() {
        use crate::scene::disk_mean;
        let pi = std::f64::consts::PI;
        let sphere = Shape::from_fn(64, |_| 2.0);
        for degrees in [0.0f64, 30.0, 60.0, 90.0, 120.0, 150.0] {
            let alpha = degrees.to_radians();
            let (light, view) = (DVec3::new(alpha.sin(), 0.0, alpha.cos()), DVec3::Z);
            for shading in [Shading::Lambert, Shading::LunarLambert] {
                let got = sphere.lit_area(light, view, shading, alpha);
                let want = disk_mean(shading, alpha) * pi * 4.0;
                let tolerance = if degrees > 120.0 { 0.02 } else { 5e-3 };
                assert!(
                    (got / want - 1.0).abs() < tolerance,
                    "{shading:?} at {degrees}°: {got} vs {want}"
                );
            }
        }
        let (a, b, c) = (3.0, 2.0, 1.0);
        let shape = Shape::from_fn(128, ellipsoid(a, b, c));
        for (axis, want) in [
            (DVec3::X, pi * b * c),
            (DVec3::Y, pi * a * c),
            (DVec3::Z, pi * a * b),
        ] {
            let got = shape.lit_area(axis, axis, Shading::LunarLambert, 0.0);
            assert!((got / want - 1.0).abs() < 0.015, "{axis}: {got} vs {want}");
        }
    }

    /// The levels halve while their border is a texel or more, each the
    /// mean of the one below, and the two halves rebuild a radius to 1e-6
    /// of the span.
    #[test]
    fn levels_keep_the_radius() {
        let shape = Shape::from_fn(64, |d| 1.0 + 0.3 * d.x * d.x + 0.1 * d.z);
        let levels = shape.levels();
        assert_eq!(levels.iter().map(|l| l.0).collect::<Vec<_>>(), [72, 36, 18]);
        let half = |bytes: &[u8], k: usize| {
            let k = k / 2 * 4 + k % 2;
            f64::from(crate::offscreen::f16_to_f32(u16::from_le_bytes([
                bytes[2 * k],
                bytes[2 * k + 1],
            ])))
        };
        let span = shape.greatest - shape.least;
        let rebuilt = |bytes: &[u8], k: usize| {
            shape.least
                + span * (half(bytes, 2 * k) + half(bytes, 2 * k + 1) / f64::from(LOW_SCALE))
        };
        let (_, data) = &levels[0];
        for (k, &r) in shape.radii.iter().enumerate() {
            let got = rebuilt(data, k);
            assert!((got - f64::from(r)).abs() < 1e-6 * span, "{got} vs {r}");
        }
        let (_, next) = &levels[1];
        let want: f64 = [0, 1, 72, 73]
            .iter()
            .map(|&k| f64::from(shape.radii[k]))
            .sum::<f64>()
            / 4.0;
        assert!((rebuilt(next, 0) - want).abs() < 1e-6 * span);
    }
}
