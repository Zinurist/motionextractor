use std::io::{ErrorKind, Read};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::mpsc;

use mx_core::Frame;

use crate::process::{StderrTail, command};
use crate::{Ffmpeg, MediaError, Result, VideoInfo};

/// How far before the target unindexed containers are seeked, in seconds.
const IMPRECISE_SEEK_MARGIN: f64 = 5.0;

#[derive(Debug, Clone, Default)]
pub struct DecodeOptions {
    /// First frame index to decode.
    pub start_frame: u64,
    /// Maximum number of frames; `None` decodes until the end.
    pub frame_count: Option<u64>,
    /// Output size; `None` keeps the native size.
    pub size: Option<(u32, u32)>,
    /// Use `-hwaccel auto`.
    pub hwaccel: bool,
}

/// A running ffmpeg process producing raw RGB frames.
pub struct Decoder {
    child: Child,
    stdout: ChildStdout,
    stderr: StderrTail,
    width: u32,
    height: u32,
    next_index: u64,
    done: bool,
}

impl Ffmpeg {
    /// Arguments for decoding `info` with `opts` to raw RGB on stdout.
    pub fn decode_args(&self, info: &VideoInfo, opts: &DecodeOptions) -> Vec<String> {
        let mut args: Vec<String> = ["-hide_banner", "-nostdin", "-v", "error"]
            .map(String::from)
            .to_vec();
        if opts.hwaccel {
            args.extend(["-hwaccel".into(), "auto".into()]);
        }
        // Target half a frame early: the previous frame is still dropped, but timestamp rounding
        // (e.g. millisecond precision in Matroska) can never skip the requested one.
        let target = (opts.start_frame as f64 - 0.5) / info.fps_f64();
        let margin = if info.has_precise_seek() {
            0.0
        } else {
            IMPRECISE_SEEK_MARGIN
        };
        let seek = target - margin;
        if opts.start_frame > 0 && seek > 0.0 {
            args.extend(["-ss".into(), format!("{seek:.6}")]);
        }
        args.extend(["-i".into(), info.path.display().to_string()]);
        // Keep the original timestamps so the fps filter's output grid stays aligned with the source
        // frames after a seek (with reset timestamps it can duplicate the first frame).
        args.extend(["-copyts", "-an", "-sn", "-dn"].map(String::from));
        let mut filter = format!("fps={}", info.fps);
        if opts.start_frame > 0 {
            // Exact cut on the original (absolute, thanks to -copyts) timestamps.
            filter.push_str(&format!(",trim=start={:.6}", info.start_time + target));
        }
        if let Some((w, h)) = opts.size {
            filter.push_str(&format!(",scale={w}:{h}:flags=area"));
        }
        args.extend(["-vf".into(), filter]);
        if let Some(n) = opts.frame_count {
            args.extend(["-frames:v".into(), n.to_string()]);
        }
        args.extend(["-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"].map(String::from));
        args
    }

    pub fn decode(&self, info: &VideoInfo, opts: &DecodeOptions) -> Result<Decoder> {
        let (width, height) = opts.size.unwrap_or((info.width, info.height));
        let mut child = command(&self.ffmpeg)
            .args(self.decode_args(info, opts))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| MediaError::Spawn {
                program: self.ffmpeg.display().to_string(),
                source,
            })?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = StderrTail::spawn(child.stderr.take().expect("piped stderr"));
        tracing::debug!(start = opts.start_frame, ?opts.frame_count, width, height, "decoder started");
        Ok(Decoder {
            child,
            stdout,
            stderr,
            width,
            height,
            next_index: opts.start_frame,
            done: false,
        })
    }
}

impl Decoder {
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Index of the frame the next `read_frame` call returns.
    pub fn next_index(&self) -> u64 {
        self.next_index
    }

    /// Reads the next frame, or `None` at the end of the stream.
    pub fn read_frame(&mut self) -> Result<Option<Frame>> {
        if self.done {
            return Ok(None);
        }
        let mut buf = vec![0u8; Frame::byte_len(self.width, self.height)];
        match self.stdout.read_exact(&mut buf) {
            Ok(()) => {
                self.next_index += 1;
                Ok(Some(
                    Frame::new(self.width, self.height, buf).expect("exact size"),
                ))
            }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                self.done = true;
                let status = self.child.wait()?;
                if status.success() {
                    Ok(None)
                } else {
                    Err(MediaError::Ffmpeg {
                        status: status.to_string(),
                        stderr: self.stderr.finish(),
                    })
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Moves decoding to a background thread that keeps up to `capacity` frames ready.
    pub fn into_threaded(mut self, capacity: usize) -> FrameReceiver {
        let (tx, rx) = mpsc::sync_channel(capacity);
        std::thread::spawn(move || {
            loop {
                let item = self.read_frame();
                let end = !matches!(item, Ok(Some(_)));
                if tx.send(item).is_err() || end {
                    break;
                }
            }
        });
        FrameReceiver { rx, done: false }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Receiving end of [`Decoder::into_threaded`]. Dropping it stops the decoder.
pub struct FrameReceiver {
    rx: mpsc::Receiver<Result<Option<Frame>>>,
    done: bool,
}

impl FrameReceiver {
    pub fn recv(&mut self) -> Result<Option<Frame>> {
        if self.done {
            return Ok(None);
        }
        let item = self.rx.recv().unwrap_or(Ok(None));
        if !matches!(item, Ok(Some(_))) {
            self.done = true;
        }
        item
    }
}
