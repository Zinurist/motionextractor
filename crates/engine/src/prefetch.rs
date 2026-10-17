//! Background decoding of preview frames into the [`FrameCache`].
//!
//! Sequential decoding is fast and seeking is slow, so workers decode *runs*: one ffmpeg process
//! starts at a missing frame and streams forward until it reaches a cached frame, leaves the wanted
//! window, or is preempted because the playhead moved elsewhere.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use mx_media::{DecodeOptions, Ffmpeg, VideoInfo};

use crate::cache::{FrameCache, Insert};

/// Frames ahead of a run's position that other workers leave to it.
const CLAIM_AHEAD: u64 = 48;
/// Upper bound for one run (frames), so long videos are revisited in priority order.
const MAX_RUN: u64 = 1800;
/// Decoder failures at the same frame before it is given up.
const MAX_ATTEMPTS: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(250);

/// The frames the prefetcher keeps cached around playhead `t`, as `[start, end)`.
///
/// The window holds `capacity − reserved` frames, two thirds of them ahead of the playhead
/// (matching the cache's eviction metric), shifted inwards at the ends of the video.
pub(crate) fn window(t: u64, count: u64, capacity: u64, reserved: u64) -> (u64, u64) {
    let avail = capacity.saturating_sub(reserved).max(1);
    if avail >= count {
        return (0, count);
    }
    let back = (avail - 1) / 3;
    let fwd = avail - 1 - back;
    let mut start = t as i64 - back as i64;
    let mut end = t as i64 + fwd as i64 + 1;
    if start < 0 {
        end -= start;
        start = 0;
    }
    if end > count as i64 {
        start -= end - count as i64;
        end = count as i64;
    }
    (start.max(0) as u64, end as u64)
}

/// The first frame to decode next: the current and reference frames, then forward from the
/// playhead to the window end, then backward to the window start.
pub(crate) fn next_missing(
    t: u64,
    r: Option<u64>,
    (start, end): (u64, u64),
    skip: impl Fn(u64) -> bool,
) -> Option<u64> {
    std::iter::once(t)
        .chain(r)
        .chain(t.max(start)..end)
        .chain((start..t.min(end)).rev())
        .find(|&i| !skip(i))
}

/// What the prefetcher reports to its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchEvent {
    /// A frame was added to the cache.
    Inserted,
    /// A decoder run finished.
    RunEnded,
    /// The decoded stream ended at this frame count, before the probed count: the video is
    /// shorter than ffprobe estimated.
    StreamEnded(u64),
}

#[derive(Debug, Clone, Copy)]
struct Run {
    next: u64,
    end: u64,
    preempt: bool,
}

struct State {
    t: u64,
    r: Option<u64>,
    runs: Vec<Option<Run>>,
    /// Frames that could not be decoded (e.g. past the real end of the video).
    failed: HashSet<u64>,
    /// Frame count: the probed one, lowered if the stream turns out to end earlier.
    count: u64,
    /// Decoder failures per frame, for retries.
    errors: HashMap<u64, u32>,
    /// Incremented on every focus change.
    focus_version: u64,
    /// Set when the cache rejected a frame: nothing more fits until the focus changes.
    stalled_at: Option<u64>,
    shutdown: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    cache: Arc<FrameCache>,
    ff: Ffmpeg,
    info: VideoInfo,
    size: (u32, u32),
    hwaccel: bool,
    generation: u64,
    on_event: Arc<dyn Fn(PrefetchEvent) + Send + Sync>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("prefetch lock poisoned")
    }

    fn window(&self, s: &State) -> (u64, u64) {
        let reserved = u64::from(s.r.is_some());
        window(s.t, s.count, self.cache.capacity(), reserved)
    }

    fn claimed_by_other(&self, s: &State, me: usize, i: u64) -> bool {
        s.runs.iter().enumerate().any(|(w, run)| {
            w != me && run.is_some_and(|r| i >= r.next && i < r.end.min(r.next + CLAIM_AHEAD))
        })
    }

    fn distance(s: &State, i: u64) -> u64 {
        if i >= s.t { i - s.t } else { (s.t - i) * 2 }
    }
}

/// Owns the decoder worker threads for one opened video.
pub struct Prefetcher {
    shared: Arc<Shared>,
}

impl Prefetcher {
    /// Starts `workers` decoder threads; `on_event` receives their progress.
    pub fn new(
        ff: Ffmpeg,
        info: VideoInfo,
        size: (u32, u32),
        cache: Arc<FrameCache>,
        workers: usize,
        hwaccel: bool,
        on_event: Arc<dyn Fn(PrefetchEvent) + Send + Sync>,
    ) -> Self {
        let workers = workers.max(1);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                t: 0,
                r: None,
                runs: vec![None; workers],
                failed: HashSet::new(),
                errors: HashMap::new(),
                count: info.frame_count,
                focus_version: 0,
                stalled_at: None,
                shutdown: false,
            }),
            wake: Condvar::new(),
            generation: cache.generation(),
            cache,
            ff,
            info,
            size,
            hwaccel,
            on_event,
        });
        for w in 0..workers {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name(format!("prefetch-{w}"))
                .spawn(move || worker(&shared, w))
                .expect("spawn prefetch worker");
        }
        Self { shared }
    }

    /// Moves the playhead to `t` (with reference frame `r`) and reprioritises decoding.
    pub fn set_focus(&self, t: u64, r: Option<u64>) {
        let sh = &self.shared;
        let mut s = sh.lock();
        if s.t == t && s.r == r {
            return;
        }
        s.t = t;
        s.r = r;
        s.focus_version += 1;
        let (start, end) = sh.window(&s);
        // Runs that left the window are pointless now.
        for run in s.runs.iter_mut().flatten() {
            if !(start..end).contains(&run.next) && Some(run.next) != r {
                run.preempt = true;
            }
        }
        // If the frames needed right now are not being decoded and no worker is free, stop the
        // least useful run.
        let needed: Vec<u64> = std::iter::once(t).chain(r).collect();
        let unserved = needed.iter().any(|&i| {
            !sh.cache.contains(i)
                && !s.failed.contains(&i)
                && !s
                    .runs
                    .iter()
                    .flatten()
                    .any(|run| i >= run.next && i < run.end.min(run.next + 4))
        });
        let all_busy = s.runs.iter().all(|run| run.is_some_and(|r| !r.preempt));
        if unserved && all_busy {
            let state = &*s;
            let worst = state
                .runs
                .iter()
                .enumerate()
                .filter_map(|(w, run)| run.map(|r| (w, Shared::distance(state, r.next))))
                .max_by_key(|&(_, d)| d)
                .map(|(w, _)| w);
            if let Some(w) = worst {
                s.runs[w].as_mut().expect("busy").preempt = true;
            }
        }
        drop(s);
        sh.wake.notify_all();
    }

    /// Whether any worker is currently decoding.
    pub fn is_busy(&self) -> bool {
        self.shared.lock().runs.iter().any(Option::is_some)
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        self.shared.lock().shutdown = true;
        self.shared.wake.notify_all();
    }
}

fn worker(sh: &Shared, me: usize) {
    loop {
        let (start, len) = {
            let mut s = sh.lock();
            loop {
                if s.shutdown {
                    return;
                }
                let win = sh.window(&s);
                let skip = |i: u64| {
                    sh.cache.contains(i) || s.failed.contains(&i) || sh.claimed_by_other(&s, me, i)
                };
                let stalled = s.stalled_at == Some(s.focus_version);
                if let Some(start) = next_missing(s.t, s.r, win, skip).filter(|_| !stalled) {
                    // Runs stay inside the window (the reference frame may lie outside it).
                    let mut len = 1;
                    while len < MAX_RUN {
                        let i = start + len;
                        if !(win.0..win.1).contains(&i) || skip(i) {
                            break;
                        }
                        len += 1;
                    }
                    s.runs[me] = Some(Run {
                        next: start,
                        end: start + len,
                        preempt: false,
                    });
                    break (start, len);
                }
                s = sh
                    .wake
                    .wait_timeout(s, Duration::from_millis(500))
                    .expect("prefetch lock poisoned")
                    .0;
            }
        };
        decode_run(sh, me, start, len);
        sh.lock().runs[me] = None;
        sh.wake.notify_all();
        (sh.on_event)(PrefetchEvent::RunEnded);
    }
}

fn decode_run(sh: &Shared, me: usize, start: u64, len: u64) {
    let opts = DecodeOptions {
        start_frame: start,
        frame_count: Some(len),
        size: Some(sh.size),
        hwaccel: sh.hwaccel,
    };
    tracing::debug!(worker = me, start, len, "decode run");
    let mut decoder = match sh.ff.decode(&sh.info, &opts) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("cannot start decoder: {e}");
            decode_error(sh, start, start + len);
            return;
        }
    };
    let mut i = start;
    while i < start + len {
        {
            let s = sh.lock();
            if s.shutdown || s.runs[me].is_none_or(|r| r.preempt) {
                return;
            }
        }
        let frame = match decoder.read_frame() {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                tracing::warn!("decoding failed at frame {i}: {e}");
                decode_error(sh, i, start + len);
                return;
            }
        };
        match sh.cache.insert(sh.generation, i, Arc::new(frame)) {
            Insert::Inserted => (sh.on_event)(PrefetchEvent::Inserted),
            Insert::Rejected => {
                let mut s = sh.lock();
                s.stalled_at = Some(s.focus_version);
                return;
            }
            Insert::Stale => return,
        }
        i += 1;
        let mut s = sh.lock();
        s.errors.remove(&start);
        if let Some(run) = s.runs[me].as_mut() {
            run.next = i;
        }
        // Another worker (or an earlier run) already has the next frame.
        if sh.cache.contains(i) {
            return;
        }
    }
    if i < start + len {
        // The stream ended early: these frames do not exist.
        let mut s = sh.lock();
        s.count = s.count.min(i);
        s.failed.extend(i..start + len);
        drop(s);
        (sh.on_event)(PrefetchEvent::StreamEnded(i));
    }
}

fn mark_failed(sh: &Shared, range: std::ops::Range<u64>) {
    sh.lock().failed.extend(range);
}

/// A decoder failed at frame `at` (e.g. ffmpeg could not start). Such errors may be transient, so
/// the frames are retried after a pause and only given up after `MAX_ATTEMPTS` failures.
fn decode_error(sh: &Shared, at: u64, end: u64) {
    let give_up = {
        let mut s = sh.lock();
        let attempts = s.errors.entry(at).or_insert(0);
        *attempts += 1;
        *attempts >= MAX_ATTEMPTS
    };
    if give_up {
        mark_failed(sh, at..end);
    } else {
        std::thread::sleep(RETRY_DELAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_fits_everything() {
        assert_eq!(window(50, 100, 200, 1), (0, 100));
        assert_eq!(window(50, 100, 101, 1), (0, 100));
    }

    #[test]
    fn window_around_playhead() {
        // 31 frames available: 10 behind, 20 ahead.
        assert_eq!(window(100, 1000, 31, 0), (90, 121));
        // Clipped at the start: extends forward.
        assert_eq!(window(3, 1000, 31, 0), (0, 31));
        // Clipped at the end: extends backward.
        assert_eq!(window(995, 1000, 31, 0), (969, 1000));
        assert_eq!(window(0, 1000, 1, 5), (0, 1));
    }

    #[test]
    fn priority_order() {
        let order: Vec<u64> = (0..)
            .scan(HashSet::new(), |done, _| {
                let next = next_missing(5, Some(1), (2, 9), |i| done.contains(&i))?;
                done.insert(next);
                Some(next)
            })
            .collect();
        assert_eq!(order, vec![5, 1, 6, 7, 8, 4, 3, 2]);
    }
}
