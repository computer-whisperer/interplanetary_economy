//! Porkchop grids as PNG heatmaps.

use anyhow::Result;
use orbit::transfer::Porkchop;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// One-hue sequential ramp (light → dark), banded so contours of equal total
/// excess speed read as edges. The encoded magnitude is how far below
/// `best + span` a transfer is: the cheapest band is darkest.
const RAMP: [[u8; 3]; 7] = [
    [0xcd, 0xe2, 0xfb],
    [0x9e, 0xc5, 0xf4],
    [0x6d, 0xa7, 0xec],
    [0x39, 0x87, 0xe5],
    [0x25, 0x6a, 0xbf],
    [0x18, 0x4f, 0x95],
    [0x0d, 0x36, 0x6b],
];
const SURFACE: [u8; 3] = [0xff, 0xff, 0xff];

/// Writes the porkchop to `path`, upscaled to roughly 1000 px wide. Returns
/// the image size.
pub fn write(pc: &Porkchop, best: f64, span: f64, path: &Path) -> Result<(u32, u32)> {
    let g = pc.grid;
    let sx = (1000 / g.departs).max(1);
    let sy = (600 / g.tofs).max(1);
    let (width, height) = ((g.departs * sx) as u32, (g.tofs * sy) as u32);
    let band = span / RAMP.len() as f64;

    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for row in 0..g.tofs * sy {
        let j = g.tofs - 1 - row / sy; // longest time of flight at the top
        for col in 0..g.departs * sx {
            let color = pc
                .get(col / sx, j)
                .map(|t| t.v_inf_total() - best)
                .filter(|&excess| excess < span)
                .map_or(SURFACE, |excess| {
                    RAMP[RAMP.len() - 1 - (excess / band) as usize]
                });
            pixels.extend_from_slice(&color);
        }
    }

    let mut enc = png::Encoder::new(BufWriter::new(File::create(path)?), width, height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&pixels)?;
    Ok((width, height))
}
