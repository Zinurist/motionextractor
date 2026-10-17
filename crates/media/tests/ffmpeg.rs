//! Integration tests against a real ffmpeg.

use mx_core::Frame;
use mx_media::testing::{MovingSquare, ffmpeg, index_level, index_video};
use mx_media::{Codec, DecodeOptions, EncodeOptions, EncodeTarget, MediaError, Rational};

fn decode_all(
    ff: &mx_media::Ffmpeg,
    info: &mx_media::VideoInfo,
    opts: DecodeOptions,
) -> Vec<Frame> {
    let mut dec = ff.decode(info, &opts).unwrap();
    let mut frames = Vec::new();
    while let Some(f) = dec.read_frame().unwrap() {
        frames.push(f);
    }
    frames
}

#[test]
fn probe_square_video() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    let spec = MovingSquare {
        audio: true,
        ..Default::default()
    };
    let info = ff.probe(&spec.generate(&ff, dir.path())).unwrap();
    assert_eq!((info.width, info.height), (160, 120));
    assert_eq!(info.fps, Rational::new(30, 1));
    assert_eq!(info.frame_count, 30);
    assert!(info.has_audio);
    assert_eq!(info.codec, "ffv1");
}

#[test]
fn probe_rejects_non_video() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not_a_video.mp4");
    std::fs::write(&path, b"hello").unwrap();
    assert!(matches!(ff.probe(&path), Err(MediaError::Probe(_))));
}

#[test]
fn decode_full_video_is_exact() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    let spec = MovingSquare::default();
    let info = ff.probe(&spec.generate(&ff, dir.path())).unwrap();
    let frames = decode_all(&ff, &info, DecodeOptions::default());
    assert_eq!(frames.len(), 30);
    for (n, f) in frames.iter().enumerate() {
        let n = n as u32;
        for (x, y) in [
            (spec.x(n), spec.y),
            (spec.x(n) + 15, spec.y + 15),
            (spec.x(n) + 16, spec.y),
            (0, 0),
        ] {
            let expected = if spec.covers(n, x, y) {
                [255; 3]
            } else {
                [0; 3]
            };
            assert_eq!(f.pixel(x, y), expected, "frame {n} pixel ({x},{y})");
        }
    }
}

#[test]
fn decode_range_and_scale() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    let spec = MovingSquare::default();
    let info = ff.probe(&spec.generate(&ff, dir.path())).unwrap();
    let frames = decode_all(
        &ff,
        &info,
        DecodeOptions {
            start_frame: 10,
            frame_count: Some(5),
            ..Default::default()
        },
    );
    assert_eq!(frames.len(), 5);
    for (i, f) in frames.iter().enumerate() {
        let n = 10 + i as u32;
        assert_eq!(f.pixel(spec.x(n) + 8, spec.y + 8), [255; 3], "frame {n}");
        assert_eq!(f.pixel(spec.x(n) - 1, spec.y + 8), [0; 3], "frame {n}");
    }
    let small = decode_all(
        &ff,
        &info,
        DecodeOptions {
            size: Some((80, 60)),
            frame_count: Some(2),
            ..Default::default()
        },
    );
    assert_eq!(small.len(), 2);
    assert_eq!((small[0].width(), small[0].height()), (80, 60));
}

#[test]
fn seeking_is_frame_accurate() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    // mp4: exact timestamps; mkv: millisecond timestamps; ts: non-zero start time.
    for (fps, ext) in [
        ("30", "mp4"),
        ("30000/1001", "mkv"),
        ("25", "ts"),
        ("24000/1001", "mp4"),
    ] {
        let info = ff
            .probe(&index_video(&ff, dir.path(), 60, fps, ext))
            .unwrap();
        assert!(
            (info.frame_count as i64 - 60).abs() <= 1,
            "{ext}: {} frames",
            info.frame_count
        );
        for start in [0u64, 1, 5, 11, 12, 13, 24, 35, 57] {
            let frames = decode_all(
                &ff,
                &info,
                DecodeOptions {
                    start_frame: start,
                    frame_count: Some(3),
                    ..Default::default()
                },
            );
            assert_eq!(frames.len(), 3, "{fps} {ext} start {start}");
            for (i, f) in frames.iter().enumerate() {
                let n = start + i as u64;
                let got = f.pixel(32, 24)[0] as i32;
                let want = index_level(n) as i32;
                assert!(
                    (got - want).abs() <= 2,
                    "{fps} {ext} start {start}: frame {n} has level {got}, expected {want}"
                );
            }
        }
    }
}

#[test]
fn threaded_decoder_and_early_drop() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    let info = ff
        .probe(&MovingSquare::default().generate(&ff, dir.path()))
        .unwrap();
    let mut rx = ff
        .decode(&info, &DecodeOptions::default())
        .unwrap()
        .into_threaded(2);
    let mut n = 0;
    while rx.recv().unwrap().is_some() {
        n += 1;
    }
    assert_eq!(n, 30);
    assert!(rx.recv().unwrap().is_none());
    // Dropping a receiver mid-stream must not hang.
    let mut rx = ff
        .decode(&info, &DecodeOptions::default())
        .unwrap()
        .into_threaded(1);
    rx.recv().unwrap().unwrap();
    drop(rx);
}

fn target(dir: &std::path::Path, name: &str) -> EncodeTarget {
    EncodeTarget {
        width: 64,
        height: 48,
        fps: Rational::new(25, 1),
        output: dir.join(name),
        audio: None,
    }
}

fn test_frames() -> Vec<Frame> {
    (0..10u8)
        .map(|i| {
            let mut f = Frame::black(64, 48);
            f.fill_rect(i as u32 * 4, 10, 8, 8, [255, 128, i * 20]);
            f
        })
        .collect()
}

#[test]
fn encode_lossless_roundtrip() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    let t = target(dir.path(), "out.mkv");
    let opts = EncodeOptions {
        codec: Codec::Ffv1,
        ..Default::default()
    };
    let mut enc = ff.encode(&opts, &t).unwrap();
    let frames = test_frames();
    for f in &frames {
        enc.write_frame(f).unwrap();
    }
    enc.finish().unwrap();
    let info = ff.probe(&t.output).unwrap();
    assert_eq!(info.fps, Rational::new(25, 1));
    assert_eq!(decode_all(&ff, &info, DecodeOptions::default()), frames);
}

#[test]
fn encode_each_codec() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    for codec in Codec::ALL {
        let name = if codec == Codec::Png {
            "png_seq".to_owned()
        } else {
            format!("out_{}.{}", codec.name(), codec.extension())
        };
        let t = target(dir.path(), &name);
        let mut enc = ff
            .encode(
                &EncodeOptions {
                    codec,
                    ..Default::default()
                },
                &t,
            )
            .unwrap();
        for f in test_frames() {
            enc.write_frame(&f).unwrap();
        }
        enc.finish().unwrap_or_else(|e| panic!("{codec:?}: {e}"));
        if codec == Codec::Png {
            assert_eq!(std::fs::read_dir(&t.output).unwrap().count(), 10);
        } else {
            let info = ff.probe(&t.output).unwrap();
            assert_eq!(info.frame_count, 10, "{codec:?}");
            assert_eq!((info.width, info.height), (64, 48), "{codec:?}");
        }
    }
}

#[test]
fn encode_odd_size_is_cropped_for_yuv420() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    let t = EncodeTarget {
        width: 65,
        height: 49,
        ..target(dir.path(), "odd.mp4")
    };
    let mut enc = ff.encode(&EncodeOptions::default(), &t).unwrap();
    enc.write_frame(&Frame::black(65, 49)).unwrap();
    enc.finish().unwrap();
    let info = ff.probe(&t.output).unwrap();
    assert_eq!((info.width, info.height), (64, 48));
}

#[test]
fn encode_errors_are_reported() {
    let ff = ffmpeg();
    let dir = tempfile::tempdir().unwrap();
    let opts = EncodeOptions {
        extra_args: vec!["-c:v".into(), "no_such_encoder".into()],
        ..Default::default()
    };
    let mut enc = ff.encode(&opts, &target(dir.path(), "bad.mp4")).unwrap();
    let mut result = Ok(());
    for f in test_frames() {
        result = enc.write_frame(&f);
        if result.is_err() {
            break;
        }
    }
    let err = match result {
        Err(e) => e,
        Ok(()) => enc.finish().unwrap_err(),
    };
    match err {
        MediaError::Ffmpeg { stderr, .. } => {
            assert!(stderr.contains("no_such_encoder"), "{stderr}")
        }
        other => panic!("unexpected error {other:?}"),
    }
    let mut enc = ff
        .encode(&EncodeOptions::default(), &target(dir.path(), "x.mp4"))
        .unwrap();
    assert!(matches!(
        enc.write_frame(&Frame::black(2, 2)),
        Err(MediaError::InvalidOption(_))
    ));
}
