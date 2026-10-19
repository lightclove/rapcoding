use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Canonical default configuration, shipped as an editable template.
pub const DEFAULT_CONFIG: &str = include_str!("../assets/config.toml");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub general: General,
    pub audio: Audio,
    pub asr: Asr,
    pub text: Text,
    pub inject: Inject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    pub mode: String,
    pub hotkey: String,
    pub mixed_hotkey: String,
    pub max_record_secs: u64,
    pub log_transcripts: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Audio {
    pub device: String,
    pub sample_rate: u32,
    pub silence_threshold: f32,
    pub silence_padding_ms: u32,
    pub min_speech_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Asr {
    pub backend: String,
    pub command: String,
    pub model: String,
    pub mixed_backend: String,
    pub mixed_model: String,
    pub language: String,
    pub threads: u32,
    pub timeout_secs: u64,
    pub initial_prompt: String,
    pub keep_wav: bool,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Text {
    pub voice_commands: String,
    pub remove_sound_tags: bool,
    pub strip_punctuation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Inject {
    pub enabled: bool,
    pub method: String,
    pub paste_shortcut: String,
    pub restore_clipboard: bool,
    pub settle_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            general: General::default(),
            audio: Audio::default(),
            asr: Asr::default(),
            text: Text::default(),
            inject: Inject::default(),
        }
    }
}

impl Default for General {
    fn default() -> Self {
        General {
            mode: "hold".into(),
            hotkey: "ctrl+alt+space".into(),
            mixed_hotkey: "ctrl+alt+shift+space".into(),
            max_record_secs: 120,
            log_transcripts: false,
        }
    }
}

impl Default for Audio {
    fn default() -> Self {
        Audio {
            device: String::new(),
            sample_rate: 16_000,
            silence_threshold: 0.008,
            silence_padding_ms: 200,
            min_speech_ms: 250,
        }
    }
}

impl Default for Asr {
    fn default() -> Self {
        Asr {
            backend: "vosk".into(),
            command: "whisper-cli".into(),
            model: "models/vosk-model-small-ru-0.22".into(),
            mixed_backend: "sidecar".into(),
            mixed_model: "models/ggml-small-q5_1.bin".into(),
            language: "ru".into(),
            threads: 0,
            timeout_secs: 60,
            initial_prompt: String::new(),
            keep_wav: false,
            args: vec![
                "-m".into(),
                "{model}".into(),
                "-f".into(),
                "{wav}".into(),
                "-l".into(),
                "{language}".into(),
                "-t".into(),
                "{threads}".into(),
                "-nt".into(),
                "-otxt".into(),
                "-of".into(),
                "{out_base}".into(),
                "--prompt".into(),
                "{prompt}".into(),
                "-np".into(),
            ],
        }
    }
}

impl Default for Text {
    fn default() -> Self {
        Text {
            voice_commands: "safe".into(),
            remove_sound_tags: true,
            strip_punctuation: false,
        }
    }
}

impl Default for Inject {
    fn default() -> Self {
        Inject {
            enabled: true,
            method: "paste".into(),
            paste_shortcut: String::new(),
            restore_clipboard: true,
            settle_ms: 300,
        }
    }
}

impl Config {
    /// Semantic validation beyond serde's shape checks.
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.general.mode.as_str(), "hold" | "toggle") {
            return Err(Error::Config(format!(
                "general.mode must be 'hold' or 'toggle', got '{}'",
                self.general.mode
            )));
        }
        if self.general.max_record_secs == 0 {
            return Err(Error::Config("general.max_record_secs must be > 0".into()));
        }
        if self.audio.sample_rate == 0 || self.audio.sample_rate > 384_000 {
            return Err(Error::Config(format!(
                "audio.sample_rate out of range: {}",
                self.audio.sample_rate
            )));
        }
        if !(0.0..1.0).contains(&self.audio.silence_threshold) {
            return Err(Error::Config(
                "audio.silence_threshold must be in [0, 1)".into(),
            ));
        }
        if !matches!(self.asr.backend.as_str(), "sidecar" | "vosk") {
            return Err(Error::Config(format!(
                "asr.backend '{}' unsupported ('sidecar' | 'vosk')",
                self.asr.backend
            )));
        }
        if !self.general.mixed_hotkey.is_empty()
            && !matches!(self.asr.mixed_backend.as_str(), "sidecar" | "vosk")
        {
            return Err(Error::Config(format!(
                "asr.mixed_backend '{}' unsupported ('sidecar' | 'vosk')",
                self.asr.mixed_backend
            )));
        }
        if self.asr.args.is_empty() {
            return Err(Error::Config("asr.args must not be empty".into()));
        }
        if self.asr.timeout_secs == 0 {
            return Err(Error::Config("asr.timeout_secs must be > 0".into()));
        }
        if !matches!(
            self.text.voice_commands.as_str(),
            "off" | "safe" | "aggressive"
        ) {
            return Err(Error::Config(format!(
                "text.voice_commands must be 'off'|'safe'|'aggressive', got '{}'",
                self.text.voice_commands
            )));
        }
        if !matches!(self.inject.method.as_str(), "paste" | "type") {
            return Err(Error::Config(format!(
                "inject.method must be 'paste' or 'type', got '{}'",
                self.inject.method
            )));
        }
        Ok(())
    }
}

/// Directory that holds `config.toml` and `rapcoding.log`.
pub fn config_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("RAPCODING_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        return base.join("rapcoding");
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        return home
            .join("Library")
            .join("Application Support")
            .join("rapcoding");
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(x).join("rapcoding");
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        return home.join(".config").join("rapcoding");
    }
}

/// Path to the config file: CLI override → `RAPCODING_CONFIG` → platform dir.
pub fn config_path(override_path: Option<&Path>) -> PathBuf {
    if let Some(p) = override_path {
        return p.to_path_buf();
    }
    if let Some(p) = std::env::var_os("RAPCODING_CONFIG") {
        return PathBuf::from(p);
    }
    config_dir().join("config.toml")
}

/// Load config, auto-creating a default file on first run.
pub fn load(path: &Path) -> Result<Config> {
    if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        let cfg: Config =
            toml::from_str(&raw).map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
        cfg.validate()?;
        Ok(cfg)
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, DEFAULT_CONFIG)?;
        Ok(Config::default())
    }
}

/// Resolve a relative path (e.g. the model) against config dir, cwd, then exe dir.
pub fn resolve_sidecar_path(cfg_dir: &Path, rel: &str) -> PathBuf {
    let p = Path::new(rel);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let mut candidates: Vec<PathBuf> = vec![cfg_dir.join(p), PathBuf::from(p)];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(p));
        }
    }
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    cfg_dir.join(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_template_matches_default() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).expect("template must parse");
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let s = "[general]\nmode = \"hold\"\nhotkey = \"f9\"\nmax_record_secs = 10\nlog_transcripts = false\nbogus = 1\n";
        assert!(toml::from_str::<Config>(s).is_err());
    }

    #[test]
    fn invalid_mode_rejected() {
        let mut c = Config::default();
        c.general.mode = "latch".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn defaults_validate() {
        Config::default()
            .validate()
            .expect("defaults must be valid");
    }

    #[test]
    fn invalid_mixed_backend_rejected() {
        let mut c = Config::default();
        c.asr.mixed_backend = "bogus".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn default_prompt_is_empty_to_avoid_hallucination() {
        // Regression: a non-empty --prompt makes whisper.cpp hallucinate on
        // short utterances (observed: "Как ты?" -> "❤️"). Keep it empty.
        assert!(
            Config::default().asr.initial_prompt.is_empty(),
            "default initial_prompt must be empty"
        );
    }
}
