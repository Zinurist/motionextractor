use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Persisted application settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Upper bound for the preview frame cache in MiB.
    pub cache_max_memory_mb: u64,
    /// Height of preview frames; 0 = native resolution.
    pub preview_height: u32,
    /// Parallel ffmpeg processes filling the preview cache.
    pub decode_workers: u32,
    /// Use `-hwaccel auto` for preview decoding.
    pub hwaccel: bool,
    /// JPEG quality for preview frames sent to non-local clients.
    pub jpeg_quality: u8,
    /// Custom ffmpeg / ffprobe executables (take effect after a restart).
    pub ffmpeg_path: Option<PathBuf>,
    pub ffprobe_path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cache_max_memory_mb: 2048,
            preview_height: 540,
            decode_workers: 2,
            hwaccel: false,
            jpeg_quality: 85,
            ffmpeg_path: None,
            ffprobe_path: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("{0}")]
    Invalid(String),
    #[error("cannot access settings file {path}: {message}")]
    Io { path: String, message: String },
}

impl Settings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        let bad = |m: &str| Err(SettingsError::Invalid(m.to_owned()));
        if !(64..=1 << 20).contains(&self.cache_max_memory_mb) {
            return bad("cache_max_memory_mb must be between 64 and 1048576");
        }
        if self.preview_height != 0 && !(120..=4320).contains(&self.preview_height) {
            return bad("preview_height must be 0 (native) or between 120 and 4320");
        }
        if !(1..=16).contains(&self.decode_workers) {
            return bad("decode_workers must be between 1 and 16");
        }
        if !(1..=100).contains(&self.jpeg_quality) {
            return bad("jpeg_quality must be between 1 and 100");
        }
        Ok(())
    }

    pub fn cache_bytes(&self) -> usize {
        (self.cache_max_memory_mb as usize) << 20
    }

    /// Loads settings; a missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        let io = |e: &dyn std::fmt::Display| SettingsError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        };
        match std::fs::read_to_string(path) {
            Ok(json) => {
                let s: Settings = serde_json::from_str(&json).map_err(|e| io(&e))?;
                s.validate()?;
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(io(&e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let io = |e: &dyn std::fmt::Display| SettingsError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io(&e))?;
        }
        let json = serde_json::to_string_pretty(self).expect("serialisable");
        std::fs::write(path, json).map_err(|e| io(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/settings.json");
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        let s = Settings {
            preview_height: 720,
            hwaccel: true,
            ..Default::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
    }

    #[test]
    fn validation() {
        assert!(Settings::default().validate().is_ok());
        assert!(
            Settings {
                preview_height: 0,
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
        assert!(
            Settings {
                preview_height: 50,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                cache_max_memory_mb: 1,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                decode_workers: 0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        std::fs::write(&path, "{\"decode_workers\": 99}").unwrap();
        assert!(matches!(
            Settings::load(&path),
            Err(SettingsError::Invalid(_))
        ));
        std::fs::write(&path, "nope").unwrap();
        assert!(matches!(
            Settings::load(&path),
            Err(SettingsError::Io { .. })
        ));
    }
}
