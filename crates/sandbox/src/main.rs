//! `sandbox`: the map view in a window, or one frame of it as a PNG.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "sandbox",
    about = "The interplanetary economy's creative sandbox"
)]
struct Cli {
    /// Draw one frame offscreen to this PNG instead of opening a window.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// The screenshot's size, pixels.
    #[arg(long, default_value = "1280x800", value_parser = size)]
    size: (u32, u32),
    /// Close the window after this many frames, and print their times.
    #[arg(long)]
    frames: Option<u64>,
}

fn size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let n = |x: &str| x.parse::<u32>().map_err(|e| e.to_string());
    Ok((n(w)?, n(h)?))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut app = sandbox::app::Shell::default();
    match cli.screenshot {
        Some(path) => {
            let gpu = render::offscreen::Offscreen::new()?;
            let (w, h) = cli.size;
            let shot = sandbox::frame::shot(&gpu, &mut app, render::Scene::default(), w, h);
            let file = std::fs::File::create(&path)?;
            let mut png = png::Encoder::new(std::io::BufWriter::new(file), shot.width, shot.height);
            png.set_color(png::ColorType::Rgba);
            png.set_depth(png::BitDepth::Eight);
            png.write_header()?.write_image_data(&shot.rgba)?;
            println!("{}", path.display());
            Ok(())
        }
        None => sandbox::host::run(app, cli.frames),
    }
}
