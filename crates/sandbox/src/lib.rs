//! The creative sandbox (M1c.md): the map view's host and UI, and mission
//! plans played back in it.

pub mod app;
pub mod atmospheres;
pub mod background;
pub mod bodies;
pub mod camera;
pub mod clock;
pub mod editor;
pub mod frame;
pub mod frames;
pub mod host;
pub mod labels;
pub mod look;
pub mod maps;
pub mod options;
pub mod orbits;
pub mod panel;
pub mod paths;
pub mod photometry;
pub mod pick;
pub mod plan;
pub mod settings;
pub mod sky;
pub mod sun;
pub mod timeline;
pub mod windows;

/// The game's data directory, `data/` in the repository.
pub fn data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}
