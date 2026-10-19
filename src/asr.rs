use std::path::{Path, PathBuf};
use std::sync::Arc;

use vosk::{Model, Recognizer};

use crate::error::{Error, Result};

/// Convert normalized f32 samples to 16-bit PCM (vosk input format).
pub fn samples_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

/// Vosk backend: local Kaldi-based model, loaded once, very fast.
#[derive(Clone)]
pub struct VoskAsr {
    model: Arc<Model>,
    model_path: PathBuf,
}

impl VoskAsr {
    pub fn new(model_path: &Path) -> Result<Self> {
        vosk::set_log_level(vosk::LogLevel::Error);
        let model = Model::new(model_path.to_string_lossy()).ok_or_else(|| {
            Error::Asr(format!(
                "failed to load vosk model '{}'",
                model_path.display()
            ))
        })?;
        Ok(VoskAsr {
            model: Arc::new(model),
            model_path: model_path.to_path_buf(),
        })
    }

    pub fn check(&self) -> Result<()> {
        if !self.model_path.is_dir() {
            return Err(Error::Asr(format!(
                "vosk model dir '{}' not found",
                self.model_path.display()
            )));
        }
        Ok(())
    }

    /// Transcribe 16 kHz mono samples. Model stays loaded between calls.
    pub fn transcribe_samples(&self, samples_16k: &[f32]) -> Result<String> {
        let pcm = samples_to_i16(samples_16k);
        let mut rec = Recognizer::new(&self.model, 16000.0)
            .ok_or_else(|| Error::Asr("failed to create vosk recognizer".into()))?;
        rec.accept_waveform(&pcm)
            .map_err(|e| Error::Asr(format!("vosk accept_waveform: {e}")))?;
        let text = rec
            .final_result()
            .single()
            .map(|s| s.text.trim().to_string())
            .unwrap_or_default();
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_to_i16_converts_and_clamps() {
        let out = samples_to_i16(&[0.0f32, 1.0, -1.0, 2.0, -2.0]);
        assert_eq!(out, vec![0, 32767, -32767, 32767, -32767]);
    }
}
