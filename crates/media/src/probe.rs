use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::process::command;
use crate::{Ffmpeg, MediaError, Result};

/// A rational number such as a frame rate (`30000/1001`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rational {
    pub num: u32,
    pub den: u32,
}

impl Rational {
    pub fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }

    pub fn as_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    fn parse(s: &str) -> Option<Self> {
        let (n, d) = s.split_once('/').unwrap_or((s, "1"));
        let r = Self::new(n.trim().parse().ok()?, d.trim().parse().ok()?);
        (r.num > 0 && r.den > 0).then_some(r)
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// Properties of a video file, as needed by the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoInfo {
    pub path: PathBuf,
    /// Display size (after applying rotation metadata).
    pub width: u32,
    pub height: u32,
    /// Constant frame rate the video is normalised to.
    pub fps: Rational,
    pub frame_count: u64,
    pub duration: f64,
    pub has_audio: bool,
    pub codec: String,
    /// Container format as reported by ffprobe (e.g. `mov,mp4,m4a,3gp,3g2,mj2`).
    pub format: String,
    /// Timestamp of the first frame in seconds (non-zero e.g. for MPEG-TS).
    pub start_time: f64,
}

impl VideoInfo {
    pub fn fps_f64(&self) -> f64 {
        self.fps.as_f64()
    }

    /// Presentation time of frame `index` in seconds.
    pub fn frame_time(&self, index: u64) -> f64 {
        index as f64 / self.fps_f64()
    }

    /// Whether ffmpeg can seek this container exactly (it has a seek index). Other containers
    /// (e.g. MPEG-TS) may land after the requested position and need a seek margin.
    pub fn has_precise_seek(&self) -> bool {
        ["mp4", "mov", "matroska", "webm", "avi"]
            .iter()
            .any(|f| self.format.split(',').any(|x| x == *f))
    }

    /// Size with the given height, preserving aspect ratio, rounded to even dimensions.
    pub fn scaled_size(&self, height: u32) -> (u32, u32) {
        let h = height.min(self.height).max(2) & !1;
        let w = ((self.width as f64 * h as f64 / self.height as f64).round() as u32).max(2);
        ((w + 1) & !1, h)
    }
}

impl Ffmpeg {
    pub fn probe(&self, path: &Path) -> Result<VideoInfo> {
        let out = command(&self.ffprobe)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_streams",
                "-show_format",
            ])
            .arg(path)
            .output()
            .map_err(|source| MediaError::Spawn {
                program: self.ffprobe.display().to_string(),
                source,
            })?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            return Err(MediaError::Probe(if stderr.is_empty() {
                format!("{} ({})", path.display(), out.status)
            } else {
                stderr
            }));
        }
        let json: Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| MediaError::Probe(format!("invalid ffprobe output: {e}")))?;
        parse_probe(path, &json)
    }
}

fn parse_probe(path: &Path, json: &Value) -> Result<VideoInfo> {
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    let is_type = |s: &&Value, t: &str| s["codec_type"] == t;
    let video = streams
        .iter()
        // Skip cover art and other still images attached to audio files.
        .find(|s| is_type(s, "video") && s["disposition"]["attached_pic"] != 1)
        .ok_or_else(|| MediaError::NoVideoStream(path.to_owned()))?;
    let has_audio = streams.iter().any(|s| is_type(&s, "audio"));

    let str_field = |v: &Value, k: &str| v[k].as_str().map(str::to_owned);
    let num_field = |v: &Value, k: &str| -> Option<f64> {
        v[k].as_f64()
            .or_else(|| v[k].as_str().and_then(|s| s.parse().ok()))
    };

    let width = video["width"].as_u64().unwrap_or(0) as u32;
    let height = video["height"].as_u64().unwrap_or(0) as u32;
    if width == 0 || height == 0 {
        return Err(MediaError::Probe("video stream has no size".into()));
    }
    let (width, height) = if rotation(video).rem_euclid(180) == 90 {
        (height, width)
    } else {
        (width, height)
    };

    let avg = str_field(video, "avg_frame_rate").and_then(|s| Rational::parse(&s));
    let real = str_field(video, "r_frame_rate").and_then(|s| Rational::parse(&s));
    let fps = avg
        .or(real)
        .ok_or_else(|| MediaError::Probe("unknown frame rate".into()))?;

    // Matroska keeps per-stream durations in a tag; the container duration may include longer
    // audio, so it is the last resort.
    let duration = num_field(video, "duration")
        .or_else(|| tag_duration(video))
        .or_else(|| num_field(&json["format"], "duration"))
        .unwrap_or(0.0);
    let nb_frames = num_field(video, "nb_frames").map(|n| n as u64);
    let frame_count = match nb_frames {
        // nb_frames is exact for constant frame rate streams.
        Some(n) if n > 0 && avg == real => n,
        _ => (duration * fps.as_f64()).round() as u64,
    };
    if frame_count == 0 {
        return Err(MediaError::Probe("video has no frames".into()));
    }

    Ok(VideoInfo {
        path: path.to_owned(),
        width,
        height,
        fps,
        frame_count,
        duration: if duration > 0.0 {
            duration
        } else {
            frame_count as f64 / fps.as_f64()
        },
        has_audio,
        codec: str_field(video, "codec_name").unwrap_or_default(),
        format: str_field(&json["format"], "format_name").unwrap_or_default(),
        start_time: num_field(&json["format"], "start_time")
            .or_else(|| num_field(video, "start_time"))
            .unwrap_or(0.0),
    })
}

/// The `DURATION` stream tag (`HH:MM:SS.fraction`) written by the Matroska muxer.
fn tag_duration(video: &Value) -> Option<f64> {
    let tags = video["tags"].as_object()?;
    let value = tags
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("duration"))?
        .1
        .as_str()?;
    value.split(':').try_fold(0.0, |acc, part| {
        Some(acc * 60.0 + part.trim().parse::<f64>().ok()?)
    })
}

/// Rotation in degrees from the display matrix side data or the legacy `rotate` tag.
fn rotation(video: &Value) -> i64 {
    let side = video["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|d| {
            d["rotation"]
                .as_i64()
                .or_else(|| d["rotation"].as_f64().map(|r| r as i64))
        });
    side.or_else(|| {
        video["tags"]["rotate"]
            .as_str()
            .and_then(|s| s.parse().ok())
    })
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stream(extra: Value) -> Value {
        let mut s = json!({
            "codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080,
            "avg_frame_rate": "30/1", "r_frame_rate": "30/1", "duration": "2.0", "nb_frames": "60"
        });
        for (k, v) in extra.as_object().unwrap() {
            s[k] = v.clone();
        }
        s
    }

    #[test]
    fn parses_basic_stream() {
        let j = json!({"streams": [stream(json!({})), {"codec_type": "audio"}], "format": {}});
        let info = parse_probe(Path::new("a.mp4"), &j).unwrap();
        assert_eq!((info.width, info.height), (1920, 1080));
        assert_eq!(info.fps, Rational::new(30, 1));
        assert_eq!(info.frame_count, 60);
        assert!(info.has_audio);
        assert_eq!(info.codec, "h264");
    }

    #[test]
    fn rotation_swaps_size() {
        let j = json!({"streams": [stream(json!({"side_data_list": [{"rotation": -90}]}))]});
        let info = parse_probe(Path::new("a.mp4"), &j).unwrap();
        assert_eq!((info.width, info.height), (1080, 1920));
        assert!(!info.has_audio);
    }

    #[test]
    fn variable_frame_rate_uses_duration() {
        let j = json!({"streams": [stream(json!({
            "avg_frame_rate": "24000/1001", "r_frame_rate": "60/1", "nb_frames": "100",
            "duration": null
        }))], "format": {"duration": "10.0"}});
        let info = parse_probe(Path::new("a.mkv"), &j).unwrap();
        assert_eq!(info.fps, Rational::new(24000, 1001));
        assert_eq!(info.frame_count, 240);
    }

    #[test]
    fn matroska_duration_tag() {
        let j = json!({"streams": [stream(json!({
            "duration": null, "nb_frames": null, "tags": {"DURATION": "00:00:01.000000000"}
        }))], "format": {"duration": "1.034"}});
        let info = parse_probe(Path::new("a.mkv"), &j).unwrap();
        assert_eq!(info.frame_count, 30);
        assert_eq!(info.duration, 1.0);
    }

    #[test]
    fn errors() {
        let j = json!({"streams": [{"codec_type": "audio"}]});
        assert!(matches!(
            parse_probe(Path::new("a"), &j),
            Err(MediaError::NoVideoStream(_))
        ));
        let j =
            json!({"streams": [stream(json!({"avg_frame_rate": "0/0", "r_frame_rate": "0/0"}))]});
        assert!(matches!(
            parse_probe(Path::new("a"), &j),
            Err(MediaError::Probe(_))
        ));
    }

    #[test]
    fn scaled_size_is_even() {
        let info = parse_probe(Path::new("a"), &json!({"streams": [stream(json!({}))]})).unwrap();
        assert_eq!(info.scaled_size(540), (960, 540));
        assert_eq!(info.scaled_size(5000), (1920, 1080));
        let odd = VideoInfo {
            width: 101,
            height: 75,
            ..info
        };
        assert_eq!(odd.scaled_size(75), (100, 74));
    }
}
