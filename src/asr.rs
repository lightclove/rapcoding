use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use vosk::{Model, Recognizer};

use crate::config::{self, Asr as AsrCfg};
use crate::error::{Error, Result};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Placeholders substituted in `asr.args`.
pub struct Vars<'a> {
    pub model: &'a str,
    pub wav: &'a str,
    pub language: &'a str,
    pub threads: u32,
    pub prompt: &'a str,
    pub out: &'a str,
    pub out_base: &'a str,
}

/// Runs a speech-to-text sidecar process (default: whisper-cli).
#[derive(Clone)]
pub struct SidecarAsr {
    cfg: AsrCfg,
    model: PathBuf,
}

impl SidecarAsr {
    pub fn new(cfg: AsrCfg, cfg_dir: &Path) -> Self {
        let model = config::resolve_sidecar_path(cfg_dir, &cfg.model);
        SidecarAsr { cfg, model }
    }

    pub fn threads(&self) -> u32 {
        if self.cfg.threads > 0 {
            self.cfg.threads
        } else {
            auto_threads()
        }
    }

    /// Fail loudly before any recognition if the tool or model is missing.
    pub fn check(&self) -> Result<()> {
        if !command_available(&self.cfg.command) {
            return Err(Error::Asr(format!(
                "ASR command '{}' not found (asr.command)",
                self.cfg.command
            )));
        }
        if !self.model.exists() {
            return Err(Error::Asr(format!(
                "ASR model '{}' not found (asr.model)",
                self.model.display()
            )));
        }
        Ok(())
    }

    /// Transcribe a 16 kHz mono WAV. Returns the raw text output.
    pub fn transcribe(&self, wav: &Path) -> Result<String> {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let out_base =
            std::env::temp_dir().join(format!("rapcoding-asr-{}-{}", std::process::id(), seq));
        let out_file = out_base.with_extension("txt");

        let vars = Vars {
            model: self.model.to_str().unwrap_or_default(),
            wav: wav.to_str().unwrap_or_default(),
            language: &self.cfg.language,
            threads: self.threads(),
            prompt: &self.cfg.initial_prompt,
            out: out_file.to_str().unwrap_or_default(),
            out_base: out_base.to_str().unwrap_or_default(),
        };
        let args = render_args(&self.cfg.args, &vars);

        let mut child = Command::new(&self.cfg.command)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Asr(format!("failed to start '{}': {e}", self.cfg.command)))?;

        let timeout = Duration::from_secs(self.cfg.timeout_secs.max(1));
        let start = Instant::now();
        let status = loop {
            if let Some(st) = child
                .try_wait()
                .map_err(|e| Error::Asr(format!("wait: {e}")))?
            {
                break st;
            }
            if start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Asr(format!(
                    "ASR timed out after {}s",
                    self.cfg.timeout_secs
                )));
            }
            std::thread::sleep(Duration::from_millis(40));
        };

        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        if let Some(mut o) = child.stdout.take() {
            let _ = o.read_to_end(&mut stdout);
        }
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_end(&mut stderr);
        }

        let text = if out_file.exists() {
            let s = std::fs::read_to_string(&out_file).unwrap_or_default();
            let _ = std::fs::remove_file(&out_file);
            s
        } else {
            String::from_utf8_lossy(&stdout).into_owned()
        };

        if !status.success() {
            let err = String::from_utf8_lossy(&stderr);
            return Err(Error::Asr(format!(
                "ASR exited with {status}; stderr: {}",
                err.trim()
            )));
        }

        Ok(strip_timestamps(&text))
    }
}

/// Replace placeholders in the argument template, dropping a `--prompt`/`-p`
/// flag when its value is empty.
pub fn render_args(template: &[String], vars: &Vars) -> Vec<String> {
    let rendered: Vec<String> = template.iter().map(|a| render_one(a, vars)).collect();
    let mut out = Vec::with_capacity(rendered.len());
    let mut i = 0;
    while i < rendered.len() {
        let a = &rendered[i];
        if (a == "--prompt" || a == "-p") && i + 1 < rendered.len() && rendered[i + 1].is_empty() {
            i += 2;
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    out
}

fn render_one(arg: &str, vars: &Vars) -> String {
    arg.replace("{model}", vars.model)
        .replace("{wav}", vars.wav)
        .replace("{language}", vars.language)
        .replace("{threads}", &vars.threads.to_string())
        .replace("{prompt}", vars.prompt)
        .replace("{out}", vars.out)
        .replace("{out_base}", vars.out_base)
}

/// Remove `[00:00:00.000 --> 00:00:05.000]  text` timestamp prefixes from
/// stdout fallback output, keeping plain lines untouched.
pub fn strip_timestamps(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        match strip_ts_line(t) {
            Some(rest) if !rest.is_empty() => out.push_str(rest),
            Some(_) => {}
            None => out.push_str(t),
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn strip_ts_line(line: &str) -> Option<&str> {
    if !line.starts_with('[') {
        return None;
    }
    let end = line.find(']')?;
    let inner = &line[1..end];
    if inner.contains("-->") {
        Some(line[end + 1..].trim())
    } else {
        None
    }
}

fn auto_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| (n.get() as u32 / 2).max(1))
        .unwrap_or(2)
}

/// Whether `cmd` resolves to an executable (absolute path, or found on PATH).
pub fn command_available(cmd: &str) -> bool {
    if cmd.contains('/') || cmd.contains('\\') {
        return Path::new(cmd).exists();
    }
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat", ".com"]
    } else {
        &[""]
    };
    std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p).any(|d| {
                exts.iter().any(|e| {
                    let full = if e.is_empty() {
                        d.join(cmd)
                    } else {
                        d.join(format!("{cmd}{e}"))
                    };
                    full.is_file()
                })
            })
        })
        .unwrap_or(false)
}

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

/// ASR backend selected by `asr.backend`.
#[derive(Clone)]
pub enum AsrBackend {
    Whisper(SidecarAsr),
    Vosk(VoskAsr),
}

impl AsrBackend {
    pub fn new(cfg: &AsrCfg, cfg_dir: &Path) -> Result<Self> {
        match cfg.backend.as_str() {
            "sidecar" => Ok(AsrBackend::Whisper(SidecarAsr::new(cfg.clone(), cfg_dir))),
            "vosk" => {
                let model_dir = config::resolve_sidecar_path(cfg_dir, &cfg.model);
                Ok(AsrBackend::Vosk(VoskAsr::new(&model_dir)?))
            }
            other => Err(Error::Config(format!(
                "unsupported asr.backend '{other}' ('sidecar' | 'vosk')"
            ))),
        }
    }

    /// Backend for the mixed RU/EN hotkey. Whisper-sidecar fields (command,
    /// args, language, threads, timeout, prompt) are shared with the primary
    /// config; only the backend kind and model differ.
    pub fn new_mixed(cfg: &AsrCfg, cfg_dir: &Path) -> Result<Self> {
        match cfg.mixed_backend.as_str() {
            "sidecar" => {
                let mut c = cfg.clone();
                c.model = cfg.mixed_model.clone();
                Ok(AsrBackend::Whisper(SidecarAsr::new(c, cfg_dir)))
            }
            "vosk" => {
                let model_dir = config::resolve_sidecar_path(cfg_dir, &cfg.mixed_model);
                Ok(AsrBackend::Vosk(VoskAsr::new(&model_dir)?))
            }
            other => Err(Error::Config(format!(
                "unsupported asr.mixed_backend '{other}' ('sidecar' | 'vosk')"
            ))),
        }
    }

    pub fn check(&self) -> Result<()> {
        match self {
            AsrBackend::Whisper(w) => w.check(),
            AsrBackend::Vosk(v) => v.check(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars<'static> {
        Vars {
            model: "m.bin",
            wav: "in.wav",
            language: "auto",
            threads: 4,
            prompt: "hi",
            out: "/tmp/out.txt",
            out_base: "/tmp/out",
        }
    }

    #[test]
    fn render_substitutes_placeholders() {
        let t = vec![
            "-m".into(),
            "{model}".into(),
            "-t".into(),
            "{threads}".into(),
        ];
        let out = render_args(&t, &vars());
        assert_eq!(out, vec!["-m", "m.bin", "-t", "4"]);
    }

    #[test]
    fn render_skips_empty_prompt() {
        let mut v = vars();
        v.prompt = "";
        let t = vec![
            "--prompt".into(),
            "{prompt}".into(),
            "-m".into(),
            "{model}".into(),
        ];
        let out = render_args(&t, &v);
        assert_eq!(out, vec!["-m", "m.bin"]);
    }

    #[test]
    fn strip_timestamps_keeps_plain() {
        let input = "[00:00:00.000 --> 00:00:01.000]   hello\nworld\n";
        assert_eq!(strip_timestamps(input), "hello\nworld");
    }

    #[test]
    fn strip_timestamps_keeps_bracket_tags() {
        let input = "[BLANK_AUDIO]\n";
        assert_eq!(strip_timestamps(input), "[BLANK_AUDIO]");
    }

    #[test]
    fn cargo_is_available() {
        assert!(command_available("cargo"));
    }

    #[test]
    fn nonsense_command_is_not_available() {
        assert!(!command_available(
            "rapcoding-definitely-not-a-real-command"
        ));
    }

    #[test]
    fn samples_to_i16_converts_and_clamps() {
        let out = samples_to_i16(&[0.0f32, 1.0, -1.0, 2.0, -2.0]);
        assert_eq!(out, vec![0, 32767, -32767, 32767, -32767]);
    }
}
