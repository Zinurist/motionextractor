//! Fast approximate Gaussian blur on interleaved `f32` images.
//!
//! Three successive box blurs approximate a Gaussian. The vertical pass slides a running row sum
//! down horizontal bands (cache friendly, no transposes). Large radii are blurred on a
//! downsampled image and upsampled bilinearly, keeping the cost roughly independent of the radius.

use rayon::prelude::*;

use crate::frame::Frame;

/// Above this sigma (in pixels) the image is downsampled before blurring.
const MAX_DIRECT_SIGMA: f32 = 3.0;

/// Returns a Gaussian-blurred copy of a single-channel plane (`w × h`, row-major) with standard
/// deviation `sigma` in pixels. Edges are clamped.
pub fn gaussian_blur_plane(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    gaussian_blur(src, w, h, 1, sigma)
}

/// Like [`gaussian_blur_plane`] for an image with `ch` interleaved channels.
pub fn gaussian_blur(src: &[f32], w: usize, h: usize, ch: usize, sigma: f32) -> Vec<f32> {
    assert_eq!(src.len(), w * h * ch, "image size mismatch");
    if sigma < 0.5 || w == 0 || h == 0 {
        return src.to_vec();
    }
    if sigma <= MAX_DIRECT_SIGMA {
        return blur_direct(src, w, h, ch, sigma);
    }
    let f = ((sigma / 2.0) as usize).max(2);
    let (dw, dh) = (w.div_ceil(f), h.div_ceil(f));
    let small = downsample(src, w, h, ch, f, dw, dh);
    // The box downsample itself contributes variance (f² − 1) / 12 (in source pixels).
    let remaining = (sigma * sigma - (f * f - 1) as f32 / 12.0).max(0.0).sqrt();
    let blurred = blur_direct(&small, dw, dh, ch, remaining / f as f32);
    upsample(&blurred, dw, dh, ch, f, w, h)
}

/// Gaussian blur of an RGB frame.
pub(crate) fn blur_frame(frame: &Frame, sigma: f32) -> Frame {
    let (w, h) = (frame.width() as usize, frame.height() as usize);
    let src: Vec<f32> = frame.data().par_iter().map(|&v| v as f32).collect();
    let out = gaussian_blur(&src, w, h, 3, sigma);
    let data = out
        .par_iter()
        .map(|&v| (v + 0.5).clamp(0.0, 255.0) as u8)
        .collect();
    Frame::new(frame.width(), frame.height(), data).expect("same size")
}

fn blur_direct(src: &[f32], w: usize, h: usize, ch: usize, sigma: f32) -> Vec<f32> {
    let radii = box_radii(sigma);
    let row_len = w * ch;
    let mut a = vec![0.0; src.len()];
    // All horizontal passes of a row run back to back while the row is in cache.
    a.par_chunks_mut(row_len)
        .zip(src.par_chunks(row_len))
        .for_each_init(
            || vec![0.0; row_len],
            |tmp, (out, row)| {
                box_row(row, tmp, w, ch, radii[0]);
                box_row(tmp, out, w, ch, radii[1]);
                tmp.copy_from_slice(out);
                box_row(tmp, out, w, ch, radii[2]);
            },
        );
    let mut b = vec![0.0; src.len()];
    for &r in &radii {
        box_columns(&a, &mut b, row_len, h, r);
        std::mem::swap(&mut a, &mut b);
    }
    a
}

/// Radii of three box filters whose convolution approximates a Gaussian of `sigma`.
fn box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let wlf = wl as f32;
    let m_ideal =
        (12.0 * sigma * sigma - n * wlf * wlf - 4.0 * n * wlf - 3.0 * n) / (-4.0 * wlf - 4.0);
    let m = m_ideal.round() as i64;
    std::array::from_fn(|i| {
        let size = if (i as i64) < m { wl } else { wu };
        ((size - 1) / 2) as usize
    })
}

/// Horizontal box blur of radius `r` over one row of `w` pixels with `ch` channels.
fn box_row(src: &[f32], dst: &mut [f32], w: usize, ch: usize, r: usize) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let inv = 1.0 / (2 * r + 1) as f32;
    let last = w as isize - 1;
    let r = r as isize;
    for c in 0..ch {
        let at = |x: isize| src[x.clamp(0, last) as usize * ch + c];
        let mut sum: f32 = (-r..=r).map(at).sum();
        for x in 0..w {
            dst[x * ch + c] = sum * inv;
            let xi = x as isize;
            sum += at(xi + r + 1) - at(xi - r);
        }
    }
}

/// Vertical box blur of radius `r`: each band of rows slides a running sum of whole rows.
fn box_columns(src: &[f32], dst: &mut [f32], row_len: usize, h: usize, r: usize) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let inv = 1.0 / (2 * r + 1) as f32;
    let band = (h / (rayon::current_num_threads() * 4)).max(16);
    let row = |y: isize| {
        let y = y.clamp(0, h as isize - 1) as usize;
        &src[y * row_len..(y + 1) * row_len]
    };
    let r = r as isize;
    dst.par_chunks_mut(band * row_len)
        .enumerate()
        .for_each(|(bi, out)| {
            let y0 = (bi * band) as isize;
            let mut sum = vec![0.0f32; row_len];
            for y in y0 - r..=y0 + r {
                sum.iter_mut().zip(row(y)).for_each(|(s, v)| *s += v);
            }
            for (k, out_row) in out.chunks_mut(row_len).enumerate() {
                let y = y0 + k as isize;
                out_row.iter_mut().zip(&sum).for_each(|(o, s)| *o = s * inv);
                let (add, sub) = (row(y + r + 1), row(y - r));
                sum.iter_mut()
                    .zip(add.iter().zip(sub))
                    .for_each(|(s, (a, b))| *s += a - b);
            }
        });
}

fn downsample(
    src: &[f32],
    w: usize,
    h: usize,
    ch: usize,
    f: usize,
    dw: usize,
    dh: usize,
) -> Vec<f32> {
    let mut out = vec![0.0; dw * dh * ch];
    out.par_chunks_mut(dw * ch)
        .enumerate()
        .for_each(|(dy, row)| {
            let y0 = dy * f;
            let y1 = (y0 + f).min(h);
            for dx in 0..dw {
                let x0 = dx * f;
                let x1 = (x0 + f).min(w);
                let n = ((y1 - y0) * (x1 - x0)) as f32;
                for c in 0..ch {
                    let mut sum = 0.0;
                    for y in y0..y1 {
                        let base = y * w * ch;
                        sum += (x0..x1).map(|x| src[base + x * ch + c]).sum::<f32>();
                    }
                    row[dx * ch + c] = sum / n;
                }
            }
        });
    out
}

fn upsample(
    src: &[f32],
    sw: usize,
    sh: usize,
    ch: usize,
    f: usize,
    w: usize,
    h: usize,
) -> Vec<f32> {
    let coord = |x: usize, n: usize| {
        let u = ((x as f32 + 0.5) / f as f32 - 0.5).clamp(0.0, (n - 1) as f32);
        let i = (u as usize).min(n.saturating_sub(2));
        (i, (u - i as f32).min(1.0))
    };
    let xs: Vec<(usize, f32)> = (0..w).map(|x| coord(x, sw)).collect();
    let srow = sw * ch;
    let mut out = vec![0.0; w * h * ch];
    out.par_chunks_mut(w * ch).enumerate().for_each(|(y, row)| {
        let (y0, ty) = coord(y, sh);
        let y1 = (y0 + 1).min(sh - 1);
        let (r0, r1) = (
            &src[y0 * srow..(y0 + 1) * srow],
            &src[y1 * srow..(y1 + 1) * srow],
        );
        for (x, &(x0, tx)) in xs.iter().enumerate() {
            let x1 = (x0 + 1).min(sw - 1);
            for c in 0..ch {
                let (a, b) = (x0 * ch + c, x1 * ch + c);
                let top = r0[a] + (r0[b] - r0[a]) * tx;
                let bottom = r1[a] + (r1[b] - r1[a]) * tx;
                row[x * ch + c] = top + (bottom - top) * ty;
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(w: usize, h: usize) -> Vec<f32> {
        let mut p = vec![0.0; w * h];
        p[(h / 2) * w + w / 2] = 1.0;
        p
    }

    /// Standard deviation of the blurred impulse along x and y.
    fn measured_sigma(p: &[f32], w: usize, h: usize) -> (f32, f32) {
        let spread = |vals: Vec<f32>, c: f32| {
            let total: f32 = vals.iter().sum();
            let var: f32 = vals
                .iter()
                .enumerate()
                .map(|(i, v)| v * (i as f32 - c).powi(2))
                .sum::<f32>()
                / total;
            var.sqrt()
        };
        let row = p[(h / 2) * w..(h / 2 + 1) * w].to_vec();
        let col = (0..h).map(|y| p[y * w + w / 2]).collect();
        (spread(row, (w / 2) as f32), spread(col, (h / 2) as f32))
    }

    #[test]
    fn preserves_energy_and_constant() {
        let (w, h) = (160, 160);
        for sigma in [1.0, 2.5, 6.0, 15.0] {
            let out = gaussian_blur_plane(&impulse(w, h), w, h, sigma);
            let total: f32 = out.iter().sum();
            assert!((total - 1.0).abs() < 0.02, "sigma {sigma}: energy {total}");
            let flat = gaussian_blur_plane(&vec![0.7; w * h], w, h, sigma);
            assert!(flat.iter().all(|v| (v - 0.7).abs() < 1e-4));
        }
    }

    #[test]
    fn spread_matches_sigma_in_both_directions() {
        let (w, h) = (257, 257);
        for sigma in [1.5, 3.0, 8.0, 20.0] {
            let out = gaussian_blur_plane(&impulse(w, h), w, h, sigma);
            let (sx, sy) = measured_sigma(&out, w, h);
            assert!(
                (sx - sigma).abs() / sigma < 0.15,
                "sigma {sigma}: measured x {sx}"
            );
            assert!(
                (sy - sigma).abs() / sigma < 0.15,
                "sigma {sigma}: measured y {sy}"
            );
        }
    }

    #[test]
    fn channels_are_independent() {
        let (w, h) = (40, 30);
        for sigma in [2.0, 9.0] {
            let plane = impulse(w, h);
            let mut rgb = vec![0.0; w * h * 3];
            for (i, v) in plane.iter().enumerate() {
                rgb[i * 3 + 1] = *v;
                rgb[i * 3 + 2] = 0.5;
            }
            let single = gaussian_blur_plane(&plane, w, h, sigma);
            let multi = gaussian_blur(&rgb, w, h, 3, sigma);
            for i in 0..w * h {
                assert!(multi[i * 3].abs() < 1e-6);
                assert!((multi[i * 3 + 1] - single[i]).abs() < 1e-5);
                assert!((multi[i * 3 + 2] - 0.5).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn tiny_sigma_is_identity() {
        let p = impulse(8, 8);
        assert_eq!(gaussian_blur_plane(&p, 8, 8, 0.2), p);
    }

    #[test]
    fn degenerate_sizes() {
        assert_eq!(gaussian_blur_plane(&[0.5], 1, 1, 10.0), vec![0.5]);
        let out = gaussian_blur_plane(&[1.0, 0.0, 0.0], 3, 1, 5.0);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(out[0] >= out[2]);
        let tall = gaussian_blur_plane(&impulse(1, 100), 1, 100, 2.0);
        assert!((tall.iter().sum::<f32>() - 1.0).abs() < 0.01);
    }
}
