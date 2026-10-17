//! Session (preview engine + render jobs) tests against real ffmpeg.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mx_core::{Comparison, Offset, Rgb};
use mx_engine::{Event, FrameCache, JobState, Preset, Session, SessionError, Settings, View};
use mx_media::testing::{MovingSquare, ffmpeg};

const WAIT: Duration = Duration::from_secs(20);

struct Fixture {
    session: Arc<Session>,
    events: Arc<Mutex<Vec<Event>>>,
    dir: tempfile::TempDir,
    spec: MovingSquare,
}

fn fixture(spec: MovingSquare) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = spec.generate(&ffmpeg(), dir.path());
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let session = Session::new(
        ffmpeg(),
        Some(dir.path().join("settings.json")),
        Arc::new(move |e| sink.lock().unwrap().push(e)),
    )
    .unwrap();
    session.open(&path).unwrap();
    Fixture {
        session,
        events,
        dir,
        spec,
    }
}

fn wait_until(what: &str, f: impl Fn() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < WAIT, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn view(s: &Session, t: u64, v: View) -> mx_engine::ViewFrame {
    s.view(t, v, WAIT, &|| false)
        .unwrap()
        .expect("frame available")
}

#[test]
fn opens_and_fills_cache() {
    let f = fixture(MovingSquare::default());
    let video = f.session.video().unwrap();
    assert_eq!(video.info.frame_count, 30);
    // Preview height 540 exceeds the native 120: native size is used.
    assert_eq!((video.preview_width, video.preview_height), (160, 120));
    wait_until("full cache", || {
        f.session.cache_state().ranges == vec![(0, 30)]
    });
    wait_until("decoding finished", || !f.session.cache_state().decoding);
    let events = f.events.lock().unwrap();
    assert!(matches!(events[0], Event::Opened { .. }));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Cache { cache } if cache.cached == 30))
    );
}

#[test]
fn output_and_original_views() {
    let f = fixture(MovingSquare::default());
    let sq = &f.spec;
    let y = sq.y + sq.size / 2;
    let out = view(&f.session, 10, View::Output);
    assert!(out.exact);
    assert_eq!(out.index, 10);
    // Rolling offset 1: trailing and leading edges lit, the overlap dark.
    assert_eq!(out.frame.pixel(sq.x(9), y), [255; 3]);
    assert_eq!(out.frame.pixel(sq.x(10) + sq.size - 1, y), [255; 3]);
    assert_eq!(out.frame.pixel(sq.x(10) + 4, y), [0; 3]);
    let orig = view(&f.session, 10, View::Original);
    assert_eq!(orig.frame.pixel(sq.x(10) + 4, y), [255; 3]);
    // Out-of-range requests clamp to the last frame.
    assert_eq!(view(&f.session, 999, View::Output).index, 29);
    // Frame 0 has no reference: black.
    assert!(
        view(&f.session, 0, View::Output)
            .frame
            .data()
            .iter()
            .all(|&v| v == 0)
    );
}

#[test]
fn parameter_changes_apply_immediately() {
    let f = fixture(MovingSquare::default());
    let sq = &f.spec;
    let y = sq.y + sq.size / 2;
    let before = view(&f.session, 10, View::Output);
    let mut preset = Preset::default();
    preset.effect.color = Rgb([255, 0, 0]);
    preset.comparison = Comparison::Fixed { frame: 0 };
    let rev = f.session.set_preset(preset.clone()).unwrap();
    assert!(rev > before.rev);
    let after = view(&f.session, 10, View::Output);
    assert_eq!(after.rev, rev);
    assert_eq!(after.frame.pixel(sq.x(0) + 4, y), [255, 0, 0]);
    assert_eq!(after.frame.pixel(sq.x(10) + 4, y), [255, 0, 0]);
    // Render-only changes keep the revision.
    preset.render.height = Some(60);
    assert_eq!(f.session.set_preset(preset).unwrap(), rev);
    let bad = Preset {
        comparison: Comparison::Rolling {
            offset: Offset::frames(0),
        },
        ..Default::default()
    };
    assert!(f.session.set_preset(bad).is_err());
    assert!(
        f.events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Preset { rev: r, .. } if *r == rev))
    );
}

#[test]
fn approximate_and_full_frames() {
    let f = fixture(MovingSquare::default());
    wait_until("frame 5", || view(&f.session, 5, View::Output).exact);
    let approx = f
        .session
        .approximate_view(7, View::Original)
        .unwrap()
        .unwrap();
    assert!(!approx.exact);
    let full = f.session.full_frame(12, View::Output).unwrap();
    assert_eq!(full, *view(&f.session, 12, View::Output).frame);
    let full_orig = f.session.full_frame(12, View::Original).unwrap();
    assert_eq!(full_orig, *view(&f.session, 12, View::Original).frame);
}

#[test]
fn settings_persist_and_reopen() {
    let f = fixture(MovingSquare {
        width: 320,
        height: 240,
        ..Default::default()
    });
    let s = Settings {
        preview_height: 120,
        ..Default::default()
    };
    f.session.set_settings(s.clone()).unwrap();
    let video = f.session.video().unwrap();
    assert_eq!((video.preview_width, video.preview_height), (160, 120));
    assert_eq!(view(&f.session, 3, View::Original).frame.width(), 160);
    assert_eq!(
        Settings::load(&f.dir.path().join("settings.json")).unwrap(),
        s
    );
    assert!(
        f.session
            .set_settings(Settings {
                decode_workers: 0,
                ..s
            })
            .is_err()
    );
}

#[test]
fn render_jobs() {
    let f = fixture(MovingSquare::default());
    let out = f.dir.path().join("out.mp4");
    let cmd = f.session.render_command(out.clone()).unwrap();
    assert!(cmd.contains("libx264"), "{cmd}");
    let job = f.session.start_render(out.clone(), false).unwrap();
    assert_eq!(job.state, JobState::Running);
    wait_until("render finished", || {
        f.session
            .render_status()
            .is_some_and(|s| s.state != JobState::Running)
    });
    let status = f.session.render_status().unwrap();
    assert_eq!(status.state, JobState::Finished, "{status:?}");
    assert_eq!(status.done, 30);
    assert_eq!(ffmpeg().probe(&out).unwrap().frame_count, 30);
    assert!(matches!(
        f.session.start_render(out.clone(), false),
        Err(SessionError::Exists(_))
    ));
    assert!(
        f.events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Render { job } if job.state == JobState::Finished))
    );
    assert!(!f.session.cancel_render());

    // Cancel a longer render.
    let mut preset = Preset::default();
    preset.effect.bloom.enabled = true;
    preset.render.encode.encoder_preset = Some("veryslow".into());
    f.session.set_preset(preset).unwrap();
    let long = fixture(MovingSquare {
        frames: 300,
        width: 640,
        height: 480,
        ..Default::default()
    });
    long.session.set_preset(f.session.preset().0).unwrap();
    long.session.start_render(out.clone(), true).unwrap();
    assert!(long.session.cancel_render());
    wait_until("render cancelled", || {
        long.session
            .render_status()
            .is_some_and(|s| s.state != JobState::Running)
    });
    assert_eq!(
        long.session.render_status().unwrap().state,
        JobState::Cancelled
    );
}

#[test]
fn no_video_errors() {
    let session = Session::new(ffmpeg(), None, Arc::new(|_| {})).unwrap();
    assert!(matches!(
        session.view(0, View::Output, WAIT, &|| false),
        Err(SessionError::NoVideo)
    ));
    assert!(matches!(
        session.start_render("x.mp4".into(), false),
        Err(SessionError::NoVideo)
    ));
    assert!(
        session
            .open(std::path::Path::new("/nonexistent.mp4"))
            .is_err()
    );
}

#[test]
fn prefetcher_keeps_window_around_playhead() {
    // A cache for ~10 frames on a 60 frame video.
    let dir = tempfile::tempdir().unwrap();
    let ff = ffmpeg();
    let info = ff
        .probe(
            &MovingSquare {
                frames: 60,
                ..Default::default()
            }
            .generate(&ff, dir.path()),
        )
        .unwrap();
    let frame_bytes = mx_core::Frame::byte_len(info.width, info.height);
    let cache = Arc::new(FrameCache::new(frame_bytes * 10));
    cache.reset(frame_bytes);
    let prefetch = mx_engine::Prefetcher::new(
        ff,
        info.clone(),
        (info.width, info.height),
        cache.clone(),
        2,
        false,
        Arc::new(|_| {}),
    );
    cache.set_focus(40, &[40]);
    prefetch.set_focus(40, None);
    wait_until("window decoded", || {
        cache.len() == 10 && !prefetch.is_busy()
    });
    // 3 behind, 6 ahead (+ the playhead).
    assert_eq!(cache.ranges(), vec![(37, 47)]);
    cache.set_focus(5, &[5, 0]);
    prefetch.set_focus(5, Some(0));
    wait_until("moved window", || {
        cache.contains(5) && cache.contains(0) && cache.contains(10)
    });
    wait_until("idle", || !prefetch.is_busy());
    assert!(
        cache.ranges().iter().all(|&(_, e)| e <= 13),
        "{:?}",
        cache.ranges()
    );
}
