//! sRGB ↔ linear conversion with lookup tables.

use std::sync::OnceLock;

const LIN_TO_SRGB_STEPS: usize = 4096;

pub fn srgb_to_linear_exact(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb_exact(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Linear value for an 8-bit sRGB value.
pub fn srgb8_to_linear(v: u8) -> f32 {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| std::array::from_fn(|i| srgb_to_linear_exact(i as f32 / 255.0)))[v as usize]
}

/// Linear value for an sRGB value in `0..=1` (quantised to 12 bits).
pub fn srgb_to_linear(v: f32) -> f32 {
    static LUT: OnceLock<Vec<f32>> = OnceLock::new();
    let lut = LUT.get_or_init(|| {
        (0..LIN_TO_SRGB_STEPS)
            .map(|i| srgb_to_linear_exact(i as f32 / (LIN_TO_SRGB_STEPS - 1) as f32))
            .collect()
    });
    lut[(v.clamp(0.0, 1.0) * (LIN_TO_SRGB_STEPS - 1) as f32 + 0.5) as usize]
}

/// sRGB value in `0..=1` for a linear value (clamped to `0..=1`).
pub fn linear_to_srgb(v: f32) -> f32 {
    static LUT: OnceLock<Vec<f32>> = OnceLock::new();
    let lut = LUT.get_or_init(|| {
        (0..LIN_TO_SRGB_STEPS)
            .map(|i| linear_to_srgb_exact(i as f32 / (LIN_TO_SRGB_STEPS - 1) as f32))
            .collect()
    });
    // Linear values are dense near zero, so interpolate to keep dark tones accurate.
    let x = v.clamp(0.0, 1.0) * (LIN_TO_SRGB_STEPS - 1) as f32;
    let i = (x as usize).min(LIN_TO_SRGB_STEPS - 2);
    let t = x - i as f32;
    lut[i] + (lut[i + 1] - lut[i]) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_8bit() {
        for v in 0..=255u8 {
            let back = (linear_to_srgb(srgb8_to_linear(v)) * 255.0).round() as u8;
            assert_eq!(back, v);
            let back = (linear_to_srgb(srgb_to_linear(v as f32 / 255.0)) * 255.0).round() as i32;
            assert!((back - v as i32).abs() <= 1, "{v} -> {back}");
        }
    }
}
