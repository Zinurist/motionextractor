//! Render pipeline tests against real ffmpeg, using lossless output for exact pixel checks.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use mx_core::{Comparison, EffectParams, Frame, Offset, Rgb};
use mx_engine::{RenderError, RenderOptions, RenderSpec, RenderTuning, render, render_with};
use mx_media::testing::{MovingSquare, ffmpeg};
use mx_media::{Codec, DecodeOptions, EncodeOptions, Ffmpeg, VideoInfo};

struct Fixture {
    ff: Ffmpeg,
    dir: tempfile::TempDir,
    spec: MovingSquare,
    info: VideoInfo,
}

fn fixture(spec: MovingSquare) -> Fixture {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    let info = ff.probe(&spec.generate(&ff, dir.path())).unwrap();
    Fixture {
        ff,
        dir,
        spec,
        info,
    }
}

fn lossless(output: &Path, comparison: Comparison) -> RenderSpec {
    RenderSpec {
        output: output.to_owned(),
        comparison,
        effect: EffectParams::default(),
        options: RenderOptions {
            encode: EncodeOptions {
                codec: Codec::Ffv1,
                ..Default::default()
            },
            ..Default::default()
        },
    }
}

fn rolling(k: i64) -> Comparison {
    Comparison::Rolling {
        offset: Offset::frames(k),
    }
}

fn decode_all(ff: &Ffmpeg, path: &Path) -> Vec<Frame> {
    let info = ff.probe(path).unwrap();
    let mut dec = ff.decode(&info, &DecodeOptions::default()).unwrap();
    std::iter::from_fn(|| dec.read_frame().unwrap()).collect()
}

/// Expected motion pixel: white where exactly one of the two frames has the square.
fn expect_diff(spec: &MovingSquare, out: &Frame, n: u32, r: Option<u32>, color: [u8; 3]) {
    let y = spec.y + spec.size / 2;
    for x in 0..spec.width {
        let want = match r {
            Some(r) if spec.covers(n, x, y) != spec.covers(r, x, y) => color,
            _ => [0; 3],
        };
        assert_eq!(out.pixel(x, y), want, "frame {n} vs {r:?}, x = {x}");
    }
    assert_eq!(out.pixel(0, 0), [0; 3]);
}

fn run(f: &Fixture, spec: &RenderSpec) -> Vec<Frame> {
    render(&f.ff, &f.info, spec, &AtomicBool::new(false), |_| {}).unwrap();
    decode_all(&f.ff, &spec.output)
}

#[test]
fn rolling_offset_one_frame() {
    let f = fixture(MovingSquare::default());
    let out = run(&f, &lossless(&f.dir.path().join("o.mkv"), rolling(1)));
    assert_eq!(out.len(), 30);
    expect_diff(&f.spec, &out[0], 0, None, [255; 3]);
    for n in 1..30 {
        expect_diff(&f.spec, &out[n as usize], n, Some(n - 1), [255; 3]);
    }
}

#[test]
fn rolling_ring_and_dual_decoder_agree() {
    let f = fixture(MovingSquare::default());
    for k in [5, -3] {
        let ring = run(
            &f,
            &lossless(&f.dir.path().join(format!("ring{k}.mkv")), rolling(k)),
        );
        let spec = lossless(&f.dir.path().join(format!("dual{k}.mkv")), rolling(k));
        let tuning = RenderTuning {
            ring_budget_bytes: 0,
        };
        render_with(
            &f.ff,
            &f.info,
            &spec,
            &AtomicBool::new(false),
            |_| {},
            tuning,
        )
        .unwrap();
        let dual = decode_all(&f.ff, &spec.output);
        assert_eq!(ring.len(), 30);
        assert!(ring == dual, "k = {k}: ring and dual output differ");
        for n in 0..30u32 {
            let r = n as i64 - k;
            let r = (0..30).contains(&r).then_some(r as u32);
            expect_diff(&f.spec, &ring[n as usize], n, r, [255; 3]);
        }
    }
}

#[test]
fn offset_in_seconds() {
    let f = fixture(MovingSquare::default());
    // 0.1 s at 30 fps = 3 frames.
    let out = run(
        &f,
        &lossless(
            &f.dir.path().join("o.mkv"),
            Comparison::Rolling {
                offset: Offset::seconds(0.1),
            },
        ),
    );
    expect_diff(&f.spec, &out[2], 2, None, [255; 3]);
    expect_diff(&f.spec, &out[10], 10, Some(7), [255; 3]);
}

#[test]
fn fixed_reference_frame() {
    let f = fixture(MovingSquare::default());
    let mut spec = lossless(&f.dir.path().join("o.mkv"), Comparison::Fixed { frame: 2 });
    spec.effect.color = Rgb([0, 255, 0]);
    let out = run(&f, &spec);
    for n in [0u32, 2, 3, 10, 29] {
        expect_diff(&f.spec, &out[n as usize], n, Some(2), [0, 255, 0]);
    }
}

#[test]
fn range_and_scaling() {
    let f = fixture(MovingSquare::default());
    let mut spec = lossless(&f.dir.path().join("o.mkv"), rolling(2));
    spec.options.start = Some(10);
    spec.options.end = Some(15);
    spec.options.height = Some(60);
    let plan = render(&f.ff, &f.info, &spec, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!((plan.width, plan.height, plan.frame_count()), (80, 60, 5));
    let out = decode_all(&f.ff, &spec.output);
    assert_eq!(out.len(), 5);
    assert_eq!((out[0].width(), out[0].height()), (80, 60));
    // Frame 10 is compared with frame 8 (before the range): the square moved 8 px = 4 px scaled.
    let y = (f.spec.y + f.spec.size / 2) / 2;
    assert!(
        out[0].pixel(f.spec.x(8) / 2 + 1, y)[0] > 200,
        "trailing edge lit"
    );
    assert_eq!(
        out[0].pixel(f.spec.x(10) / 2 + 2, y),
        [0; 3],
        "overlap dark"
    );
}

#[test]
fn default_h264_with_audio() {
    let f = fixture(MovingSquare {
        audio: true,
        ..Default::default()
    });
    let spec = RenderSpec {
        output: f.dir.path().join("o.mp4"),
        comparison: rolling(1),
        effect: EffectParams::default(),
        options: RenderOptions::default(),
    };
    let mut last = None;
    render(&f.ff, &f.info, &spec, &AtomicBool::new(false), |p| {
        last = Some(p)
    })
    .unwrap();
    let last = last.unwrap();
    assert_eq!((last.done, last.total), (30, 30));
    let info = f.ff.probe(&spec.output).unwrap();
    assert_eq!(info.codec, "h264");
    assert!(info.has_audio);
    assert_eq!(info.frame_count, 30);
}

#[test]
fn cancel_removes_output() {
    let f = fixture(MovingSquare {
        frames: 60,
        ..Default::default()
    });
    let spec = lossless(&f.dir.path().join("o.mkv"), rolling(1));
    let cancel = AtomicBool::new(false);
    let result = render(&f.ff, &f.info, &spec, &cancel, |p| {
        if p.done == 5 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert!(matches!(result, Err(RenderError::Cancelled)), "{result:?}");
    assert!(!spec.output.exists());
}

#[test]
fn invalid_range_and_encoder_errors() {
    let f = fixture(MovingSquare::default());
    let mut spec = lossless(&f.dir.path().join("o.mkv"), rolling(1));
    spec.options.start = Some(20);
    spec.options.end = Some(20);
    let r = render(&f.ff, &f.info, &spec, &AtomicBool::new(false), |_| {});
    assert!(matches!(r, Err(RenderError::InvalidRange { .. })));

    let mut spec = lossless(&f.dir.path().join("bad.mkv"), rolling(1));
    spec.options.encode.extra_args = vec!["-c:v".into(), "no_such_encoder".into()];
    let r = render(&f.ff, &f.info, &spec, &AtomicBool::new(false), |_| {});
    assert!(matches!(r, Err(RenderError::Media(_))), "{r:?}");
    assert!(!spec.output.exists());
}
