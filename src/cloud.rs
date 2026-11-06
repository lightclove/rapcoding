//! OpenAI-compatible cloud ASR for the interview binding.
//!
//! The local vosk models understand exactly one language; the interview
//! binding must transcribe RU/EN mixed speech, so it goes to a cloud endpoint
//! (`POST {base_url}/audio/transcriptions`, multipart WAV + model). Any
//! OpenAI-compatible provider works (Groq, OpenAI, Deepgram, …).
//!
//! Privacy contract: audio is sent to the cloud ONLY for the interview
//! binding. The regular RU/EN hotkeys stay fully local.

use std::time::Duration;

use crate::config::Cloud;
use crate::error::{Error, Result};

const BOUNDARY: &str = "----rapcoding-form-boundary-7d2f1c";

/// Multipart body: `file` (WAV bytes) and `model` fields, terminated
/// correctly. Hand-rolled because ureq 2.x has no multipart module.
pub fn build_multipart(wav: &[u8], model: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(wav.len() + 256);
    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"model\"\r\n\r\n{model}\r\n").as_bytes(),
    );
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// Extract `text` from an OpenAI-compatible JSON response. A missing/empty
/// field is an error, not an empty transcript passed off as success.
pub fn parse_transcript(json: &str) -> Result<String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| Error::Cloud(format!("bad JSON: {e}")))?;
    let text = v
        .get("text")
        .and_then(|t| t.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Cloud("response has no 'text' field".into()))?;
    Ok(text)
}

/// Cloud transcription client. `api_key` is never included in error messages
/// or logs.
#[derive(Clone)]
pub struct CloudAsr {
    base_url: String,
    api_key: String,
    model: String,
}

impl CloudAsr {
    pub fn new(cfg: &Cloud) -> Result<Self> {
        if cfg.api_key.trim().is_empty() {
            return Err(Error::Cloud("cloud.api_key is empty".into()));
        }
        if cfg.base_url.trim().is_empty() {
            return Err(Error::Config("cloud.base_url is empty".into()));
        }
        if cfg.model.trim().is_empty() {
            return Err(Error::Config("cloud.model is empty".into()));
        }
        Ok(CloudAsr {
            base_url: cfg.base_url.trim().trim_end_matches('/').to_string(),
            api_key: cfg.api_key.trim().to_string(),
            model: cfg.model.trim().to_string(),
        })
    }

    /// Transcribe 16 kHz mono samples. Network problems and non-200 responses
    /// are loud errors with the status and a body excerpt (sanitized).
    pub fn transcribe(&self, samples_16k: &[f32]) -> Result<String> {
        let wav = crate::wav::wav_bytes(samples_16k);
        let body = build_multipart(&wav, &self.model);
        let url = format!("{}/audio/transcriptions", self.base_url);
        // Long audio can take a while; the read timeout is per-read, so a
        // stalled connection still fails loudly instead of hanging forever.
        let agent = ureq::builder()
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(Duration::from_secs(120))
            .build();
        let resp = agent
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set(
                "Content-Type",
                &format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .send(body.as_slice())
            .map_err(|e| Error::Cloud(format!("POST {url}: {e}")))?;
        let status = resp.status();
        let text = resp
            .into_string()
            .map_err(|e| Error::Cloud(format!("read response: {e}")))?;
        if status != 200 {
            return Err(Error::Cloud(format!(
                "HTTP {status}: {}",
                crate::log::sanitize(&clip(&text, 300))
            )));
        }
        parse_transcript(&text)
    }
}

/// First `n` chars of a (possibly huge) error body.
fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_has_file_and_model_fields() {
        let body = build_multipart(b"RIFFxxxx", "whisper-large-v3");
        let s = String::from_utf8(body.clone()).expect("body is utf8 text + wav bytes");
        assert!(s.starts_with(format!("--{BOUNDARY}\r\n").as_str()), "{s}");
        assert!(
            s.contains("name=\"file\"; filename=\"audio.wav\""),
            "{s}"
        );
        assert!(s.contains("RIFFxxxx"), "{s}");
        assert!(s.contains("name=\"model\""), "{s}");
        assert!(s.contains("whisper-large-v3"), "{s}");
        assert!(s.ends_with(format!("--{BOUNDARY}--\r\n").as_str()), "{s}");
        // The wav payload is framed by CRLFs on both sides.
        assert!(s.contains("audio/wav\r\n\r\nRIFFxxxx\r\n"), "{s}");
    }

    #[test]
    fn parse_transcript_extracts_text() {
        assert_eq!(
            parse_transcript(r#"{"text": "привет hello"}"#).unwrap(),
            "привет hello"
        );
        assert_eq!(
            parse_transcript(r#"{"text":"  hi  ","x_groq":{}}"#).unwrap(),
            "hi"
        );
    }

    #[test]
    fn parse_transcript_rejects_missing_text() {
        assert!(parse_transcript(r#"{"x_groq":{}}"#).is_err());
        assert!(parse_transcript("not json").is_err());
        assert!(parse_transcript(r#"{"text": "   "}"#).is_err());
    }

    #[test]
    fn clip_limits_error_body() {
        assert_eq!(clip("abcdef", 3), "abc");
        assert_eq!(clip("ab", 10), "ab");
    }

    #[test]
    fn new_requires_key_and_url_and_model() {
        let mut cfg = Cloud::default();
        assert!(CloudAsr::new(&cfg).is_err(), "empty key must disable");
        cfg.api_key = "k".into();
        cfg.base_url = String::new();
        assert!(CloudAsr::new(&cfg).is_err(), "empty url must fail");
        cfg.base_url = "https://api.groq.com/openai/v1/".into();
        cfg.model = String::new();
        assert!(CloudAsr::new(&cfg).is_err(), "empty model must fail");
        cfg.model = "whisper-large-v3".into();
        let c = CloudAsr::new(&cfg).unwrap();
        // Trailing slash is stripped (the path is appended by transcribe).
        assert_eq!(c.base_url, "https://api.groq.com/openai/v1");
        // The key is not embedded in any error path here; verify only that it
        // round-trips into the client.
        assert_eq!(c.api_key, "k");
    }
}
