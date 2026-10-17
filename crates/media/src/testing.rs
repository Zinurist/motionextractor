//! Synthetic test videos generated with ffmpeg `lavfi` sources (no binary fixtures in the repo).

use std::path::{Path, PathBuf};

use crate::Ffmpeg;
use crate::process::command;

/// Locates ffmpeg or panics with a helpful message (tests require ffmpeg).
pub fn ffmpeg() -> Ffmpeg {
    Ffmpeg::locate().expect("tests require ffmpeg and ffprobe on PATH")
}

fn run(ff: &Ffmpeg, args: &[String]) {
    let out = command(&ff.ffmpeg)
        // Single-threaded: tests generate fixtures in parallel.
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-y",
            "-filter_threads",
            "1",
            "-threads",
            "1",
        ])
        .args(args)
        .output()
        .expect("run ffmpeg");
    assert!(
        out.status.success(),
        "fixture generation failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A white square moving horizontally over black, encoded losslessly (FFV1, RGB).
///
/// In frame `n` the square covers `x ∈ [x0 + n·speed, x0 + n·speed + size)`, `y ∈ [y, y + size)`.
#[derive(Debug, Clone)]
pub struct MovingSquare {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    pub size: u32,
    pub x0: u32,
    pub y: u32,
    pub speed: u32,
    pub audio: bool,
}

impl Default for MovingSquare {
    fn default() -> Self {
        Self {
            width: 160,
            height: 120,
            fps: 30,
            frames: 30,
            size: 16,
            x0: 8,
            y: 40,
            speed: 4,
            audio: false,
        }
    }
}

impl MovingSquare {
    /// Left edge of the square in frame `n`.
    pub fn x(&self, n: u32) -> u32 {
        self.x0 + n * self.speed
    }

    /// Whether pixel `(x, y)` is covered by the square in frame `n`.
    pub fn covers(&self, n: u32, x: u32, y: u32) -> bool {
        (self.x(n)..self.x(n) + self.size).contains(&x) && (self.y..self.y + self.size).contains(&y)
    }

    pub fn generate(&self, ff: &Ffmpeg, dir: &Path) -> PathBuf {
        let path = dir.join(format!(
            "square_{}x{}_{}f{}.mkv",
            self.width,
            self.height,
            self.frames,
            if self.audio { "_audio" } else { "" }
        ));
        let d = self.frames as f64 / self.fps as f64 + 1.0;
        let mut args: Vec<String> = vec![
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            format!(
                "color=c=black:s={}x{}:r={}:d={d}",
                self.width, self.height, self.fps
            ),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            format!("color=c=white:s={0}x{0}:r={1}:d={d}", self.size, self.fps),
        ];
        if self.audio {
            args.extend(["-f", "lavfi", "-i"].map(String::from));
            args.push(format!("sine=frequency=440:duration={d}"));
        }
        args.extend([
            "-filter_complex".into(),
            format!(
                // overlay's `n` is off by one on the first frame, so derive the index from `t`.
                "[0][1]overlay=x='{}+round(t*{})*{}':y={}:shortest=1:eval=frame,format=gbrp[v]",
                self.x0, self.fps, self.speed, self.y
            ),
            "-map".into(),
            "[v]".into(),
        ]);
        if self.audio {
            args.extend(["-map", "2:a", "-c:a", "flac", "-shortest"].map(String::from));
        }
        args.extend(["-frames:v".into(), self.frames.to_string()]);
        args.extend(["-c:v", "ffv1", "-level", "3"].map(String::from));
        args.push(path.display().to_string());
        run(ff, &args);
        path
    }
}

/// An H.264 video (container chosen by `ext`) whose frames are uniformly grey with luma `(4·n) mod 256` for frame `n`, with a
/// keyframe every 12 frames and B-frames — used to verify frame-accurate seeking.
pub fn index_video(ff: &Ffmpeg, dir: &Path, frames: u32, fps: &str, ext: &str) -> PathBuf {
    let path = dir.join(format!(
        "index_{frames}f_{}fps.{ext}",
        fps.replace('/', "_")
    ));
    let d = frames as f64 / 20.0 + 1.0;
    run(
        ff,
        &[
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            format!("color=c=black:s=64x48:r={fps}:d={d},format=gray,geq=lum='mod(N*4,256)'"),
            "-frames:v".into(),
            frames.to_string(),
            "-c:v".into(),
            "libx264".into(),
            "-qp".into(),
            "0".into(),
            "-g".into(),
            "12".into(),
            "-bf".into(),
            "2".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            path.display().to_string(),
        ],
    );
    path
}

/// Expected grey level of frame `n` of [`index_video`].
pub fn index_level(n: u64) -> u8 {
    ((n * 4) % 256) as u8
}
