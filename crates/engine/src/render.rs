//! Full-resolution render pipeline: decoder(s) → effect → encoder.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use mx_core::{
    Comparison, EffectError, EffectParams, Frame, ParamError, Prepared, process_prepared,
};
use mx_media::{
    AudioSource, Codec, DecodeOptions, EncodeOptions, EncodeTarget, Ffmpeg, FrameReceiver,
    MediaError, VideoInfo,
};
use serde::{Deserialize, Serialize};

/// Frames buffered between pipeline stages.
const CHANNEL_CAPACITY: usize = 4;

/// Render settings beyond the effect itself.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderOptions {
    #[serde(flatten)]
    pub encode: EncodeOptions,
    /// First frame to render (inclusive); default 0.
    pub start: Option<u64>,
    /// Last frame to render (exclusive); default: end of video.
    pub end: Option<u64>,
    /// Output height (aspect preserved); default: native size.
    pub height: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct RenderSpec {
    pub output: PathBuf,
    pub comparison: Comparison,
    pub effect: EffectParams,
    pub options: RenderOptions,
}

/// Everything decided before rendering starts; also used to preview the ffmpeg command.
#[derive(Debug, Clone)]
pub struct RenderPlan {
    pub start: u64,
    pub end: u64,
    pub width: u32,
    pub height: u32,
    pub target: EncodeTarget,
    pub encoder_args: Vec<String>,
}

impl RenderPlan {
    pub fn frame_count(&self) -> u64 {
        self.end - self.start
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub elapsed: Duration,
}

impl Progress {
    pub fn fps(&self) -> f64 {
        let s = self.elapsed.as_secs_f64();
        if s > 0.0 { self.done as f64 / s } else { 0.0 }
    }

    pub fn eta(&self) -> Option<Duration> {
        let fps = self.fps();
        (fps > 0.0).then(|| Duration::from_secs_f64((self.total - self.done) as f64 / fps))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Effect(#[from] EffectError),
    #[error(transparent)]
    Param(#[from] ParamError),
    #[error("invalid frame range {start}..{end} for a video with {count} frames")]
    InvalidRange { start: u64, end: u64, count: u64 },
    #[error("render cancelled")]
    Cancelled,
}

/// Internal knobs, exposed for tests.
#[derive(Debug, Clone, Copy)]
pub struct RenderTuning {
    /// Rolling mode keeps the last `|k| + 1` frames in memory if they fit in this budget;
    /// otherwise a second decoder streams the reference frames.
    pub ring_budget_bytes: usize,
}

impl Default for RenderTuning {
    fn default() -> Self {
        Self {
            ring_budget_bytes: 512 << 20,
        }
    }
}

pub fn plan(ff: &Ffmpeg, info: &VideoInfo, spec: &RenderSpec) -> Result<RenderPlan, RenderError> {
    spec.comparison.validate()?;
    spec.effect.validate()?;
    let count = info.frame_count;
    let start = spec.options.start.unwrap_or(0);
    let end = spec.options.end.unwrap_or(count).min(count);
    if start >= end {
        return Err(RenderError::InvalidRange { start, end, count });
    }
    let (width, height) = match spec.options.height {
        Some(h) => info.scaled_size(h),
        None => (info.width, info.height),
    };
    let codec = spec.options.encode.codec;
    let audio = (info.has_audio && codec.supports_audio()).then(|| AudioSource {
        path: info.path.clone(),
        start: info.frame_time(start),
        duration: info.frame_time(end - start),
    });
    let target = EncodeTarget {
        width,
        height,
        fps: info.fps,
        output: spec.output.clone(),
        audio,
    };
    let encoder_args = ff.encode_args(&spec.options.encode, &target);
    Ok(RenderPlan {
        start,
        end,
        width,
        height,
        target,
        encoder_args,
    })
}

pub fn render(
    ff: &Ffmpeg,
    info: &VideoInfo,
    spec: &RenderSpec,
    cancel: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<RenderPlan, RenderError> {
    render_with(ff, info, spec, cancel, on_progress, RenderTuning::default())
}

pub fn render_with(
    ff: &Ffmpeg,
    info: &VideoInfo,
    spec: &RenderSpec,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
    tuning: RenderTuning,
) -> Result<RenderPlan, RenderError> {
    let plan = plan(ff, info, spec)?;
    let size = (spec.options.height.is_some()).then_some((plan.width, plan.height));
    let mut source = FrameSource::new(
        ff,
        info,
        &plan,
        &spec.comparison,
        size,
        &spec.effect,
        tuning,
    )?;
    let encoder = ff.encode(&spec.options.encode, &plan.target)?;
    let output = plan.target.output_path(spec.options.encode.codec);

    // Encoding runs on its own thread so ffmpeg encodes while the next frame is processed.
    let (tx, rx) = mpsc::sync_channel::<Frame>(CHANNEL_CAPACITY);
    let writer = std::thread::spawn(move || -> Result<(), MediaError> {
        let mut encoder = encoder;
        for frame in rx {
            encoder.write_frame(&frame)?;
        }
        encoder.finish()
    });

    let started = Instant::now();
    let total = plan.frame_count();
    let result = (|| {
        for t in plan.start..plan.end {
            if cancel.load(Ordering::Relaxed) {
                return Err(RenderError::Cancelled);
            }
            let Some((current, reference)) = source.next(t, &spec.effect)? else {
                // The video had fewer frames than probed.
                tracing::warn!(frame = t, "video ended early");
                break;
            };
            let out = process_prepared(&current, reference.as_deref(), &spec.effect)?;
            if tx.send(out).is_err() {
                // The writer failed; its error is reported below.
                break;
            }
            on_progress(Progress {
                done: t - plan.start + 1,
                total,
                elapsed: started.elapsed(),
            });
        }
        Ok(())
    })();
    drop(tx);
    drop(source);

    let written = writer.join().expect("encoder thread panicked");
    let result = result.and(written.map_err(RenderError::from));
    if result.is_err() && spec.options.encode.codec != Codec::Png {
        let _ = std::fs::remove_file(&output);
    }
    result.map(|()| plan)
}

/// Yields `(current, reference)` frame pairs in order.
enum FrameSource {
    Fixed {
        frames: FrameReceiver,
        reference: Option<Arc<Prepared>>,
    },
    /// One decoder; recent frames are kept so the reference can be looked up.
    Ring {
        frames: FrameReceiver,
        buffer: VecDeque<(u64, Arc<Prepared>)>,
        next_index: u64,
        k: i64,
        count: u64,
    },
    /// Two decoders: current frames and reference frames stream in lockstep.
    Dual {
        frames: FrameReceiver,
        references: FrameReceiver,
        next_reference: u64,
        k: i64,
        count: u64,
    },
}

impl FrameSource {
    fn new(
        ff: &Ffmpeg,
        info: &VideoInfo,
        plan: &RenderPlan,
        comparison: &Comparison,
        size: Option<(u32, u32)>,
        effect: &EffectParams,
        tuning: RenderTuning,
    ) -> Result<Self, RenderError> {
        let count = info.frame_count;
        let decoder = |start: u64, n: u64| -> Result<FrameReceiver, RenderError> {
            let opts = DecodeOptions {
                start_frame: start,
                frame_count: Some(n),
                size,
                hwaccel: false,
            };
            Ok(ff.decode(info, &opts)?.into_threaded(CHANNEL_CAPACITY))
        };
        match *comparison {
            Comparison::Fixed { frame } => {
                let reference = if frame < count {
                    decoder(frame, 1)?
                        .recv()?
                        .map(|f| Arc::new(Prepared::new(f, effect)))
                } else {
                    None
                };
                Ok(FrameSource::Fixed {
                    frames: decoder(plan.start, plan.frame_count())?,
                    reference,
                })
            }
            Comparison::Rolling { offset } => {
                let k = offset.to_frames(info.fps_f64());
                let frame_bytes = Frame::byte_len(plan.width, plan.height);
                let ring_bytes = (k.unsigned_abs() as usize + 1).saturating_mul(frame_bytes);
                if ring_bytes <= tuning.ring_budget_bytes {
                    // Decode the union of current and reference ranges.
                    let first = plan.start.saturating_sub(k.max(0) as u64);
                    let last = (plan.end + (-k).max(0) as u64).min(count);
                    Ok(FrameSource::Ring {
                        frames: decoder(first, last - first)?,
                        buffer: VecDeque::new(),
                        next_index: first,
                        k,
                        count,
                    })
                } else {
                    let first_ref = (plan.start as i64 - k).clamp(0, count as i64) as u64;
                    let last_ref = (plan.end as i64 - k).clamp(0, count as i64) as u64;
                    let references = decoder(first_ref, (last_ref - first_ref).max(1))?;
                    Ok(FrameSource::Dual {
                        frames: decoder(plan.start, plan.frame_count())?,
                        references,
                        next_reference: first_ref,
                        k,
                        count,
                    })
                }
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn next(
        &mut self,
        t: u64,
        effect: &EffectParams,
    ) -> Result<Option<(Arc<Prepared>, Option<Arc<Prepared>>)>, RenderError> {
        match self {
            FrameSource::Fixed { frames, reference } => Ok(frames
                .recv()?
                .map(|f| (Arc::new(Prepared::new(f, effect)), reference.clone()))),
            FrameSource::Ring {
                frames,
                buffer,
                next_index,
                k,
                count,
            } => {
                let r = reference_index(t, *k, *count);
                let needed = r.map_or(t, |r| r.max(t));
                while *next_index <= needed {
                    match frames.recv()? {
                        Some(f) => {
                            buffer.push_back((*next_index, Arc::new(Prepared::new(f, effect))))
                        }
                        None => break,
                    }
                    *next_index += 1;
                }
                let find = |i: u64| buffer.iter().find(|(j, _)| *j == i).map(|(_, f)| f.clone());
                let Some(current) = find(t) else {
                    return Ok(None);
                };
                let reference = r.and_then(find);
                // Frames below this index are not needed by any later t.
                let keep_from = (t + 1).saturating_sub((*k).max(0) as u64);
                while buffer.front().is_some_and(|(i, _)| *i < keep_from) {
                    buffer.pop_front();
                }
                Ok(Some((current, reference)))
            }
            FrameSource::Dual {
                frames,
                references,
                next_reference,
                k,
                count,
            } => {
                let Some(current) = frames.recv()? else {
                    return Ok(None);
                };
                let reference = match reference_index(t, *k, *count) {
                    Some(r) => {
                        debug_assert_eq!(r, *next_reference, "reference stream out of step");
                        *next_reference += 1;
                        references
                            .recv()?
                            .map(|f| Arc::new(Prepared::new(f, effect)))
                    }
                    None => None,
                };
                Ok(Some((Arc::new(Prepared::new(current, effect)), reference)))
            }
        }
    }
}

fn reference_index(t: u64, k: i64, count: u64) -> Option<u64> {
    let r = t as i64 - k;
    (r >= 0 && (r as u64) < count).then_some(r as u64)
}

/// Whether writing to `path` would replace existing data: an existing file or, for a `%` pattern
/// (image sequence), a non-empty directory.
pub fn output_occupied(path: &std::path::Path) -> bool {
    if path.to_string_lossy().contains('%') {
        path.parent()
            .and_then(|d| std::fs::read_dir(d).ok())
            .is_some_and(|mut entries| entries.next().is_some())
    } else {
        path.exists()
    }
}
