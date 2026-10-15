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
    /// A mission plan to design and play (data/missions/).
    #[arg(long)]
    plan: Option<PathBuf>,
    /// The date the clock starts at, YYYY-MM-DD[THH:MM] (TDB): by default
    /// the plan's first ship's start, or 2045-03-01.
    #[arg(long)]
    date: Option<orbit::time::Epoch>,
    /// The frame paths are drawn in: focus, earth-moon,
    /// earth-moon-pulsating, sun-earth or sun-mars.
    #[arg(long, default_value = "focus")]
    frame: String,
    /// The body the camera starts focused on, by ephemeris id.
    #[arg(long, default_value = "earth")]
    focus: String,
    /// The camera's starting altitude above the focus (a shaped body's
    /// greatest radius), km.
    #[arg(long, allow_hyphen_values = true)]
    altitude: Option<f64>,
    /// The camera's starting azimuth and elevation about the focus, about
    /// the frame's axes, degrees.
    #[arg(long, allow_hyphen_values = true)]
    azimuth: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    elevation: Option<f64>,
    /// Stops of exposure added by hand at the start, as `-` and `=` do.
    #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
    exposure: i32,
    /// The settings' departures from realism, in stops, 0 being true to
    /// life: the stars and the Milky Way over the scene's exposure,
    /// bodies seen as points over it, Earth's night lights over their
    /// radiance, and the Sun under its.
    #[arg(long, default_value_t = sandbox::look::BACKGROUND_LIFT, allow_hyphen_values = true)]
    background_lift: i32,
    #[arg(long, default_value_t = sandbox::look::BODY_LIFT, allow_hyphen_values = true)]
    body_lift: i32,
    #[arg(long, default_value_t = sandbox::look::NIGHT_LIGHTS, allow_hyphen_values = true)]
    night_lights: i32,
    #[arg(long, default_value_t = sandbox::look::SUN, allow_hyphen_values = true)]
    sun: i32,
}

fn size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let n = |x: &str| x.parse::<u32>().map_err(|e| e.to_string());
    Ok((n(w)?, n(h)?))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let eph = orbit::ephem::Ephemeris::standard();
    let plan = cli
        .plan
        .as_deref()
        .map(sandbox::plan::Loaded::open)
        .transpose()?;
    let date = cli
        .date
        .or(plan.as_ref().map(|p| p.start))
        .unwrap_or(orbit::time::Epoch::from_ymd(2045, 3, 1));
    let (first, last) = eph.span();
    if date.0 < first.0 || date.0 > last.0 {
        return Err(format!("the ephemeris covers {first} to {last}").into());
    }
    let frames = sandbox::frames::Frame::offered(eph);
    let frame = frames
        .iter()
        .find(|(id, _, _)| *id == cli.frame)
        .map(|&(_, _, f)| f)
        .ok_or_else(|| {
            let ids: Vec<&str> = frames.iter().map(|(id, _, _)| *id).collect();
            format!(
                "no frame {:?}; the frames are {}",
                cli.frame,
                ids.join(", ")
            )
        })?;
    let mut app = sandbox::app::Shell::new(date);
    app.plan = plan;
    let focus = eph
        .lookup(&cli.focus)
        .filter(|&b| sandbox::bodies::drawn(eph).contains(&b) && eph.covers(b, date))
        .ok_or_else(|| format!("no body {:?} drawn on {}", cli.focus, date))?;
    if cli.altitude.is_some() {
        // Over the focus's shape, which comes with its maps.
        sandbox::maps::wait()?;
    }
    let mut camera = sandbox::camera::MapCamera::new(eph, focus);
    if let Some(a) = cli.altitude {
        camera.set_altitude(eph, a);
    }
    camera.set_direction(
        cli.azimuth.map_or(camera.azimuth, f64::to_radians),
        cli.elevation.map_or(camera.elevation, f64::to_radians),
    );
    app.camera = camera;
    app.follow_focus();
    app.exposure.bias = cli
        .exposure
        .clamp(-sandbox::app::MAX_BIAS, sandbox::app::MAX_BIAS);
    app.settings = sandbox::settings::Settings {
        background_lift: cli.background_lift.clamp(0, sandbox::look::MAX_LIFT),
        body_lift: cli.body_lift.clamp(0, sandbox::look::MAX_LIFT),
        night_lights: cli.night_lights.clamp(0, sandbox::look::MAX_NIGHT_LIGHTS),
        sun: cli.sun.clamp(sandbox::look::MIN_SUN, 0),
        ..app.settings
    };
    // The view's direction is about this frame's axes.
    app.frame = frame;
    match cli.screenshot {
        Some(path) => {
            if let Some(plan) = app.plan.as_mut() {
                plan.wait();
                eprintln!("{}", plan.progress());
            }
            if let Err(why) = sandbox::sky::wait() {
                eprintln!("no sky: {why}");
            }
            if let Err(why) = sandbox::maps::wait() {
                eprintln!("no maps: {why}");
            }
            let gpu = render::offscreen::Offscreen::new()?;
            let (w, h) = cli.size;
            let shot = sandbox::frame::shot(&gpu, &mut app, None, w, h, 1.0);
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
