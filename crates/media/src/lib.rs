//! ffmpeg / ffprobe integration via external processes.

mod audio;
mod decode;
mod encode;
mod probe;
mod process;
#[cfg(feature = "testing")]
pub mod testing;

use std::path::{Path, PathBuf};

pub use decode::{DecodeOptions, Decoder, FrameReceiver};
pub use encode::{Audio, AudioSource, Codec, EncodeOptions, EncodeTarget, Encoder, command_line};
pub use probe::{Rational, VideoInfo};

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("{0} not found (install ffmpeg or set {1})")]
    NotFound(&'static str, &'static str),
    #[error("failed to run {program}: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("ffprobe failed: {0}")]
    Probe(String),
    #[error("ffmpeg failed ({status}): {stderr}")]
    Ffmpeg { status: String, stderr: String },
    #[error("{0}: no video stream")]
    NoVideoStream(PathBuf),
    #[error("invalid option: {0}")]
    InvalidOption(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = MediaError> = std::result::Result<T, E>;

pub const FFMPEG_ENV: &str = "MOTION_EXTRACTOR_FFMPEG";
pub const FFPROBE_ENV: &str = "MOTION_EXTRACTOR_FFPROBE";

/// Locations of the ffmpeg and ffprobe executables.
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Ffmpeg {
    pub fn new(ffmpeg: impl Into<PathBuf>, ffprobe: impl Into<PathBuf>) -> Self {
        Self {
            ffmpeg: ffmpeg.into(),
            ffprobe: ffprobe.into(),
        }
    }

    /// Finds ffmpeg/ffprobe: environment variable, then next to the running executable (bundled),
    /// then `PATH`.
    pub fn locate() -> Result<Self> {
        Ok(Self {
            ffmpeg: find_tool("ffmpeg", FFMPEG_ENV)?,
            ffprobe: find_tool("ffprobe", FFPROBE_ENV)?,
        })
    }

    /// First line of `ffmpeg -version`.
    pub fn version(&self) -> Result<String> {
        let out = process::command(&self.ffmpeg)
            .arg("-version")
            .output()
            .map_err(|source| MediaError::Spawn {
                program: self.ffmpeg.display().to_string(),
                source,
            })?;
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text.lines().next().unwrap_or_default().to_owned())
    }
}

fn find_tool(name: &'static str, env: &'static str) -> Result<PathBuf> {
    if let Some(p) = std::env::var_os(env) {
        return Ok(PathBuf::from(p));
    }
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&file)));
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|dir| dir.join(&file));
    bundled
        .into_iter()
        .chain(on_path)
        .find(|p| is_file(p))
        .ok_or(MediaError::NotFound(name, env))
}

fn is_file(p: &Path) -> bool {
    p.metadata().map(|m| m.is_file()).unwrap_or(false)
}
