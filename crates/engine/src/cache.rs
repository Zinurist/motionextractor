//! Memory-bounded cache of decoded preview frames.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use mx_core::Frame;

/// Always keep room for at least this many frames, whatever the budget (current + reference + one).
const MIN_CAPACITY: u64 = 3;

/// Decoded source frames keyed by frame index.
///
/// Eviction is distance based: the frame furthest from the playhead goes first, where frames behind
/// the playhead count double (playback and scrubbing usually move forward). Pinned frames (current,
/// reference) are never evicted.
pub struct FrameCache {
    inner: Mutex<Inner>,
    changed: Condvar,
}

struct Inner {
    generation: u64,
    frames: BTreeMap<u64, Arc<Frame>>,
    frame_bytes: usize,
    budget: usize,
    playhead: u64,
    pinned: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insert {
    Inserted,
    /// The frame would be the first to be evicted; it was not stored.
    Rejected,
    /// The cache was reset since the frame was requested.
    Stale,
}

impl Inner {
    fn capacity(&self) -> u64 {
        if self.frame_bytes == 0 {
            return MIN_CAPACITY;
        }
        ((self.budget / self.frame_bytes) as u64).max(MIN_CAPACITY)
    }

    fn distance(&self, i: u64) -> u64 {
        if self.pinned.contains(&i) {
            return 0;
        }
        if i >= self.playhead {
            i - self.playhead
        } else {
            (self.playhead - i) * 2
        }
    }

    /// The unpinned cached frame with the largest distance. Distance grows monotonically away from
    /// the playhead, so only the ends of the ordered map need to be inspected.
    fn victim(&self) -> Option<(u64, u64)> {
        let first = self.frames.keys().find(|i| !self.pinned.contains(i));
        let last = self.frames.keys().rev().find(|i| !self.pinned.contains(i));
        [first, last]
            .into_iter()
            .flatten()
            .map(|&i| (i, self.distance(i)))
            .max_by_key(|&(_, d)| d)
    }

    fn evict_to_capacity(&mut self) {
        while self.frames.len() as u64 > self.capacity() {
            match self.victim() {
                Some((i, _)) => {
                    self.frames.remove(&i);
                }
                None => break,
            }
        }
    }
}

impl FrameCache {
    pub fn new(budget: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                generation: 0,
                frames: BTreeMap::new(),
                frame_bytes: 0,
                budget,
                playhead: 0,
                pinned: Vec::new(),
            }),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("cache lock poisoned")
    }

    /// Clears the cache for a new video or preview size; returns the new generation.
    pub fn reset(&self, frame_bytes: usize) -> u64 {
        let mut c = self.lock();
        c.generation += 1;
        c.frames.clear();
        c.frame_bytes = frame_bytes;
        c.playhead = 0;
        c.pinned.clear();
        self.changed.notify_all();
        c.generation
    }

    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    pub fn set_budget(&self, budget: usize) {
        let mut c = self.lock();
        c.budget = budget;
        c.evict_to_capacity();
    }

    /// Maximum number of frames the cache holds.
    pub fn capacity(&self) -> u64 {
        self.lock().capacity()
    }

    pub fn set_focus(&self, playhead: u64, pinned: &[u64]) {
        let mut c = self.lock();
        c.playhead = playhead;
        c.pinned = pinned.to_vec();
    }

    pub fn get(&self, i: u64) -> Option<Arc<Frame>> {
        self.lock().frames.get(&i).cloned()
    }

    pub fn contains(&self, i: u64) -> bool {
        self.lock().frames.contains_key(&i)
    }

    pub fn len(&self) -> usize {
        self.lock().frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn insert(&self, generation: u64, i: u64, frame: Arc<Frame>) -> Insert {
        let mut c = self.lock();
        if generation != c.generation {
            return Insert::Stale;
        }
        if c.frames.contains_key(&i) {
            return Insert::Inserted;
        }
        if c.frames.len() as u64 >= c.capacity() {
            match c.victim() {
                Some((v, d)) if d > c.distance(i) => {
                    c.frames.remove(&v);
                }
                _ => return Insert::Rejected,
            }
        }
        c.frames.insert(i, frame);
        self.changed.notify_all();
        Insert::Inserted
    }

    /// The cached frame closest to `i` (by index), if any.
    pub fn nearest(&self, i: u64) -> Option<(u64, Arc<Frame>)> {
        let c = self.lock();
        let after = c.frames.range(i..).next();
        let before = c.frames.range(..i).next_back();
        match (before, after) {
            (Some((b, fb)), Some((a, fa))) => Some(if i - b <= a - i {
                (*b, fb.clone())
            } else {
                (*a, fa.clone())
            }),
            (Some((j, f)), None) | (None, Some((j, f))) => Some((*j, f.clone())),
            (None, None) => None,
        }
    }

    /// Waits until all `indices` are cached. Returns them, or `None` on timeout, when `abort`
    /// returns true, or when the cache is reset.
    pub fn wait_for(
        &self,
        indices: &[u64],
        timeout: Duration,
        abort: &dyn Fn() -> bool,
    ) -> Option<Vec<Arc<Frame>>> {
        let deadline = Instant::now() + timeout;
        let mut c = self.lock();
        let generation = c.generation;
        loop {
            let found: Option<Vec<_>> = indices.iter().map(|i| c.frames.get(i).cloned()).collect();
            if found.is_some() {
                return found;
            }
            let now = Instant::now();
            if now >= deadline || abort() || c.generation != generation {
                return None;
            }
            // Wake up regularly to check `abort`.
            let slice = (deadline - now).min(Duration::from_millis(20));
            c = self
                .changed
                .wait_timeout(c, slice)
                .expect("cache lock poisoned")
                .0;
        }
    }

    /// Cached frames as sorted half-open ranges `[start, end)`.
    pub fn ranges(&self) -> Vec<(u64, u64)> {
        let c = self.lock();
        let mut out: Vec<(u64, u64)> = Vec::new();
        for &i in c.frames.keys() {
            match out.last_mut() {
                Some((_, end)) if *end == i => *end = i + 1,
                _ => out.push((i, i + 1)),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Arc<Frame> {
        Arc::new(Frame::black(2, 2))
    }

    /// A cache holding exactly `n` 2×2 frames.
    fn cache(n: usize) -> (FrameCache, u64) {
        let c = FrameCache::new(n * 12);
        let g = c.reset(12);
        (c, g)
    }

    #[test]
    fn capacity_and_minimum() {
        let (c, _) = cache(10);
        assert_eq!(c.capacity(), 10);
        c.set_budget(0);
        assert_eq!(c.capacity(), MIN_CAPACITY);
    }

    #[test]
    fn evicts_furthest_from_playhead() {
        let (c, g) = cache(4);
        c.set_focus(10, &[]);
        for i in [8, 10, 11, 12] {
            assert_eq!(c.insert(g, i, frame()), Insert::Inserted);
        }
        // 13 is closer (3) than 8 (2 behind = distance 4): 8 goes.
        assert_eq!(c.insert(g, 13, frame()), Insert::Inserted);
        assert!(!c.contains(8));
        // 20 is further than everything: rejected.
        assert_eq!(c.insert(g, 20, frame()), Insert::Rejected);
        assert_eq!(c.ranges(), vec![(10, 14)]);
    }

    #[test]
    fn pinned_frames_survive() {
        let (c, g) = cache(3);
        c.set_focus(50, &[0]);
        for i in [0, 50, 51] {
            c.insert(g, i, frame());
        }
        assert_eq!(c.insert(g, 52, frame()), Insert::Rejected);
        c.set_focus(50, &[]);
        assert_eq!(c.insert(g, 52, frame()), Insert::Inserted);
        assert!(!c.contains(0));
    }

    #[test]
    fn budget_shrink_evicts() {
        let (c, g) = cache(6);
        c.set_focus(3, &[]);
        for i in 0..6 {
            c.insert(g, i, frame());
        }
        c.set_budget(4 * 12);
        assert_eq!(c.ranges(), vec![(2, 6)]);
    }

    #[test]
    fn stale_generation_and_reset() {
        let (c, g) = cache(4);
        c.insert(g, 1, frame());
        let g2 = c.reset(12);
        assert!(c.is_empty());
        assert_eq!(c.insert(g, 2, frame()), Insert::Stale);
        assert_eq!(c.insert(g2, 2, frame()), Insert::Inserted);
    }

    #[test]
    fn ranges_and_nearest() {
        let (c, g) = cache(10);
        for i in [1, 2, 3, 7, 9] {
            c.insert(g, i, frame());
        }
        assert_eq!(c.ranges(), vec![(1, 4), (7, 8), (9, 10)]);
        assert_eq!(c.nearest(5).unwrap().0, 3);
        assert_eq!(c.nearest(6).unwrap().0, 7);
        assert_eq!(c.nearest(100).unwrap().0, 9);
        assert_eq!(c.nearest(0).unwrap().0, 1);
    }

    #[test]
    fn wait_for_frames() {
        let (c, g) = cache(10);
        let c = Arc::new(c);
        let c2 = c.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            c2.insert(g, 5, frame());
            c2.insert(g, 6, frame());
        });
        let got = c.wait_for(&[5, 6], Duration::from_secs(5), &|| false);
        assert_eq!(got.map(|v| v.len()), Some(2));
        t.join().unwrap();
        assert!(
            c.wait_for(&[99], Duration::from_millis(30), &|| false)
                .is_none()
        );
        assert!(
            c.wait_for(&[99], Duration::from_secs(5), &|| true)
                .is_none()
        );
    }
}
