use std::path::Path;

use crate::process::command;
use crate::{Ffmpeg, MediaError, Result};

impl Ffmpeg {
    /// Extracts the first audio stream of `input` as 16-bit stereo 48 kHz WAV (playable by every
    /// browser engine) to `output`.
    pub fn extract_audio_wav(&self, input: &Path, output: &Path) -> Result<()> {
        let out = command(&self.ffmpeg)
            .args(["-hide_banner", "-nostdin", "-v", "error", "-y", "-i"])
            .arg(input)
            .args([
                "-vn", "-sn", "-dn", "-map", "0:a:0", "-ac", "2", "-ar", "48000",
            ])
            .args(["-c:a", "pcm_s16le", "-f", "wav"])
            .arg(output)
            .output()
            .map_err(|source| MediaError::Spawn {
                program: self.ffmpeg.display().to_string(),
                source,
            })?;
        if out.status.success() {
            Ok(())
        } else {
            let _ = std::fs::remove_file(output);
            Err(MediaError::Ffmpeg {
                status: out.status.to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            })
        }
    }
}
