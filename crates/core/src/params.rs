use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ParamError {
    #[error("{name} must be between {min} and {max}, got {value}")]
    OutOfRange {
        name: &'static str,
        value: f64,
        min: f64,
        max: f64,
    },
    #[error("offset must not be zero")]
    ZeroOffset,
    #[error("invalid color {0:?}, expected #rrggbb")]
    BadColor(String),
    #[error("invalid offset {0:?}, expected e.g. 3, 3f or 0.5s")]
    BadOffset(String),
}

/// How the per-channel difference is reduced to a single motion value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Rec. 709 luma of the difference.
    #[default]
    Luma,
    /// Maximum of the channel differences.
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    /// Absolute difference → mask → color/bloom/composite.
    #[default]
    Difference,
    /// The original editor technique: average of the frame and the inverted reference.
    Classic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    /// A solid color (`background_color`, black by default).
    #[default]
    #[serde(alias = "black")]
    Color,
    /// Overlay mode: motion is drawn on top of the original video.
    Original,
}

/// What the moving parts show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionFill {
    /// The motion color.
    #[default]
    Color,
    /// The original video, as if seen through a window where things move.
    Original,
}

/// An sRGB color, serialised as `"#rrggbb"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub const WHITE: Rgb = Rgb([255, 255, 255]);
    pub const BLACK: Rgb = Rgb([0, 0, 0]);
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(f, "#{r:02x}{g:02x}{b:02x}")
    }
}

impl FromStr for Rgb {
    type Err = ParamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        let bad = || ParamError::BadColor(s.to_owned());
        if hex.len() != 6 || !hex.is_ascii() {
            return Err(bad());
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
        Ok(Rgb([channel(0)?, channel(2)?, channel(4)?]))
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bloom {
    pub enabled: bool,
    /// Motion values below this do not glow (`0..1`).
    pub threshold: f32,
    /// Blur radius as a fraction of the frame height.
    pub radius: f32,
    pub intensity: f32,
}

impl Default for Bloom {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: 0.0,
            radius: 0.02,
            intensity: 1.0,
        }
    }
}

/// All parameters of the effect pipeline. Spatial sizes are fractions of the frame height so the
/// result does not depend on the processing resolution.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectParams {
    pub blend: BlendMode,
    pub metric: Metric,
    /// Pre-blur radius (fraction of height) applied to both inputs; 0 disables it.
    pub denoise: f32,
    pub threshold: f32,
    pub gain: f32,
    pub gamma: f32,
    pub color: Rgb,
    /// Whether moving parts show `color` or the original video.
    pub motion_fill: MotionFill,
    pub bloom: Bloom,
    pub background: Background,
    /// Background color when `background` is `Color`.
    pub background_color: Rgb,
    /// Brightness factor for the original video in overlay mode.
    pub background_brightness: f32,
    /// Opacity of the motion layer in overlay mode.
    pub overlay_opacity: f32,
}

impl Default for EffectParams {
    fn default() -> Self {
        Self {
            blend: BlendMode::Difference,
            metric: Metric::Luma,
            denoise: 0.0,
            threshold: 0.02,
            gain: 4.0,
            gamma: 1.0,
            color: Rgb::WHITE,
            motion_fill: MotionFill::Color,
            bloom: Bloom::default(),
            background: Background::Color,
            background_color: Rgb::BLACK,
            background_brightness: 1.0,
            overlay_opacity: 1.0,
        }
    }
}

fn check(name: &'static str, value: f32, min: f32, max: f32) -> Result<(), ParamError> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(ParamError::OutOfRange {
            name,
            value: value as f64,
            min: min as f64,
            max: max as f64,
        })
    }
}

impl EffectParams {
    pub fn validate(&self) -> Result<(), ParamError> {
        check("denoise", self.denoise, 0.0, 0.1)?;
        check("threshold", self.threshold, 0.0, 1.0)?;
        check("gain", self.gain, 0.0, 100.0)?;
        check("gamma", self.gamma, 0.1, 10.0)?;
        check("bloom.threshold", self.bloom.threshold, 0.0, 1.0)?;
        check("bloom.radius", self.bloom.radius, 0.0, 0.5)?;
        check("bloom.intensity", self.bloom.intensity, 0.0, 10.0)?;
        check(
            "background_brightness",
            self.background_brightness,
            0.0,
            1.0,
        )?;
        check("overlay_opacity", self.overlay_opacity, 0.0, 1.0)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeUnit {
    #[default]
    Frames,
    Seconds,
}

/// Signed distance to the reference frame, in frames or seconds. Positive values compare against
/// an earlier frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Offset {
    pub value: f64,
    pub unit: TimeUnit,
}

impl Offset {
    pub fn frames(n: i64) -> Self {
        Self {
            value: n as f64,
            unit: TimeUnit::Frames,
        }
    }

    pub fn seconds(s: f64) -> Self {
        Self {
            value: s,
            unit: TimeUnit::Seconds,
        }
    }

    pub fn validate(&self) -> Result<(), ParamError> {
        if !self.value.is_finite() || self.value == 0.0 {
            return Err(ParamError::ZeroOffset);
        }
        Ok(())
    }

    /// The offset in whole frames. Never zero for a non-zero offset: values that round to zero
    /// become ±1.
    pub fn to_frames(&self, fps: f64) -> i64 {
        let raw = match self.unit {
            TimeUnit::Frames => self.value,
            TimeUnit::Seconds => self.value * fps,
        };
        let k = raw.round() as i64;
        if k == 0 && raw != 0.0 {
            raw.signum() as i64
        } else {
            k
        }
    }
}

impl Default for Offset {
    fn default() -> Self {
        Self::frames(1)
    }
}

impl FromStr for Offset {
    type Err = ParamError;

    /// Parses `3`, `3f` (frames) or `0.5s` (seconds).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || ParamError::BadOffset(s.to_owned());
        let t = s.trim();
        let offset = if let Some(v) = t.strip_suffix('s') {
            Offset::seconds(v.trim().parse().map_err(|_| bad())?)
        } else {
            let v = t.strip_suffix('f').unwrap_or(t).trim();
            Offset::frames(v.parse().map_err(|_| bad())?)
        };
        offset.validate()?;
        Ok(offset)
    }
}

/// Which frame each frame is compared against.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Comparison {
    /// Compare frame `t` with frame `t − k`.
    Rolling { offset: Offset },
    /// Compare every frame with one fixed frame.
    Fixed { frame: u64 },
}

impl Default for Comparison {
    fn default() -> Self {
        Comparison::Rolling {
            offset: Offset::default(),
        }
    }
}

impl Comparison {
    pub fn validate(&self) -> Result<(), ParamError> {
        match self {
            Comparison::Rolling { offset } => offset.validate(),
            Comparison::Fixed { .. } => Ok(()),
        }
    }

    /// The reference frame index for frame `t` of a video with `count` frames, or `None` when it
    /// falls outside the video.
    pub fn reference_index(&self, t: u64, fps: f64, count: u64) -> Option<u64> {
        let r = match *self {
            Comparison::Rolling { offset } => t as i64 - offset.to_frames(fps),
            Comparison::Fixed { frame } => frame as i64,
        };
        (r >= 0 && (r as u64) < count).then_some(r as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_parse_and_format() {
        assert_eq!("#ff8000".parse::<Rgb>().unwrap(), Rgb([255, 128, 0]));
        assert_eq!("00ff00".parse::<Rgb>().unwrap(), Rgb([0, 255, 0]));
        assert!("#ff80".parse::<Rgb>().is_err());
        assert!("#gg0000".parse::<Rgb>().is_err());
        assert_eq!(Rgb([1, 171, 255]).to_string(), "#01abff");
    }

    #[test]
    fn offset_parse() {
        assert_eq!("3".parse::<Offset>().unwrap(), Offset::frames(3));
        assert_eq!("-2f".parse::<Offset>().unwrap(), Offset::frames(-2));
        assert_eq!("0.5s".parse::<Offset>().unwrap(), Offset::seconds(0.5));
        assert_eq!("0".parse::<Offset>(), Err(ParamError::ZeroOffset));
        assert!("abc".parse::<Offset>().is_err());
    }

    #[test]
    fn offset_to_frames() {
        assert_eq!(Offset::frames(5).to_frames(30.0), 5);
        assert_eq!(Offset::seconds(0.5).to_frames(30.0), 15);
        assert_eq!(Offset::seconds(1.0).to_frames(29.97), 30);
        assert_eq!(Offset::seconds(0.001).to_frames(30.0), 1);
        assert_eq!(Offset::seconds(-0.001).to_frames(30.0), -1);
    }

    #[test]
    fn reference_index() {
        let rolling = Comparison::Rolling {
            offset: Offset::frames(2),
        };
        assert_eq!(rolling.reference_index(0, 30.0, 10), None);
        assert_eq!(rolling.reference_index(1, 30.0, 10), None);
        assert_eq!(rolling.reference_index(2, 30.0, 10), Some(0));
        let future = Comparison::Rolling {
            offset: Offset::frames(-2),
        };
        assert_eq!(future.reference_index(7, 30.0, 10), Some(9));
        assert_eq!(future.reference_index(8, 30.0, 10), None);
        let fixed = Comparison::Fixed { frame: 4 };
        assert_eq!(fixed.reference_index(9, 30.0, 10), Some(4));
        assert_eq!(fixed.reference_index(0, 30.0, 4), None);
    }

    #[test]
    fn background_accepts_the_old_black_name() {
        let p: EffectParams = serde_json::from_str(r#"{"background": "black"}"#).unwrap();
        assert_eq!(p.background, Background::Color);
        assert_eq!(p.background_color, Rgb::BLACK);
        let json = serde_json::to_string(&EffectParams::default()).unwrap();
        assert!(json.contains(r#""background":"color""#), "{json}");
        assert!(json.contains(r#""motion_fill":"color""#), "{json}");
    }

    #[test]
    fn serde_roundtrip_and_defaults() {
        let p: EffectParams =
            serde_json::from_str(r##"{"color":"#ff0000","bloom":{"enabled":true}}"##).unwrap();
        assert_eq!(p.color, Rgb([255, 0, 0]));
        assert!(p.bloom.enabled);
        assert_eq!(p.bloom.radius, Bloom::default().radius);
        assert_eq!(p.gain, EffectParams::default().gain);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<EffectParams>(&json).unwrap(), p);

        let c: Comparison =
            serde_json::from_str(r#"{"mode":"rolling","offset":{"value":0.5,"unit":"seconds"}}"#)
                .unwrap();
        assert_eq!(
            c,
            Comparison::Rolling {
                offset: Offset::seconds(0.5)
            }
        );
        let c: Comparison = serde_json::from_str(r#"{"mode":"fixed","frame":12}"#).unwrap();
        assert_eq!(c, Comparison::Fixed { frame: 12 });
    }

    #[test]
    fn validate_ranges() {
        assert!(EffectParams::default().validate().is_ok());
        let p = EffectParams {
            threshold: 1.5,
            ..Default::default()
        };
        assert!(matches!(
            p.validate(),
            Err(ParamError::OutOfRange {
                name: "threshold",
                ..
            })
        ));
        let p = EffectParams {
            gain: f32::NAN,
            ..Default::default()
        };
        assert!(p.validate().is_err());
    }
}
