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
    pub gui: Gui,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    pub mode: String,
    pub hotkey: String,
    pub en_hotkey: String,
    pub max_record_secs: u64,
    pub log_transcripts: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Audio {
    pub device: String,
    /// Deprecated: the ASR rate is fixed at 16 kHz (`asr::ASR_RATE`). Kept as a
    /// known-but-ignored field so configs written before 0.1.0 (which carried
    /// `sample_rate`) still parse instead of failing `deny_unknown_fields`.
    #[serde(default)]
    pub sample_rate: Option<u32>,
    pub silence_threshold: f32,
    pub silence_padding_ms: u32,
    pub min_speech_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Asr {
    pub model: String,
    pub en_model: String,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gui {
    pub enabled: bool,
    pub theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            general: General::default(),
            audio: Audio::default(),
            asr: Asr::default(),
            text: Text::default(),
            inject: Inject::default(),
            gui: Gui::default(),
        }
    }
}

impl Default for General {
    fn default() -> Self {
        General {
            mode: "hold".into(),
            hotkey: "ctrl+alt+space".into(),
            en_hotkey: "ctrl+win+alt+space".into(),
            max_record_secs: 120,
            log_transcripts: false,
        }
    }
}

impl Default for Audio {
    fn default() -> Self {
        Audio {
            device: String::new(),
            sample_rate: None,
            silence_threshold: 0.008,
            silence_padding_ms: 200,
            min_speech_ms: 250,
        }
    }
}

impl Default for Asr {
    fn default() -> Self {
        Asr {
            model: "models/vosk-model-small-ru-0.22".into(),
            en_model: "models/vosk-model-small-en-us-0.15".into(),
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

impl Default for Gui {
    fn default() -> Self {
        Gui {
            enabled: true,
            theme: "light".into(),
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
        if !(1..=3600).contains(&self.general.max_record_secs) {
            return Err(Error::Config(
                "general.max_record_secs must be in 1..=3600".into(),
            ));
        }
        crate::hotkey::parse_hotkey(&self.general.hotkey)?;
        crate::hotkey::parse_hotkey(&self.general.en_hotkey)?;
        if self.general.hotkey == self.general.en_hotkey {
            return Err(Error::Config(
                "general.hotkey and general.en_hotkey must differ".into(),
            ));
        }
        if !(0.0..1.0).contains(&self.audio.silence_threshold) {
            return Err(Error::Config(
                "audio.silence_threshold must be in [0, 1)".into(),
            ));
        }
        if self.audio.silence_padding_ms > 5_000 {
            return Err(Error::Config(
                "audio.silence_padding_ms must be <= 5000".into(),
            ));
        }
        if self.audio.min_speech_ms > 60_000 {
            return Err(Error::Config(
                "audio.min_speech_ms must be <= 60000".into(),
            ));
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
        if self.inject.settle_ms > 10_000 {
            return Err(Error::Config("inject.settle_ms must be <= 10000".into()));
        }
        if !matches!(self.gui.theme.as_str(), "dark" | "light") {
            return Err(Error::Config(format!(
                "gui.theme must be 'dark' or 'light', got '{}'",
                self.gui.theme
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

/// Serialize a config back to TOML (used by the GUI to persist edits).
pub fn save(path: &Path, cfg: &Config) -> Result<()> {
    let s = toml::to_string_pretty(cfg)?;
    std::fs::write(path, s)?;
    Ok(())
}

/// Resolve a relative path (e.g. the model) against config dir, cwd, then exe dir.
pub fn resolve_model_path(cfg_dir: &Path, rel: &str) -> PathBuf {
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
    fn invalid_en_hotkey_rejected() {
        let mut c = Config::default();
        c.general.en_hotkey = "ctrl+space+alt".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn identical_hotkeys_rejected() {
        let mut c = Config::default();
        c.general.en_hotkey = c.general.hotkey.clone();
        assert!(c.validate().is_err());
    }

    #[test]
    fn silence_padding_ms_upper_bound() {
        let mut c = Config::default();
        c.audio.silence_padding_ms = 5001;
        assert!(c.validate().is_err());
    }

    #[test]
    fn min_speech_ms_upper_bound() {
        let mut c = Config::default();
        c.audio.min_speech_ms = 60_001;
        assert!(c.validate().is_err());
    }

    #[test]
    fn settle_ms_upper_bound() {
        let mut c = Config::default();
        c.inject.settle_ms = 10_001;
        assert!(c.validate().is_err());
    }

    #[test]
    fn max_record_secs_upper_bound() {
        let mut c = Config::default();
        c.general.max_record_secs = 3601;
        assert!(c.validate().is_err());
    }

    #[test]
    fn legacy_sample_rate_field_is_accepted() {
        // Configs written before 0.1.0 carried `sample_rate`. It must still
        // parse (not be rejected as an unknown field) so existing users do
        // not get a startup crash after upgrading.
        let s = "[audio]\nsample_rate = 16000\n";
        let cfg: Config = toml::from_str(s).unwrap();
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.audio.sample_rate, Some(16_000));
    }

    #[test]
    fn legacy_sample_rate_value_is_ignored() {
        // The field is deprecated (ASR is fixed at 16 kHz), so any value is
        // accepted and ignored rather than failing validation.
        let s = "[audio]\nsample_rate = 48000\n";
        let cfg: Config = toml::from_str(s).unwrap();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn invalid_gui_theme_rejected() {
        let mut c = Config::default();
        c.gui.theme = "blue".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn gui_can_be_disabled() {
        let mut c = Config::default();
        c.gui.enabled = false;
        c.validate().expect("disabled gui must be valid");
    }

    #[test]
    fn gui_theme_defaults_to_light() {
        assert_eq!(Config::default().gui.theme, "light");
    }

    #[test]
    fn save_load_roundtrip() {
        let cfg = Config::default();
        let path = std::env::temp_dir().join(format!(
            "rapcoding-config-roundtrip-{}.toml",
            std::process::id()
        ));
        save(&path, &cfg).expect("save must succeed");
        let loaded = load(&path).expect("load must succeed");
        assert_eq!(loaded, cfg);
        let _ = std::fs::remove_file(&path);
    }
}
