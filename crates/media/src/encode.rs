use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};

use mx_core::Frame;
use serde::{Deserialize, Serialize};

use crate::process::{StderrTail, command};
use crate::{Ffmpeg, MediaError, Rational, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    #[default]
    H264,
    H265,
    Vp9,
    Prores,
    Ffv1,
    Png,
}

impl Codec {
    pub const ALL: [Codec; 6] = [
        Codec::H264,
        Codec::H265,
        Codec::Vp9,
        Codec::Prores,
        Codec::Ffv1,
        Codec::Png,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::H265 => "h265",
            Codec::Vp9 => "vp9",
            Codec::Prores => "prores",
            Codec::Ffv1 => "ffv1",
            Codec::Png => "png",
        }
    }

    pub fn from_name(name: &str) -> Option<Codec> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }

    /// File extension of the default container.
    pub fn extension(self) -> &'static str {
        match self {
            Codec::H264 | Codec::H265 => "mp4",
            Codec::Vp9 => "webm",
            Codec::Prores => "mov",
            Codec::Ffv1 => "mkv",
            Codec::Png => "png",
        }
    }

    /// Codec implied by an output file name (`out.webm` → VP9, `out.mov` → ProRes, …).
    pub fn from_path(path: &Path) -> Codec {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("webm") => Codec::Vp9,
            Some("mov") => Codec::Prores,
            Some("png") => Codec::Png,
            _ if path.to_string_lossy().contains('%') => Codec::Png,
            _ => Codec::H264,
        }
    }

    pub fn default_crf(self) -> Option<u32> {
        match self {
            Codec::H264 => Some(18),
            Codec::H265 => Some(20),
            Codec::Vp9 => Some(31),
            Codec::Prores | Codec::Ffv1 | Codec::Png => None,
        }
    }

    /// Encoder speed preset (or ProRes profile).
    pub fn default_preset(self) -> Option<&'static str> {
        match self {
            Codec::H264 | Codec::H265 => Some("medium"),
            Codec::Vp9 => Some("2"),
            Codec::Prores => Some("3"),
            Codec::Ffv1 | Codec::Png => None,
        }
    }

    pub fn default_pix_fmt(self) -> &'static str {
        match self {
            Codec::H264 | Codec::H265 | Codec::Vp9 => "yuv420p",
            Codec::Prores => "yuv422p10le",
            Codec::Ffv1 => "gbrp",
            Codec::Png => "rgb24",
        }
    }

    pub fn supports_audio(self) -> bool {
        self != Codec::Png
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audio {
    /// Re-encode the original audio (AAC, or Opus for WebM).
    #[default]
    Encode,
    /// Copy the original audio stream unchanged (fails if the container does not support it).
    Copy,
    None,
}

/// User-facing encoder options; unset values use the codec's defaults.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EncodeOptions {
    pub codec: Codec,
    pub crf: Option<u32>,
    pub encoder_preset: Option<String>,
    pub pix_fmt: Option<String>,
    pub audio: Audio,
    /// Extra arguments inserted before the output path.
    pub extra_args: Vec<String>,
}

/// Where the audio for the output comes from (a time range of the input file).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioSource {
    pub path: PathBuf,
    pub start: f64,
    pub duration: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EncodeTarget {
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub output: PathBuf,
    pub audio: Option<AudioSource>,
}

impl EncodeTarget {
    /// The path ffmpeg writes to: PNG sequences get a `frame_%06d.png` pattern inside the output
    /// directory unless the path already contains a `%` pattern.
    pub fn output_path(&self, codec: Codec) -> PathBuf {
        if codec == Codec::Png && !self.output.to_string_lossy().contains('%') {
            self.output.join("frame_%06d.png")
        } else {
            self.output.clone()
        }
    }
}

impl Ffmpeg {
    /// Full ffmpeg argument list for encoding raw RGB frames from stdin.
    pub fn encode_args(&self, opts: &EncodeOptions, target: &EncodeTarget) -> Vec<String> {
        let codec = opts.codec;
        let mut a: Vec<String> = Vec::new();
        let mut push = |items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));
        push(&["-hide_banner", "-nostdin", "-v", "error", "-y"]);
        push(&["-f", "rawvideo", "-pix_fmt", "rgb24"]);
        push(&["-s", &format!("{}x{}", target.width, target.height)]);
        push(&["-framerate", &target.fps.to_string(), "-i", "pipe:0"]);

        let audio = target
            .audio
            .as_ref()
            .filter(|_| codec.supports_audio() && opts.audio != Audio::None);
        if let Some(src) = audio {
            if src.start > 0.0 {
                push(&["-ss", &format!("{:.6}", src.start)]);
            }
            // Limiting the audio input (instead of -shortest) keeps every video frame.
            push(&["-t", &format!("{:.6}", src.duration)]);
            push(&["-i", &src.path.display().to_string()]);
            push(&["-map", "0:v:0", "-map", "1:a:0?"]);
        }

        let pix_fmt = opts.pix_fmt.as_deref().unwrap_or(codec.default_pix_fmt());
        let crf = opts.crf.or(codec.default_crf());
        let preset = opts.encoder_preset.as_deref().or(codec.default_preset());
        let mut filters = Vec::new();
        let yuv = pix_fmt.starts_with("yuv");
        if yuv && !pix_fmt.starts_with("yuv444") {
            // Chroma subsampling needs even dimensions.
            filters.push("crop=trunc(iw/2)*2:trunc(ih/2)*2".to_owned());
        }
        if yuv {
            filters.push("scale=out_color_matrix=bt709:out_range=tv".to_owned());
        }
        filters.push(format!("format={pix_fmt}"));
        push(&["-vf", &filters.join(",")]);

        match codec {
            Codec::H264 => push(&["-c:v", "libx264"]),
            Codec::H265 => push(&[
                "-c:v",
                "libx265",
                "-tag:v",
                "hvc1",
                "-x265-params",
                "log-level=error",
            ]),
            Codec::Vp9 => push(&["-c:v", "libvpx-vp9", "-b:v", "0", "-row-mt", "1"]),
            Codec::Prores => push(&["-c:v", "prores_ks"]),
            Codec::Ffv1 => push(&["-c:v", "ffv1", "-level", "3"]),
            Codec::Png => push(&["-c:v", "png"]),
        }
        if let Some(crf) = crf.filter(|_| codec.default_crf().is_some()) {
            push(&["-crf", &crf.to_string()]);
        }
        if let Some(preset) = preset.filter(|_| codec.default_preset().is_some()) {
            match codec {
                Codec::Vp9 => push(&["-deadline", "good", "-cpu-used", preset]),
                Codec::Prores => push(&["-profile:v", preset]),
                _ => push(&["-preset", preset]),
            }
        }
        if yuv {
            push(&[
                "-colorspace",
                "bt709",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "bt709",
            ]);
        }
        if audio.is_some() {
            match (opts.audio, codec) {
                (Audio::Copy, _) => push(&["-c:a", "copy"]),
                (_, Codec::Vp9) => push(&["-c:a", "libopus", "-b:a", "160k"]),
                _ => push(&["-c:a", "aac", "-b:a", "192k"]),
            }
        }
        let out = target.output_path(codec);
        let ext = out.extension().and_then(|e| e.to_str()).unwrap_or_default();
        if ["mp4", "mov", "m4v"].contains(&ext.to_ascii_lowercase().as_str()) {
            push(&["-movflags", "+faststart"]);
        }
        a.extend(opts.extra_args.iter().cloned());
        a.push(out.display().to_string());
        a
    }

    pub fn encode(&self, opts: &EncodeOptions, target: &EncodeTarget) -> Result<Encoder> {
        if target.width == 0 || target.height == 0 {
            return Err(MediaError::InvalidOption("empty output size".into()));
        }
        let out = target.output_path(opts.codec);
        if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let mut child = command(&self.ffmpeg)
            .args(self.encode_args(opts, target))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| MediaError::Spawn {
                program: self.ffmpeg.display().to_string(),
                source,
            })?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stderr = StderrTail::spawn(child.stderr.take().expect("piped stderr"));
        Ok(Encoder {
            child,
            stdin: Some(stdin),
            stderr,
            size: (target.width, target.height),
            finished: false,
        })
    }
}

/// A running ffmpeg process consuming raw RGB frames.
pub struct Encoder {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr: StderrTail,
    size: (u32, u32),
    finished: bool,
}

impl Encoder {
    pub fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        if (frame.width(), frame.height()) != self.size {
            return Err(MediaError::InvalidOption(format!(
                "frame is {}x{}, encoder expects {}x{}",
                frame.width(),
                frame.height(),
                self.size.0,
                self.size.1
            )));
        }
        let stdin = self.stdin.as_mut().expect("encoder already finished");
        match stdin.write_all(frame.data()) {
            Ok(()) => Ok(()),
            // ffmpeg exited early; report its error instead of the broken pipe.
            Err(e) if e.kind() == ErrorKind::BrokenPipe => Err(self.wait_error()),
            Err(e) => Err(e.into()),
        }
    }

    /// Closes the input and waits for ffmpeg to finish writing the file.
    pub fn finish(mut self) -> Result<()> {
        self.stdin = None;
        self.finished = true;
        let status = self.child.wait()?;
        let stderr = self.stderr.finish();
        if status.success() {
            Ok(())
        } else {
            Err(MediaError::Ffmpeg {
                status: status.to_string(),
                stderr,
            })
        }
    }

    fn wait_error(&mut self) -> MediaError {
        self.stdin = None;
        self.finished = true;
        let status = self.child.wait().map(|s| s.to_string()).unwrap_or_default();
        MediaError::Ffmpeg {
            status,
            stderr: self.stderr.finish(),
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Shell-style rendering of a command for display.
pub fn command_line(program: &Path, args: &[String]) -> String {
    let quote = |s: &str| {
        let safe = !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./:=+,%@".contains(c));
        if safe {
            s.to_owned()
        } else {
            format!("'{}'", s.replace('\'', r"'\''"))
        }
    };
    std::iter::once(program.display().to_string().as_str())
        .chain(args.iter().map(String::as_str))
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(output: &str) -> EncodeTarget {
        EncodeTarget {
            width: 640,
            height: 360,
            fps: Rational::new(30, 1),
            output: output.into(),
            audio: None,
        }
    }

    fn args(opts: &EncodeOptions, t: &EncodeTarget) -> String {
        let ff = Ffmpeg::new("ffmpeg", "ffprobe");
        ff.encode_args(opts, t).join(" ")
    }

    #[test]
    fn h264_defaults() {
        let a = args(&EncodeOptions::default(), &target("out.mp4"));
        assert!(a.contains("-s 640x360 -framerate 30/1 -i pipe:0"), "{a}");
        assert!(a.contains("-c:v libx264 -crf 18 -preset medium"), "{a}");
        assert!(a.contains("format=yuv420p"), "{a}");
        assert!(a.contains("-movflags +faststart"), "{a}");
        assert!(a.ends_with("out.mp4"));
        assert!(!a.contains("-c:a"));
    }

    #[test]
    fn overrides_and_extra_args() {
        let opts = EncodeOptions {
            codec: Codec::H265,
            crf: Some(28),
            encoder_preset: Some("fast".into()),
            pix_fmt: Some("yuv444p".into()),
            extra_args: vec!["-tune".into(), "grain".into()],
            ..Default::default()
        };
        let a = args(&opts, &target("out.mkv"));
        assert!(a.contains("-c:v libx265"), "{a}");
        assert!(a.contains("-crf 28 -preset fast"), "{a}");
        assert!(a.contains("format=yuv444p") && !a.contains("crop="), "{a}");
        assert!(a.ends_with("-tune grain out.mkv"), "{a}");
        assert!(!a.contains("faststart"));
    }

    #[test]
    fn audio_handling() {
        let mut t = target("out.webm");
        t.audio = Some(AudioSource {
            path: "in.mp4".into(),
            start: 1.5,
            duration: 2.0,
        });
        let vp9 = EncodeOptions {
            codec: Codec::Vp9,
            ..Default::default()
        };
        let a = args(&vp9, &t);
        assert!(
            a.contains("-ss 1.500000 -t 2.000000 -i in.mp4 -map 0:v:0 -map 1:a:0?"),
            "{a}"
        );
        assert!(a.contains("-c:a libopus"), "{a}");
        assert!(a.contains("-deadline good -cpu-used 2"), "{a}");
        let copy = EncodeOptions {
            audio: Audio::Copy,
            ..Default::default()
        };
        assert!(args(&copy, &t).contains("-c:a copy"));
        let none = EncodeOptions {
            audio: Audio::None,
            ..Default::default()
        };
        assert!(!args(&none, &t).contains("in.mp4"));
        let png = EncodeOptions {
            codec: Codec::Png,
            ..Default::default()
        };
        assert!(!args(&png, &t).contains("-c:a"));
    }

    #[test]
    fn png_sequence_path() {
        let png = EncodeOptions {
            codec: Codec::Png,
            ..Default::default()
        };
        let a = args(&png, &target("frames"));
        assert!(
            a.ends_with(&format!(
                "{}",
                Path::new("frames").join("frame_%06d.png").display()
            )),
            "{a}"
        );
        assert!(args(&png, &target("x/img%03d.png")).ends_with("x/img%03d.png"));
    }

    #[test]
    fn prores_and_ffv1() {
        let prores = EncodeOptions {
            codec: Codec::Prores,
            crf: Some(10),
            ..Default::default()
        };
        let a = args(&prores, &target("o.mov"));
        assert!(a.contains("-c:v prores_ks -profile:v 3"), "{a}");
        assert!(!a.contains("-crf"), "{a}");
        let ffv1 = EncodeOptions {
            codec: Codec::Ffv1,
            ..Default::default()
        };
        let a = args(&ffv1, &target("o.mkv"));
        assert!(a.contains("format=gbrp -c:v ffv1"), "{a}");
        assert!(!a.contains("bt709"));
    }

    #[test]
    fn codec_from_path() {
        assert_eq!(Codec::from_path(Path::new("a.MP4")), Codec::H264);
        assert_eq!(Codec::from_path(Path::new("a.webm")), Codec::Vp9);
        assert_eq!(Codec::from_path(Path::new("a.mov")), Codec::Prores);
        assert_eq!(Codec::from_path(Path::new("f_%04d.png")), Codec::Png);
        assert_eq!(Codec::from_name("ffv1"), Some(Codec::Ffv1));
    }

    #[test]
    fn command_line_quotes() {
        let s = command_line(
            Path::new("ffmpeg"),
            &["-i".into(), "my video.mp4".into(), "it's".into()],
        );
        assert_eq!(s, r"ffmpeg -i 'my video.mp4' 'it'\''s'");
    }
}
