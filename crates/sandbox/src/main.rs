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
    /// The date the clock starts at.
    #[arg(long, default_value = "2045-03-01")]
    date: orbit::time::Epoch,
    /// The body the camera starts focused on, by ephemeris id.
    #[arg(long, default_value = "earth")]
    focus: String,
    /// The camera's starting altitude above the focus, km.
    #[arg(long, allow_hyphen_values = true)]
    altitude: Option<f64>,
    /// The camera's starting azimuth and elevation about the focus,
    /// degrees.
    #[arg(long, allow_hyphen_values = true)]
    azimuth: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    elevation: Option<f64>,
}

fn size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let n = |x: &str| x.parse::<u32>().map_err(|e| e.to_string());
    Ok((n(w)?, n(h)?))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let eph = orbit::ephem::Ephemeris::standard();
    let (first, last) = eph.span();
    if cli.date.0 < first.0 || cli.date.0 > last.0 {
        return Err(format!("the ephemeris covers {first} to {last}").into());
    }
    let mut app = sandbox::app::Shell::new(cli.date);
    let focus = eph
        .lookup(&cli.focus)
        .filter(|&b| sandbox::bodies::drawn(eph).contains(&b) && eph.covers(b, cli.date))
        .ok_or_else(|| format!("no body {:?} drawn on {}", cli.focus, cli.date))?;
    let mut camera = sandbox::camera::MapCamera::new(eph, focus);
    if let Some(a) = cli.altitude {
        camera.set_altitude(eph, a);
    }
    camera.set_direction(
        cli.azimuth.map_or(camera.azimuth, f64::to_radians),
        cli.elevation.map_or(camera.elevation, f64::to_radians),
    );
    app.camera = camera;
    match cli.screenshot {
        Some(path) => {
            let gpu = render::offscreen::Offscreen::new()?;
            let (w, h) = cli.size;
            let shot = sandbox::frame::shot(&gpu, &mut app, None, w, h);
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
