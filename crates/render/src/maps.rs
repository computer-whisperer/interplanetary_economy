//! Bodies' surfaces (M1c.md, step 6 plan, item 4): images, and the maps a
//! sphere carries. Maps are equirectangular in the body's frame: east
//! longitude from −180° at the left edge to 180° at the right, north at
//! the top.

use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::sync::Arc;

/// An image for the GPU, row by row from the top, in one of the formats
/// [`Image::FORMATS`] lists.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub data: Vec<u8>,
}

impl Image {
    /// The formats images come in: color (8-bit sRGB RGBA), one channel
    /// (8-bit linear), and two or four half floats.
    pub const FORMATS: [wgpu::TextureFormat; 4] = [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::R8Unorm,
        wgpu::TextureFormat::Rg16Float,
        wgpu::TextureFormat::Rgba16Float,
    ];

    /// An 8-bit sRGB RGBA image.
    pub fn srgba(width: u32, height: u32, data: Vec<u8>) -> Image {
        Image::new(width, height, wgpu::TextureFormat::Rgba8UnormSrgb, data)
    }

    /// One 8-bit linear channel.
    pub fn gray(width: u32, height: u32, data: Vec<u8>) -> Image {
        Image::new(width, height, wgpu::TextureFormat::R8Unorm, data)
    }

    fn new(width: u32, height: u32, format: wgpu::TextureFormat, data: Vec<u8>) -> Image {
        let pixel = format.block_copy_size(None).expect("a color format");
        assert_eq!(
            data.len(),
            (width * height * pixel) as usize,
            "{width} x {height} of {format:?}"
        );
        Image {
            width,
            height,
            format,
            data,
        }
    }

    /// Bytes per pixel.
    pub fn pixel(&self) -> u32 {
        self.format.block_copy_size(None).expect("a color format")
    }
}

/// IEEE single to half precision, rounding to nearest even (normal and
/// subnormal halves; no NaN payloads).
pub fn f32_to_f16(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exponent == 0xff {
        return sign | 0x7c00 | if mantissa != 0 { 0x200 } else { 0 };
    }
    let e = exponent - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = mantissa | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = m >> shift;
        let rest = m & ((1 << shift) - 1);
        let midpoint = 1 << (shift - 1);
        let round = rest > midpoint || (rest == midpoint && half & 1 == 1);
        return sign | (half + u32::from(round)) as u16;
    }
    let half = ((e as u32) << 10) | (mantissa >> 13);
    let rest = mantissa & 0x1fff;
    let round = rest > 0x1000 || (rest == 0x1000 && half & 1 == 1);
    sign | (half + u32::from(round)) as u16
}

/// Relief as a normal map, from heights (km, row by row from the north,
/// equirectangular) on a sphere of `radius` km: each texel the surface's
/// normal's east and north components (two half floats), from central
/// differences.
pub fn relief(width: u32, height: u32, heights: &[f32], radius: f64) -> Image {
    assert_eq!(heights.len(), (width * height) as usize);
    let (w, h) = (width as usize, height as usize);
    let step_lon = std::f64::consts::TAU / w as f64;
    let step_lat = std::f64::consts::PI / h as f64;
    let mut data = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let lat = std::f64::consts::FRAC_PI_2 - (y as f64 + 0.5) * step_lat;
        // East–west distance between columns, km; never under a row's.
        let dx = 2.0 * radius * lat.cos().max(step_lat) * step_lon;
        let dy = 2.0 * radius * step_lat;
        let (up, down) = (y.saturating_sub(1), (y + 1).min(h - 1));
        let dy = dy * (down - up) as f64 / 2.0;
        for x in 0..w {
            let at = |x: usize, y: usize| f64::from(heights[y * w + x]);
            let slope_east = (at((x + 1) % w, y) - at((x + w - 1) % w, y)) / dx;
            let slope_north = (at(x, up) - at(x, down)) / dy.max(1e-9);
            let n = glam::DVec3::new(-slope_east, -slope_north, 1.0).normalize();
            for c in [n.x, n.y] {
                data.extend_from_slice(&f32_to_f16(c as f32).to_le_bytes());
            }
        }
    }
    Image::new(width, height, wgpu::TextureFormat::Rg16Float, data)
}

/// An sRGB equirectangular color map's mean over the sphere, linear,
/// where `clouds` (cover, one channel, at any size) leave it clear; and
/// the clouds' mean cover. Each row is weighed by the cosine of its
/// latitude.
pub fn means(color: &Image, clouds: Option<&Image>) -> ([f32; 3], f32) {
    assert_eq!(color.format, wgpu::TextureFormat::Rgba8UnormSrgb);
    let decode: Vec<f64> = (0..=255)
        .map(|v| {
            let c = f64::from(v) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
        .collect();
    let (w, h) = (color.width as usize, color.height as usize);
    // The cover over color texel (x, y): the mean of the clouds' texels
    // within it, or the one it lies in.
    let cover = |x: usize, y: usize| {
        clouds.map_or(0.0, |c| {
            assert_eq!(c.format, wgpu::TextureFormat::R8Unorm);
            let (cw, ch) = (c.width as usize, c.height as usize);
            let span = |k: usize, n: usize, of: usize| {
                let lo = k * n / of;
                lo..((k + 1) * n / of).max(lo + 1)
            };
            let (xs, ys) = (span(x, cw, w), span(y, ch, h));
            let count = (xs.len() * ys.len()) as f64;
            let sum: u32 = ys
                .flat_map(|cy| xs.clone().map(move |cx| u32::from(c.data[cy * cw + cx])))
                .sum();
            f64::from(sum) / 255.0 / count
        })
    };
    let (mut sum, mut clear, mut weight) = ([0.0f64; 3], 0.0, 0.0);
    for y in 0..h {
        let lat = std::f64::consts::FRAC_PI_2 - (y as f64 + 0.5) / h as f64 * std::f64::consts::PI;
        let row = &color.data[y * w * 4..(y + 1) * w * 4];
        let (mut r, mut open) = ([0.0f64; 3], 0.0);
        for (x, p) in row.chunks_exact(4).enumerate() {
            let k = 1.0 - cover(x, y);
            for c in 0..3 {
                r[c] += decode[p[c] as usize] * k;
            }
            open += k;
        }
        for c in 0..3 {
            sum[c] += r[c] * lat.cos();
        }
        clear += open * lat.cos();
        weight += w as f64 * lat.cos();
    }
    (
        sum.map(|s| (s / clear.max(1e-30)) as f32),
        (1.0 - clear / weight) as f32,
    )
}

/// How a surface reflects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shading {
    /// Lambertian: clouded and dusty bodies.
    Lambert,
    /// McEwen's lunar-Lambert law: airless, rough regolith, its full disk
    /// bright to the limb.
    LunarLambert,
}

/// A body's maps. `color` is multiplied by the sphere's albedo.
#[derive(Clone, Debug)]
pub struct Maps {
    pub color: Image,
    /// `color`'s mean over the sphere where clouds leave it clear, linear,
    /// and the clouds' mean cover (from [`means`]): with `cloud_albedo`,
    /// what a point standing in for the sphere reflects.
    pub mean: [f32; 3],
    pub cover: f32,
    /// From [`relief`].
    pub relief: Option<Image>,
    /// Cloud cover, 0 to 1, one channel.
    pub clouds: Option<Image>,
    /// The clouds' albedo.
    pub cloud_albedo: f32,
    /// Lights on the night side, one channel, sRGB-encoded.
    pub night: Option<Image>,
    /// Their radiance at full value, and color.
    pub night_radiance: [f32; 3],
    /// Water, 1, where the Sun glints; one channel.
    pub water: Option<Image>,
    pub shading: Shading,
    /// Its atmosphere, over the sphere's radius.
    pub air: Option<crate::atmosphere::Air>,
    /// Its rings, in its equator's plane.
    pub rings: Option<Rings>,
}

/// Rings in a body's equatorial plane: from `inner` to `outer` km, their
/// particles' albedo and normal optical depth across them (a row of
/// texels, RGBA half floats: albedo, then depth), lit as a slab of
/// Lambert spheres scattering once.
#[derive(Clone, Debug)]
pub struct Rings {
    pub inner: f64,
    pub outer: f64,
    pub profile: Image,
}

impl Rings {
    /// Rings from `inner` km out in steps of `step` km, each step's
    /// optical depth `tau` (which noise may take below 0: what is sampled
    /// is taken as at least 0, so that a pixel's mean stays unbiased) and
    /// particle albedo `albedo`.
    pub fn new(inner: f64, step: f64, tau: &[f32], albedo: &[[f32; 3]]) -> Rings {
        assert_eq!(tau.len(), albedo.len());
        let data = tau
            .iter()
            .zip(albedo)
            .flat_map(|(t, a)| [a[0], a[1], a[2], *t])
            .flat_map(|x| f32_to_f16(x).to_le_bytes())
            .collect();
        Rings {
            inner,
            outer: inner + step * tau.len() as f64,
            profile: Image::new(tau.len() as u32, 1, wgpu::TextureFormat::Rgba16Float, data),
        }
    }
}

/// Bits of `SphereGpu::flags` (scene.wgsl's `FLAG_*`).
pub(crate) mod flags {
    pub const RELIEF: u32 = 1;
    pub const CLOUDS: u32 = 2;
    pub const NIGHT: u32 = 4;
    pub const WATER: u32 = 8;
    pub const LUNAR_LAMBERT: u32 = 16;
}

impl Maps {
    pub(crate) fn flags(&self) -> u32 {
        let mut f = 0;
        for (on, bit) in [
            (self.relief.is_some(), flags::RELIEF),
            (self.clouds.is_some(), flags::CLOUDS),
            (self.night.is_some(), flags::NIGHT),
            (self.water.is_some(), flags::WATER),
            (self.shading == Shading::LunarLambert, flags::LUNAR_LAMBERT),
        ] {
            if on {
                f |= bit;
            }
        }
        f
    }
}

/// Maps on the GPU: a bind group per set of maps (by its `Arc`, kept while
/// anyone else holds that), and one for spheres without.
pub(crate) struct MapsPass {
    pub(crate) layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    table_sampler: wgpu::Sampler,
    plain: wgpu::BindGroup,
    uploaded: HashMap<usize, (Arc<Maps>, wgpu::BindGroup)>,
}

/// The textures a sphere's bind group holds, in binding order: the maps,
/// then (after their sampler, which wraps east to west) the atmosphere's
/// three tables, and theirs (which clamps); then the rings' profile (read
/// with the tables').
const TEXTURES: usize = 5;
const TABLES: usize = 3;
const RINGS_BINDING: u32 = (TEXTURES + 2 + TABLES) as u32;

impl MapsPass {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> MapsPass {
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let mut entries: Vec<wgpu::BindGroupLayoutEntry> =
            (0..TEXTURES as u32).map(texture).collect();
        entries.push(sampler_entry(TEXTURES as u32));
        entries.extend((0..TABLES as u32).map(|k| texture(TEXTURES as u32 + 1 + k)));
        entries.push(sampler_entry((TEXTURES + 1 + TABLES) as u32));
        entries.push(texture(RINGS_BINDING));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("maps"),
            entries: &entries,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("maps"),
            address_mode_u: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });
        let table_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atmosphere tables, rings"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        // One texel each: white, a flat normal, nothing.
        let one = |format, data: &[u8]| {
            let image = Image::new(1, 1, format, data.to_vec());
            single(device, queue, &image)
        };
        let white = one(wgpu::TextureFormat::Rgba8UnormSrgb, &[255; 4]);
        let flat = one(wgpu::TextureFormat::Rg16Float, &[0; 4]);
        let none = one(wgpu::TextureFormat::R8Unorm, &[0]);
        let no_table = one(wgpu::TextureFormat::Rgba16Float, &[0; 8]);
        let plain = group(
            device,
            &layout,
            [&sampler, &table_sampler],
            [&white, &flat, &none, &none, &none],
            [&no_table, &no_table, &no_table],
            &no_table,
        );
        MapsPass {
            layout,
            sampler,
            table_sampler,
            plain,
            uploaded: HashMap::new(),
        }
    }

    /// Uploads the maps of this frame's spheres that are not yet, and lets
    /// go of those no one else holds: a body leaving the view and coming
    /// back is not uploaded again (found in review: 70–100 ms each time,
    /// for Earth's).
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        post: &crate::post::Post,
        maps: &[Option<Arc<Maps>>],
    ) {
        self.uploaded.retain(|_, (m, _)| Arc::strong_count(m) > 1);
        for m in maps.iter().flatten() {
            let key = Arc::as_ptr(m) as usize;
            if self.uploaded.contains_key(&key) {
                continue;
            }
            let mut views = Vec::with_capacity(TEXTURES);
            let plain = |format, data: &[u8]| {
                single(device, queue, &Image::new(1, 1, format, data.to_vec()))
            };
            views.push(post.upload_with_mips(device, queue, encoder, &m.color));
            views.push(match &m.relief {
                Some(i) => post.upload_with_mips(device, queue, encoder, i),
                None => plain(wgpu::TextureFormat::Rg16Float, &[0; 4]),
            });
            for map in [&m.clouds, &m.night, &m.water] {
                views.push(match map {
                    Some(i) => post.upload_with_mips(device, queue, encoder, i),
                    None => plain(wgpu::TextureFormat::R8Unorm, &[0]),
                });
            }
            let tables = match &m.air {
                Some(air) => [
                    &air.tables.transmittance,
                    &air.tables.multiple,
                    &air.tables.skylight,
                ]
                .map(|t| single(device, queue, t)),
                None => [0; TABLES].map(|_| plain(wgpu::TextureFormat::Rgba16Float, &[0; 8])),
            };
            let rings = match &m.rings {
                Some(r) => post.upload_with_mips(device, queue, encoder, &r.profile),
                None => plain(wgpu::TextureFormat::Rgba16Float, &[0; 8]),
            };
            let g = group(
                device,
                &self.layout,
                [&self.sampler, &self.table_sampler],
                std::array::from_fn(|k| &views[k]),
                std::array::from_fn(|k| &tables[k]),
                &rings,
            );
            self.uploaded.insert(key, (m.clone(), g));
        }
    }

    /// The bind group for a sphere with `maps`.
    pub(crate) fn group(&self, maps: Option<&Arc<Maps>>) -> &wgpu::BindGroup {
        maps.and_then(|m| self.uploaded.get(&(Arc::as_ptr(m) as usize)))
            .map_or(&self.plain, |(_, g)| g)
    }
}

/// A one-level texture of `image`.
fn single(device: &wgpu::Device, queue: &wgpu::Queue, image: &Image) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: image.width,
        height: image.height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("map"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: image.format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &image.data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.pixel() * image.width),
            rows_per_image: None,
        },
        size,
    );
    texture.create_view(&Default::default())
}

fn group<'a>(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    [sampler, table_sampler]: [&'a wgpu::Sampler; 2],
    views: [&'a wgpu::TextureView; TEXTURES],
    tables: [&'a wgpu::TextureView; TABLES],
    rings: &'a wgpu::TextureView,
) -> wgpu::BindGroup {
    fn view(binding: usize, v: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: wgpu::BindingResource::TextureView(v),
        }
    }
    let mut entries: Vec<wgpu::BindGroupEntry> =
        views.iter().enumerate().map(|(k, v)| view(k, v)).collect();
    entries.push(wgpu::BindGroupEntry {
        binding: TEXTURES as u32,
        resource: wgpu::BindingResource::Sampler(sampler),
    });
    entries.extend(
        tables
            .iter()
            .enumerate()
            .map(|(k, v)| view(TEXTURES + 1 + k, v)),
    );
    entries.push(wgpu::BindGroupEntry {
        binding: (TEXTURES + 1 + TABLES) as u32,
        resource: wgpu::BindingResource::Sampler(table_sampler),
    });
    entries.push(view(RINGS_BINDING as usize, rings));
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("maps"),
        layout,
        entries: &entries,
    })
}

/// The per-sphere constants maps add (part of `SphereGpu`): the body's
/// axes in view space, and how its maps are used.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct SurfaceGpu {
    /// Columns: the body's x (prime meridian), y (90° east) and z (north)
    /// axes, in view space.
    pub axes: [[f32; 4]; 3],
    /// Night lights' radiance at full value.
    pub night: [f32; 3],
    /// `flags::*`.
    pub flags: u32,
    pub cloud_albedo: f32,
    pub _pad: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_round_trip() {
        for x in [
            0.0f32, 1.0, -2.0, 0.5, 65504.0, 1e-5, -0.333, 6.1e-5, 3.0e-8,
        ] {
            let h = f32_to_f16(x);
            let back = crate::offscreen::f16_to_f32(h);
            assert!((back - x).abs() <= x.abs() * 1e-3 + 6e-8, "{x} -> {back}");
        }
        assert_eq!(f32_to_f16(1.0), 0x3c00);
        assert_eq!(f32_to_f16(f32::INFINITY), 0x7c00);
    }

    /// Color counts by how clear the sky over it is, the clouds' map at
    /// any size: finer, each color texel takes the mean of the clouds'
    /// over it (found in review: it took one), so black under 0.52 of
    /// cover and white under 0.018 is 0.67 white, the cover the clouds'
    /// own mean; under even cover, the plain mean.
    #[test]
    fn means_weigh_by_the_clear_sky() {
        let color = Image::srgba(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]);
        let (mean, cover) = means(
            &color,
            Some(&Image::gray(4, 2, vec![255, 255, 0, 0, 9, 9, 9, 9])),
        );
        let clear = [1.0 - 528.0 / 4.0 / 255.0, 1.0 - 18.0 / 4.0 / 255.0];
        let want = clear[1] / (clear[0] + clear[1]);
        assert!(
            mean.iter().all(|m| (f64::from(*m) - want).abs() < 1e-6)
                && (f64::from(cover) - 546.0 / 8.0 / 255.0).abs() < 1e-6,
            "{mean:?} {cover}"
        );
        let (mean, cover) = means(&color, Some(&Image::gray(1, 1, vec![51])));
        assert!(
            (mean[0] - 0.5).abs() < 1e-6 && (cover - 0.2).abs() < 1e-6,
            "{mean:?} {cover}"
        );
        assert_eq!(means(&color, None), ([0.5; 3], 0.0));
    }

    /// Maps stay uploaded while anyone else holds them, through frames
    /// that do not draw them, and go once no one does. Found in review:
    /// they went the first frame without them, and Earth's took 70–100 ms
    /// to upload again on its return.
    #[test]
    fn maps_stay_while_held() {
        let gpu = crate::offscreen::Offscreen::new().expect("a GPU");
        let post = crate::post::Post::new(&gpu.device);
        let mut pass = MapsPass::new(&gpu.device, &gpu.queue);
        let maps = Arc::new(Maps {
            color: Image::srgba(1, 1, vec![255; 4]),
            mean: [1.0; 3],
            cover: 0.0,
            relief: None,
            clouds: None,
            cloud_albedo: 0.0,
            night: None,
            night_radiance: [0.0; 3],
            water: None,
            shading: Shading::Lambert,
            air: None,
            rings: None,
        });
        let mut frame = |held: &[Option<Arc<Maps>>]| {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            pass.prepare(&gpu.device, &gpu.queue, &mut encoder, &post, held);
            gpu.queue.submit(Some(encoder.finish()));
            pass.uploaded.len()
        };
        assert_eq!(frame(&[Some(maps.clone())]), 1);
        assert_eq!(frame(&[]), 1, "held, not drawn");
        drop(maps);
        assert_eq!(frame(&[]), 0);
    }

    /// A slope rising eastward by 1 km per 10 km tilts the normal west.
    #[test]
    fn relief_tilts_away_from_rises() {
        let (w, h) = (360, 180);
        let radius = 3_000.0;
        let km_per_column = std::f64::consts::TAU * radius / f64::from(w);
        let heights: Vec<f32> = (0..h)
            .flat_map(|_| (0..w).map(move |x| (f64::from(x) * km_per_column * 0.1) as f32))
            .collect();
        let image = relief(w, h, &heights, radius);
        // At the equator, away from the seam.
        let k = ((h / 2) * w + 100) as usize * 4;
        let east =
            crate::offscreen::f16_to_f32(u16::from_le_bytes([image.data[k], image.data[k + 1]]));
        let north = crate::offscreen::f16_to_f32(u16::from_le_bytes([
            image.data[k + 2],
            image.data[k + 3],
        ]));
        let want = -0.1 / (1.0f32 + 0.01).sqrt();
        assert!((east - want).abs() < 2e-3, "{east} vs {want}");
        assert!(north.abs() < 1e-3, "{north}");
    }
}
