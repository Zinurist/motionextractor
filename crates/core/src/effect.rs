use rayon::prelude::*;

use crate::blur::{blur_frame, gaussian_blur_plane};
use crate::color::{linear_to_srgb, srgb_to_linear, srgb8_to_linear};
use crate::frame::Frame;
use crate::params::{Background, BlendMode, EffectParams, Metric, ParamError};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EffectError {
    #[error("reference frame is {ref_w}x{ref_h}, current frame is {w}x{h}")]
    SizeMismatch {
        w: u32,
        h: u32,
        ref_w: u32,
        ref_h: u32,
    },
    #[error(transparent)]
    Param(#[from] ParamError),
}

/// A source frame together with its pre-filtered (denoised) version.
///
/// Pre-filtering only depends on the frame and the `denoise` parameter, so callers that use a
/// frame more than once (as current and later as reference) can prepare it once.
#[derive(Debug, Clone)]
pub struct Prepared {
    original: Frame,
    filtered: Option<Frame>,
    denoise: f32,
}

impl Prepared {
    pub fn new(frame: Frame, p: &EffectParams) -> Self {
        let sigma = p.denoise * frame.height() as f32;
        let filtered = (sigma >= 0.5).then(|| blur_frame(&frame, sigma));
        Self {
            original: frame,
            filtered,
            denoise: p.denoise,
        }
    }

    pub fn original(&self) -> &Frame {
        &self.original
    }

    /// The frame used for differencing.
    pub fn filtered(&self) -> &Frame {
        self.filtered.as_ref().unwrap_or(&self.original)
    }

    /// Whether this preparation is still valid for `p`.
    pub fn matches(&self, p: &EffectParams) -> bool {
        self.denoise == p.denoise
    }
}

/// Applies the motion extraction effect to `current`, comparing it with `reference`.
///
/// `reference = None` means the frame has no reference (e.g. the first frames in rolling mode):
/// the motion mask is zero and only the background remains.
pub fn process(
    current: &Frame,
    reference: Option<&Frame>,
    p: &EffectParams,
) -> Result<Frame, EffectError> {
    p.validate()?;
    check_sizes(current, reference)?;
    let current = Prepared::new(current.clone(), p);
    let reference = reference.map(|r| Prepared::new(r.clone(), p));
    process_prepared(&current, reference.as_ref(), p)
}

/// [`process`] for frames that were already prepared with the same `denoise` parameter.
pub fn process_prepared(
    current: &Prepared,
    reference: Option<&Prepared>,
    p: &EffectParams,
) -> Result<Frame, EffectError> {
    p.validate()?;
    check_sizes(current.original(), reference.map(Prepared::original))?;
    debug_assert!(current.matches(p) && reference.is_none_or(|r| r.matches(p)));
    let reference = reference.map(Prepared::filtered);
    Ok(match p.blend {
        BlendMode::Classic => classic(current.filtered(), reference),
        BlendMode::Difference => difference(current, reference, p),
    })
}

fn check_sizes(current: &Frame, reference: Option<&Frame>) -> Result<(), EffectError> {
    match reference {
        Some(r) if r.width() != current.width() || r.height() != current.height() => {
            Err(EffectError::SizeMismatch {
                w: current.width(),
                h: current.height(),
                ref_w: r.width(),
                ref_h: r.height(),
            })
        }
        _ => Ok(()),
    }
}

/// `0.5·C + 0.5·(1 − R)`; without a reference the image is uniformly mid grey.
fn classic(current: &Frame, reference: Option<&Frame>) -> Frame {
    let reference = reference.unwrap_or(current);
    let data = current
        .data()
        .par_iter()
        .zip(reference.data().par_iter())
        .map(|(&c, &r)| (c as u16 + 255 - r as u16).div_ceil(2) as u8)
        .collect();
    Frame::new(current.width(), current.height(), data).expect("same size")
}

fn difference(current: &Prepared, reference: Option<&Frame>, p: &EffectParams) -> Frame {
    let frame = current.original();
    let (w, h) = (frame.width() as usize, frame.height() as usize);
    let mask = match reference {
        Some(reference) => motion_mask(current.filtered(), reference, p),
        None => vec![0.0; w * h],
    };
    let bloom = (p.bloom.enabled && p.bloom.intensity > 0.0).then(|| bloom_plane(&mask, w, h, p));
    composite(frame, &mask, bloom.as_deref(), p)
}

/// Per-pixel motion value `m'` in `0..=1` after metric, threshold, gain and gamma.
fn motion_mask(current: &Frame, reference: &Frame, p: &EffectParams) -> Vec<f32> {
    let w = current.width() as usize;
    let (c, r) = (current.data(), reference.data());
    let lut: [f32; 256] = std::array::from_fn(|i| {
        let m = i as f32 / 255.0;
        let v = ((m - p.threshold) * p.gain).clamp(0.0, 1.0);
        if p.gamma == 1.0 {
            v
        } else {
            v.powf(1.0 / p.gamma)
        }
    });
    let metric = p.metric;
    let mut mask = vec![0.0; w * current.height() as usize];
    mask.par_chunks_mut(w)
        .zip(c.par_chunks(w * 3).zip(r.par_chunks(w * 3)))
        .for_each(|(out, (crow, rrow))| {
            for (o, (cp, rp)) in out
                .iter_mut()
                .zip(crow.chunks_exact(3).zip(rrow.chunks_exact(3)))
            {
                let d = [
                    cp[0].abs_diff(rp[0]),
                    cp[1].abs_diff(rp[1]),
                    cp[2].abs_diff(rp[2]),
                ];
                let m = match metric {
                    // Rec. 709 weights in 8-bit fixed point (54 + 183 + 19 = 256).
                    Metric::Luma => {
                        ((54 * d[0] as u32 + 183 * d[1] as u32 + 19 * d[2] as u32 + 128) >> 8) as u8
                    }
                    Metric::Max => d[0].max(d[1]).max(d[2]),
                };
                *o = lut[m as usize];
            }
        });
    mask
}

/// Blurred glow strength per pixel (multiply with the linear color and intensity).
fn bloom_plane(mask: &[f32], w: usize, h: usize, p: &EffectParams) -> Vec<f32> {
    let t = p.bloom.threshold;
    let scale = if t < 1.0 { 1.0 / (1.0 - t) } else { 0.0 };
    // The mask is a coverage value; glow is accumulated in linear light.
    let bright: Vec<f32> = mask.par_iter().map(|&m| (m - t).max(0.0) * scale).collect();
    gaussian_blur_plane(&bright, w, h, p.bloom.radius * h as f32)
}

fn composite(current: &Frame, mask: &[f32], bloom: Option<&[f32]>, p: &EffectParams) -> Frame {
    let w = current.width() as usize;
    let color = p.color.0.map(|c| c as f32 / 255.0);
    let color_lin = p.color.0.map(srgb8_to_linear);
    let glow = color_lin.map(|c| c * p.bloom.intensity);
    let original = p.background == Background::Original;
    let opacity = if original { p.overlay_opacity } else { 1.0 };
    let brightness = p.background_brightness / 255.0;

    let mut out = vec![0u8; current.data().len()];
    out.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        let src = &current.data()[y * w * 3..(y + 1) * w * 3];
        for x in 0..w {
            let i = y * w + x;
            let a = mask[i] * opacity;
            for ch in 0..3 {
                let bg = if original {
                    src[x * 3 + ch] as f32 * brightness
                } else {
                    0.0
                };
                let mut v = bg + (color[ch] - bg) * a;
                if let Some(b) = bloom {
                    v = linear_to_srgb(srgb_to_linear(v) + b[i] * glow[ch]);
                }
                row[x * 3 + ch] = (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    });
    Frame::new(current.width(), current.height(), out).expect("same size")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{Bloom, Rgb};
    use proptest::prelude::*;

    fn raw() -> EffectParams {
        // Identity mapping: mask = metric(|C − R|).
        EffectParams {
            threshold: 0.0,
            gain: 1.0,
            ..Default::default()
        }
    }

    fn square_frames(
        w: u32,
        h: u32,
        from: (u32, u32),
        to: (u32, u32),
        size: u32,
    ) -> (Frame, Frame) {
        let mut a = Frame::black(w, h);
        a.fill_rect(from.0, from.1, size, size, [255, 255, 255]);
        let mut b = Frame::black(w, h);
        b.fill_rect(to.0, to.1, size, size, [255, 255, 255]);
        (b, a)
    }

    #[test]
    fn identical_frames_are_black() {
        let f = Frame::filled(8, 6, [120, 30, 200]);
        let out = process(&f, Some(&f), &EffectParams::default()).unwrap();
        assert_eq!(out, Frame::black(8, 6));
    }

    #[test]
    fn no_reference_shows_background_only() {
        let f = Frame::filled(4, 4, [100, 150, 200]);
        assert_eq!(
            process(&f, None, &EffectParams::default()).unwrap(),
            Frame::black(4, 4)
        );
        let overlay = EffectParams {
            background: Background::Original,
            ..Default::default()
        };
        assert_eq!(process(&f, None, &overlay).unwrap(), f);
    }

    #[test]
    fn moved_square_lights_up_both_positions() {
        let (cur, refr) = square_frames(40, 20, (2, 2), (20, 2), 6);
        let out = process(&cur, Some(&refr), &EffectParams::default()).unwrap();
        assert_eq!(out.pixel(4, 4), [255, 255, 255], "old position");
        assert_eq!(out.pixel(22, 4), [255, 255, 255], "new position");
        assert_eq!(out.pixel(12, 4), [0, 0, 0]);
        assert_eq!(out.pixel(30, 15), [0, 0, 0]);
    }

    #[test]
    fn metric_luma_and_max() {
        let a = Frame::filled(1, 1, [0, 0, 0]);
        let b = Frame::filled(1, 1, [200, 0, 0]);
        let luma = process(&b, Some(&a), &raw()).unwrap();
        // 200 · 54/256 ≈ 42
        assert_eq!(luma.pixel(0, 0), [42, 42, 42]);
        let max = EffectParams {
            metric: Metric::Max,
            ..raw()
        };
        assert_eq!(
            process(&b, Some(&a), &max).unwrap().pixel(0, 0),
            [200, 200, 200]
        );
    }

    #[test]
    fn threshold_gain_gamma() {
        let a = Frame::filled(1, 1, [0, 0, 0]);
        let small = Frame::filled(1, 1, [5, 5, 5]);
        let mid = Frame::filled(1, 1, [51, 51, 51]); // 0.2
        let p = EffectParams {
            threshold: 0.05,
            gain: 2.0,
            ..Default::default()
        };
        assert_eq!(
            process(&small, Some(&a), &p).unwrap().pixel(0, 0),
            [0, 0, 0]
        );
        // (0.2 − 0.05) · 2 = 0.3 → 77
        assert_eq!(
            process(&mid, Some(&a), &p).unwrap().pixel(0, 0),
            [77, 77, 77]
        );
        let g = EffectParams { gamma: 2.0, ..p };
        // 0.3^(1/2) ≈ 0.548 → 140
        assert_eq!(
            process(&mid, Some(&a), &g).unwrap().pixel(0, 0),
            [140, 140, 140]
        );
    }

    #[test]
    fn color_tints_motion() {
        let (cur, refr) = square_frames(10, 10, (0, 0), (5, 5), 3);
        let p = EffectParams {
            color: Rgb([255, 64, 0]),
            ..Default::default()
        };
        let out = process(&cur, Some(&refr), &p).unwrap();
        assert_eq!(out.pixel(6, 6), [255, 64, 0]);
        assert_eq!(out.pixel(9, 0), [0, 0, 0]);
    }

    #[test]
    fn overlay_draws_motion_over_original() {
        let mut refr = Frame::filled(10, 10, [100, 100, 100]);
        refr.fill_rect(0, 0, 3, 3, [0, 0, 0]);
        let mut cur = Frame::filled(10, 10, [100, 100, 100]);
        cur.fill_rect(0, 0, 3, 3, [255, 255, 255]);
        let p = EffectParams {
            background: Background::Original,
            color: Rgb([255, 0, 0]),
            ..Default::default()
        };
        let out = process(&cur, Some(&refr), &p).unwrap();
        assert_eq!(out.pixel(1, 1), [255, 0, 0], "motion drawn");
        assert_eq!(
            out.pixel(8, 8),
            [100, 100, 100],
            "static original untouched"
        );

        let dimmed = EffectParams {
            background_brightness: 0.5,
            overlay_opacity: 0.5,
            ..p
        };
        let out = process(&cur, Some(&refr), &dimmed).unwrap();
        assert_eq!(out.pixel(8, 8), [50, 50, 50]);
        // lerp(white · 0.5, red, 0.5) = (191, 64, 64)
        assert_eq!(out.pixel(1, 1), [191, 64, 64]);
    }

    #[test]
    fn bloom_glows_around_motion() {
        let (cur, refr) = square_frames(100, 100, (45, 45), (45, 45), 0);
        let mut cur = cur;
        cur.fill_rect(45, 45, 10, 10, [255, 255, 255]);
        let p = EffectParams {
            bloom: Bloom {
                enabled: true,
                radius: 0.05,
                intensity: 2.0,
                threshold: 0.0,
            },
            ..Default::default()
        };
        let plain = process(&cur, Some(&refr), &EffectParams::default()).unwrap();
        let glow = process(&cur, Some(&refr), &p).unwrap();
        assert_eq!(plain.pixel(40, 50), [0, 0, 0]);
        let near = glow.pixel(40, 50)[0];
        let far = glow.pixel(5, 5)[0];
        assert!(near > 30, "glow next to the square: {near}");
        assert!(far < near, "glow falls off: far {far}, near {near}");
        assert_eq!(glow.pixel(50, 50), [255, 255, 255]);
    }

    #[test]
    fn bloom_is_resolution_independent() {
        let make = |s: u32| {
            let mut cur = Frame::black(64 * s, 48 * s);
            cur.fill_rect(20 * s, 20 * s, 8 * s, 8 * s, [255, 255, 255]);
            (cur, Frame::black(64 * s, 48 * s))
        };
        let p = EffectParams {
            bloom: Bloom {
                enabled: true,
                radius: 0.08,
                intensity: 1.5,
                threshold: 0.0,
            },
            ..Default::default()
        };
        let (c1, r1) = make(1);
        let (c4, r4) = make(4);
        let small = process(&c1, Some(&r1), &p).unwrap();
        let big = process(&c4, Some(&r4), &p).unwrap();
        // Compare the big result box-downsampled 4× with the small one.
        let mut total_err = 0.0;
        for y in 0..48 {
            for x in 0..64 {
                let mut sum = 0u32;
                for dy in 0..4 {
                    for dx in 0..4 {
                        sum += big.pixel(x * 4 + dx, y * 4 + dy)[0] as u32;
                    }
                }
                let avg = sum as f32 / 16.0;
                total_err += (avg - small.pixel(x, y)[0] as f32).abs();
            }
        }
        let mean_err = total_err / (64.0 * 48.0);
        assert!(mean_err < 4.0, "mean error {mean_err}");
    }

    #[test]
    fn denoise_suppresses_noise_but_keeps_motion() {
        let (w, h) = (80, 60);
        let mut refr = Frame::black(w, h);
        let mut cur = Frame::black(w, h);
        // Checkerboard noise of ±12 levels everywhere.
        for y in 0..h {
            for x in 0..w {
                let n = if (x + y) % 2 == 0 { 12 } else { 0 };
                refr.set_pixel(x, y, [n, n, n]);
                cur.set_pixel(x, y, [12 - n, 12 - n, 12 - n]);
            }
        }
        cur.fill_rect(30, 20, 20, 20, [255, 255, 255]);
        let noisy = process(&cur, Some(&refr), &EffectParams::default()).unwrap();
        assert!(noisy.pixel(5, 5)[0] > 0, "noise visible without denoise");
        let p = EffectParams {
            denoise: 0.02,
            ..Default::default()
        };
        let clean = process(&cur, Some(&refr), &p).unwrap();
        assert_eq!(clean.pixel(5, 5), [0, 0, 0]);
        assert_eq!(clean.pixel(40, 30), [255, 255, 255]);
    }

    #[test]
    fn classic_mode() {
        let c = Frame::filled(2, 2, [200, 100, 0]);
        let r = Frame::filled(2, 2, [200, 100, 0]);
        let p = EffectParams {
            blend: BlendMode::Classic,
            ..Default::default()
        };
        assert_eq!(
            process(&c, Some(&r), &p).unwrap().pixel(0, 0),
            [128, 128, 128]
        );
        assert_eq!(process(&c, None, &p).unwrap().pixel(1, 1), [128, 128, 128]);
        let white = Frame::filled(2, 2, [255, 255, 255]);
        let black = Frame::black(2, 2);
        assert_eq!(
            process(&white, Some(&black), &p).unwrap().pixel(0, 0),
            [255, 255, 255]
        );
        assert_eq!(
            process(&black, Some(&white), &p).unwrap().pixel(0, 0),
            [0, 0, 0]
        );
    }

    #[test]
    fn rejects_mismatched_sizes_and_bad_params() {
        let a = Frame::black(4, 4);
        let b = Frame::black(4, 5);
        assert!(matches!(
            process(&a, Some(&b), &EffectParams::default()),
            Err(EffectError::SizeMismatch { .. })
        ));
        let bad = EffectParams {
            gain: -1.0,
            ..Default::default()
        };
        assert!(matches!(
            process(&a, None, &bad),
            Err(EffectError::Param(_))
        ));
    }

    fn arb_frame(w: u32, h: u32) -> impl Strategy<Value = Frame> {
        proptest::collection::vec(any::<u8>(), Frame::byte_len(w, h))
            .prop_map(move |d| Frame::new(w, h, d).unwrap())
    }

    proptest! {
        #[test]
        fn difference_is_symmetric(a in arb_frame(7, 5), b in arb_frame(7, 5)) {
            let p = EffectParams::default();
            prop_assert_eq!(process(&a, Some(&b), &p).unwrap(), process(&b, Some(&a), &p).unwrap());
        }

        #[test]
        fn self_difference_is_black(a in arb_frame(6, 4), color in any::<[u8; 3]>()) {
            let p = EffectParams { color: Rgb(color), ..Default::default() };
            prop_assert_eq!(process(&a, Some(&a), &p).unwrap(), Frame::black(6, 4));
        }

        #[test]
        fn motion_is_bounded_by_color(a in arb_frame(5, 5), b in arb_frame(5, 5), color in any::<[u8; 3]>()) {
            let p = EffectParams { color: Rgb(color), ..Default::default() };
            let out = process(&a, Some(&b), &p).unwrap();
            for px in out.data().chunks(3) {
                for ch in 0..3 {
                    prop_assert!(px[ch] <= color[ch]);
                }
            }
        }
    }
}
