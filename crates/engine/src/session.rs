//! The state of one running app instance: the opened video, effect parameters, the preview cache
//! and the render job.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use mx_core::{
    Comparison, EffectError, EffectParams, Frame, ParamError, Prepared, process_prepared,
};
use mx_media::{DecodeOptions, Ffmpeg, MediaError, VideoInfo, command_line};
use serde::{Deserialize, Serialize};

use crate::cache::FrameCache;
use crate::lru::Lru;
use crate::prefetch::Prefetcher;
use crate::render::{RenderError, RenderSpec, output_occupied};
use crate::settings::{Settings, SettingsError};
use crate::{Preset, plan, render};

const CACHE_EVENT_INTERVAL: Duration = Duration::from_millis(200);
const RENDER_EVENT_INTERVAL: Duration = Duration::from_millis(250);

/// Which image of a frame is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    Output,
    Original,
}

/// A preview image ready to be shown.
#[derive(Debug, Clone)]
pub struct ViewFrame {
    /// Frame index actually shown (differs from the requested one for approximations).
    pub index: u64,
    pub view: View,
    /// False for a stand-in shown while the requested frame is still being decoded.
    pub exact: bool,
    /// Effect parameter revision the image was computed with.
    pub rev: u64,
    pub frame: Arc<Frame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Finished,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobStatus {
    pub id: u64,
    pub state: JobState,
    pub done: u64,
    pub total: u64,
    pub fps: f64,
    pub eta_seconds: Option<f64>,
    pub elapsed_seconds: f64,
    pub output: PathBuf,
    pub command: String,
    pub error: Option<String>,
}

/// Information about the opened video for clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoState {
    pub info: VideoInfo,
    pub preview_width: u32,
    pub preview_height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheState {
    pub ranges: Vec<(u64, u64)>,
    pub capacity: u64,
    pub cached: u64,
    /// Whether frames are being decoded right now.
    pub decoding: bool,
}

/// Notifications for clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Opened {
        video: VideoState,
    },
    Preset {
        preset: Preset,
        rev: u64,
    },
    Cache {
        cache: CacheState,
    },
    Render {
        job: JobStatus,
    },
    Settings {
        settings: Settings,
    },
    /// A problem not tied to a request (e.g. a file dropped onto the window could not be opened).
    Error {
        message: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("no video opened")]
    NoVideo,
    #[error("a render is already running")]
    Busy,
    #[error("{0} already exists")]
    Exists(PathBuf),
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Effect(#[from] EffectError),
    #[error(transparent)]
    Param(#[from] ParamError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

struct Video {
    state: VideoState,
    prefetch: Prefetcher,
}

struct State {
    video: Option<Video>,
    preset: Preset,
    /// Incremented whenever comparison or effect parameters change.
    rev: u64,
}

struct Job {
    status: JobStatus,
    cancel: Arc<AtomicBool>,
}

pub struct Session {
    me: Weak<Session>,
    ff: Ffmpeg,
    settings_path: Option<PathBuf>,
    settings: Mutex<Settings>,
    state: Mutex<State>,
    cache: Arc<FrameCache>,
    processed: Mutex<Lru<(u64, View, u64), Arc<Frame>>>,
    prepared: Mutex<Lru<(u64, u32), Arc<Prepared>>>,
    job: Mutex<Option<Job>>,
    next_job_id: AtomicU64,
    last_cache_event: Mutex<Option<Instant>>,
    events: EventSink,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().expect("session lock poisoned")
}

impl Session {
    /// Creates a session. Settings are loaded from (and saved to) `settings_path` if given.
    pub fn new(
        ff: Ffmpeg,
        settings_path: Option<PathBuf>,
        events: EventSink,
    ) -> Result<Arc<Self>, SessionError> {
        let settings = match &settings_path {
            Some(p) => Settings::load(p)?,
            None => Settings::default(),
        };
        Ok(Arc::new_cyclic(|me| Session {
            me: me.clone(),
            ff,
            settings_path,
            cache: Arc::new(FrameCache::new(settings.cache_bytes())),
            settings: Mutex::new(settings),
            state: Mutex::new(State {
                video: None,
                preset: Preset::default(),
                rev: 0,
            }),
            processed: Mutex::new(Lru::new(24)),
            prepared: Mutex::new(Lru::new(8)),
            job: Mutex::new(None),
            next_job_id: AtomicU64::new(1),
            last_cache_event: Mutex::new(None),
            events,
        }))
    }

    pub fn ffmpeg(&self) -> &Ffmpeg {
        &self.ff
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn set_settings(&self, new: Settings) -> Result<(), SessionError> {
        new.validate()?;
        if let Some(p) = &self.settings_path {
            new.save(p)?;
        }
        let old = std::mem::replace(&mut *lock(&self.settings), new.clone());
        self.cache.set_budget(new.cache_bytes());
        let reopen = old.preview_height != new.preview_height
            || old.decode_workers != new.decode_workers
            || old.hwaccel != new.hwaccel;
        (self.events)(Event::Settings { settings: new });
        if reopen {
            let path = lock(&self.state)
                .video
                .as_ref()
                .map(|v| v.state.info.path.clone());
            if let Some(path) = path {
                self.open(&path)?;
            }
        }
        self.emit_cache(true);
        Ok(())
    }

    pub fn open(&self, path: &Path) -> Result<VideoState, SessionError> {
        let info = self.ff.probe(path)?;
        let settings = self.settings();
        let (pw, ph) = match settings.preview_height {
            0 => (info.width, info.height),
            h => info.scaled_size(h),
        };
        let mut state = lock(&self.state);
        // Stop the old workers before resetting the cache.
        state.video = None;
        self.cache.reset(Frame::byte_len(pw, ph));
        lock(&self.processed).clear();
        lock(&self.prepared).clear();
        let me = self.me.clone();
        let prefetch = Prefetcher::new(
            self.ff.clone(),
            info.clone(),
            (pw, ph),
            self.cache.clone(),
            settings.decode_workers as usize,
            settings.hwaccel,
            Arc::new(move |force| {
                if let Some(s) = me.upgrade() {
                    s.emit_cache(force);
                }
            }),
        );
        let video = VideoState {
            info,
            preview_width: pw,
            preview_height: ph,
        };
        state.video = Some(Video {
            state: video.clone(),
            prefetch,
        });
        // Start prefetching at the beginning.
        Self::focus(&state, &self.cache, 0, View::Output);
        drop(state);
        (self.events)(Event::Opened {
            video: video.clone(),
        });
        self.emit_cache(true);
        Ok(video)
    }

    /// Shows an error message in every connected UI.
    pub fn report_error(&self, message: String) {
        tracing::warn!("{message}");
        (self.events)(Event::Error { message });
    }

    pub fn video(&self) -> Option<VideoState> {
        lock(&self.state).video.as_ref().map(|v| v.state.clone())
    }

    pub fn preset(&self) -> (Preset, u64) {
        let s = lock(&self.state);
        (s.preset.clone(), s.rev)
    }

    /// Replaces comparison, effect and render settings; returns the new revision.
    pub fn set_preset(&self, preset: Preset) -> Result<u64, SessionError> {
        preset.comparison.validate()?;
        preset.effect.validate()?;
        let mut s = lock(&self.state);
        if preset.comparison != s.preset.comparison || preset.effect != s.preset.effect {
            s.rev += 1;
        }
        s.preset = preset.clone();
        let rev = s.rev;
        drop(s);
        (self.events)(Event::Preset { preset, rev });
        Ok(rev)
    }

    pub fn cache_state(&self) -> CacheState {
        CacheState {
            ranges: self.cache.ranges(),
            capacity: self.cache.capacity(),
            cached: self.cache.len() as u64,
            decoding: lock(&self.state)
                .video
                .as_ref()
                .is_some_and(|v| v.prefetch.is_busy()),
        }
    }

    fn emit_cache(&self, force: bool) {
        {
            let mut last = lock(&self.last_cache_event);
            if !force && last.is_some_and(|t| t.elapsed() < CACHE_EVENT_INTERVAL) {
                return;
            }
            *last = Some(Instant::now());
        }
        (self.events)(Event::Cache {
            cache: self.cache_state(),
        });
    }

    /// Reference frame index of frame `t` under the current parameters.
    fn reference(preset: &Preset, info: &VideoInfo, t: u64) -> Option<u64> {
        preset
            .comparison
            .reference_index(t, info.fps_f64(), info.frame_count)
    }

    /// Points the cache and the prefetcher at frame `t`.
    fn focus(state: &State, cache: &FrameCache, t: u64, view: View) {
        let Some(video) = &state.video else { return };
        let info = &video.state.info;
        let r = match view {
            View::Output => Self::reference(&state.preset, info, t),
            View::Original => None,
        };
        let mut pins = vec![t];
        pins.extend(r);
        if let Comparison::Fixed { frame } = state.preset.comparison
            && frame < info.frame_count
        {
            pins.push(frame);
        }
        cache.set_focus(t, &pins);
        video.prefetch.set_focus(t, r);
    }

    /// The preview image of frame `t`. Waits up to `wait` (or until `abort` returns true) for the
    /// source frames to be decoded; returns `None` if they are not available in time.
    pub fn view(
        &self,
        t: u64,
        view: View,
        wait: Duration,
        abort: &dyn Fn() -> bool,
    ) -> Result<Option<ViewFrame>, SessionError> {
        let (info, preset, rev) = {
            let s = lock(&self.state);
            let video = s.video.as_ref().ok_or(SessionError::NoVideo)?;
            let t = t.min(video.state.info.frame_count - 1);
            Self::focus(&s, &self.cache, t, view);
            (video.state.info.clone(), s.preset.clone(), s.rev)
        };
        let t = t.min(info.frame_count - 1);
        if let Some(frame) = lock(&self.processed).get(&(t, view, rev)) {
            return Ok(Some(ViewFrame {
                index: t,
                view,
                exact: true,
                rev,
                frame,
            }));
        }
        let r = match view {
            View::Output => Self::reference(&preset, &info, t),
            View::Original => None,
        };
        let wanted: Vec<u64> = std::iter::once(t).chain(r).collect();
        let Some(frames) = self.cache.wait_for(&wanted, wait, abort) else {
            return Ok(None);
        };
        let frame = match view {
            View::Original => frames[0].clone(),
            View::Output => {
                let cur = self.prepare(t, &frames[0], &preset.effect);
                let reference = r.map(|r| self.prepare(r, &frames[1], &preset.effect));
                Arc::new(process_prepared(
                    &cur,
                    reference.as_deref(),
                    &preset.effect,
                )?)
            }
        };
        lock(&self.processed).insert((t, view, rev), frame.clone());
        Ok(Some(ViewFrame {
            index: t,
            view,
            exact: true,
            rev,
            frame,
        }))
    }

    /// A stand-in for frame `t` from the nearest cached frame, shown while `t` is being decoded.
    pub fn approximate_view(&self, t: u64, view: View) -> Result<Option<ViewFrame>, SessionError> {
        let (info, preset, rev) = {
            let s = lock(&self.state);
            let video = s.video.as_ref().ok_or(SessionError::NoVideo)?;
            (video.state.info.clone(), s.preset.clone(), s.rev)
        };
        let Some((index, frame)) = self.cache.nearest(t) else {
            return Ok(None);
        };
        let frame = match view {
            View::Original => frame,
            View::Output => {
                let cur = self.prepare(index, &frame, &preset.effect);
                let reference = Self::reference(&preset, &info, index).and_then(|r| {
                    self.cache
                        .get(r)
                        .map(|f| self.prepare(r, &f, &preset.effect))
                });
                Arc::new(process_prepared(
                    &cur,
                    reference.as_deref(),
                    &preset.effect,
                )?)
            }
        };
        Ok(Some(ViewFrame {
            index,
            view,
            exact: false,
            rev,
            frame,
        }))
    }

    fn prepare(&self, i: u64, frame: &Arc<Frame>, p: &EffectParams) -> Arc<Prepared> {
        let key = (i, p.denoise.to_bits());
        if let Some(prepared) = lock(&self.prepared).get(&key) {
            return prepared;
        }
        let prepared = Arc::new(Prepared::new(Frame::clone(frame), p));
        lock(&self.prepared).insert(key, prepared.clone());
        prepared
    }

    /// Frame `t` at full resolution, decoded directly (not from the preview cache).
    pub fn full_frame(&self, t: u64, view: View) -> Result<Frame, SessionError> {
        let (info, preset) = {
            let s = lock(&self.state);
            let video = s.video.as_ref().ok_or(SessionError::NoVideo)?;
            (video.state.info.clone(), s.preset.clone())
        };
        let t = t.min(info.frame_count - 1);
        let decode = |i: u64| -> Result<Frame, SessionError> {
            let opts = DecodeOptions {
                start_frame: i,
                frame_count: Some(1),
                ..Default::default()
            };
            self.ff
                .decode(&info, &opts)?
                .read_frame()?
                .ok_or_else(|| MediaError::InvalidOption(format!("frame {i} not found")).into())
        };
        let current = decode(t)?;
        if view == View::Original {
            return Ok(current);
        }
        let reference = Self::reference(&preset, &info, t).map(decode).transpose()?;
        let p = &preset.effect;
        let cur = Prepared::new(current, p);
        let reference = reference.map(|r| Prepared::new(r, p));
        Ok(process_prepared(&cur, reference.as_ref(), p)?)
    }

    fn render_spec(&self, output: PathBuf) -> Result<(VideoInfo, RenderSpec), SessionError> {
        let s = lock(&self.state);
        let video = s.video.as_ref().ok_or(SessionError::NoVideo)?;
        Ok((
            video.state.info.clone(),
            RenderSpec {
                output,
                comparison: s.preset.comparison,
                effect: s.preset.effect,
                options: s.preset.render.clone(),
            },
        ))
    }

    /// The ffmpeg encoder command a render to `output` would run.
    pub fn render_command(&self, output: PathBuf) -> Result<String, SessionError> {
        let (info, spec) = self.render_spec(output)?;
        let plan = plan(&self.ff, &info, &spec)?;
        Ok(command_line(&self.ff.ffmpeg, &plan.encoder_args))
    }

    pub fn render_status(&self) -> Option<JobStatus> {
        lock(&self.job).as_ref().map(|j| j.status.clone())
    }

    /// Starts rendering the current video with the current settings in the background.
    pub fn start_render(
        &self,
        output: PathBuf,
        overwrite: bool,
    ) -> Result<JobStatus, SessionError> {
        let (info, spec) = self.render_spec(output)?;
        let plan = plan(&self.ff, &info, &spec)?;
        let target = plan.target.output_path(spec.options.encode.codec);
        if !overwrite && output_occupied(&target) {
            return Err(SessionError::Exists(target));
        }
        let mut job = lock(&self.job);
        if job
            .as_ref()
            .is_some_and(|j| j.status.state == JobState::Running)
        {
            return Err(SessionError::Busy);
        }
        let id = self.next_job_id.fetch_add(1, Ordering::Relaxed);
        let status = JobStatus {
            id,
            state: JobState::Running,
            done: 0,
            total: plan.frame_count(),
            fps: 0.0,
            eta_seconds: None,
            elapsed_seconds: 0.0,
            output: spec.output.clone(),
            command: command_line(&self.ff.ffmpeg, &plan.encoder_args),
            error: None,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        *job = Some(Job {
            status: status.clone(),
            cancel: cancel.clone(),
        });
        drop(job);
        (self.events)(Event::Render {
            job: status.clone(),
        });

        let me = self.me.upgrade().expect("session alive");
        std::thread::Builder::new()
            .name(format!("render-{id}"))
            .spawn(move || me.run_render(id, &info, &spec, &cancel))
            .expect("spawn render thread");
        Ok(status)
    }

    fn run_render(&self, id: u64, info: &VideoInfo, spec: &RenderSpec, cancel: &AtomicBool) {
        let mut last_event = Instant::now();
        let result = render(&self.ff, info, spec, cancel, |p| {
            let status = self.update_job(id, |s| {
                s.done = p.done;
                s.total = p.total;
                s.fps = p.fps();
                s.eta_seconds = p.eta().map(|d| d.as_secs_f64());
                s.elapsed_seconds = p.elapsed.as_secs_f64();
            });
            if last_event.elapsed() >= RENDER_EVENT_INTERVAL
                && let Some(job) = status
            {
                last_event = Instant::now();
                (self.events)(Event::Render { job });
            }
        });
        let status = self.update_job(id, |s| {
            s.eta_seconds = None;
            match &result {
                Ok(_) => {
                    s.state = JobState::Finished;
                    s.done = s.total;
                }
                Err(RenderError::Cancelled) => s.state = JobState::Cancelled,
                Err(e) => {
                    s.state = JobState::Failed;
                    s.error = Some(e.to_string());
                }
            }
        });
        if let Some(job) = status {
            (self.events)(Event::Render { job });
        }
    }

    fn update_job(&self, id: u64, f: impl FnOnce(&mut JobStatus)) -> Option<JobStatus> {
        let mut job = lock(&self.job);
        let job = job.as_mut().filter(|j| j.status.id == id)?;
        f(&mut job.status);
        Some(job.status.clone())
    }

    /// Requests cancellation of the running render; returns whether one was running.
    pub fn cancel_render(&self) -> bool {
        let job = lock(&self.job);
        match job.as_ref() {
            Some(j) if j.status.state == JobState::Running => {
                j.cancel.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }
}
