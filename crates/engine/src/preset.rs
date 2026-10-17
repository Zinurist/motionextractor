use std::path::Path;

use mx_core::{Comparison, EffectParams, ParamError};
use serde::{Deserialize, Serialize};

use crate::RenderOptions;

pub const PRESET_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("cannot read preset {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid preset: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported preset version {0}")]
    Version(u32),
    #[error(transparent)]
    Param(#[from] ParamError),
}

/// Effect parameters and render options of a session, saved as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub version: u32,
    pub comparison: Comparison,
    pub effect: EffectParams,
    pub render: RenderOptions,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            version: PRESET_VERSION,
            comparison: Comparison::default(),
            effect: EffectParams::default(),
            render: RenderOptions::default(),
        }
    }
}

impl Preset {
    pub fn from_json(json: &str) -> Result<Self, PresetError> {
        let preset: Preset = serde_json::from_str(json)?;
        if preset.version > PRESET_VERSION {
            return Err(PresetError::Version(preset.version));
        }
        preset.comparison.validate()?;
        preset.effect.validate()?;
        Ok(preset)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialisable")
    }

    pub fn load(path: &Path) -> Result<Self, PresetError> {
        let json = std::fs::read_to_string(path).map_err(|source| PresetError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&json)
    }

    pub fn save(&self, path: &Path) -> Result<(), PresetError> {
        std::fs::write(path, self.to_json()).map_err(|source| PresetError::Io {
            path: path.display().to_string(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mx_core::{Offset, Rgb};
    use mx_media::Codec;

    #[test]
    fn roundtrip() {
        let mut p = Preset {
            comparison: Comparison::Rolling {
                offset: Offset::seconds(0.25),
            },
            ..Default::default()
        };
        p.effect.color = Rgb([10, 200, 30]);
        p.render.encode.codec = Codec::Vp9;
        p.render.encode.crf = Some(40);
        p.render.height = Some(720);
        let back = Preset::from_json(&p.to_json()).unwrap();
        assert_eq!(back, p);
        // Encoder options are flattened into the render section.
        assert!(p.to_json().contains(r#""codec": "vp9""#));
    }

    #[test]
    fn partial_json_uses_defaults() {
        let p = Preset::from_json(r#"{"effect": {"gain": 8}}"#).unwrap();
        assert_eq!(p.effect.gain, 8.0);
        assert_eq!(p.comparison, Comparison::default());
        assert_eq!(p.render, RenderOptions::default());
    }

    #[test]
    fn rejects_invalid() {
        assert!(matches!(
            Preset::from_json(r#"{"version": 99}"#),
            Err(PresetError::Version(99))
        ));
        assert!(matches!(
            Preset::from_json(r#"{"effect": {"threshold": 3}}"#),
            Err(PresetError::Param(_))
        ));
        assert!(matches!(
            Preset::from_json(
                r#"{"comparison": {"mode": "rolling", "offset": {"value": 0, "unit": "frames"}}}"#
            ),
            Err(PresetError::Param(ParamError::ZeroOffset))
        ));
        assert!(matches!(Preset::from_json("{"), Err(PresetError::Json(_))));
    }

    #[test]
    fn save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.json");
        let p = Preset::default();
        p.save(&path).unwrap();
        assert_eq!(Preset::load(&path).unwrap(), p);
        assert!(matches!(
            Preset::load(&dir.path().join("missing.json")),
            Err(PresetError::Io { .. })
        ));
    }
}
