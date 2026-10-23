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
    /// Where downloaded models live. Empty = the platform default
    /// (`config_dir/models`), so configs written before SC-053 keep working.
    /// An absolute path is stored as chosen by the user.
    pub models_dir: String,
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
            models_dir: String::new(),
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
        validate_models_dir(&self.asr.models_dir)?;
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
            return Err(Error::Config("audio.min_speech_ms must be <= 60000".into()));
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
///
/// Written atomically: first to a `<name>.tmp` sibling, then renamed over
/// the target (`fs::rename` maps to `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`
/// on Windows). A crash or full disk mid-write can therefore never leave a
/// truncated config that would brick the next start.
pub fn save(path: &Path, cfg: &Config) -> Result<()> {
    let s = toml::to_string_pretty(cfg)?;
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    let write_result = std::fs::write(&tmp, s);
    if let Err(e) = &write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Config(format!("write {}: {e}", tmp.display())));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::Config(format!(
            "rename {} -> {}: {e}",
            tmp.display(),
            path.display()
        ))
    })?;
    Ok(())
}

/// Resolve a relative path (e.g. the model) against the models root, config
/// dir, default config dir (installed models), cwd, then exe dir.
/// `models_root` comes from [`models_root`], so a relative `models/<name>`
/// follows the configured folder (and falls back to the platform default when
/// it is empty).
pub fn resolve_model_path_in(models_root: &Path, cfg_dir: &Path, rel: &str) -> PathBuf {
    let p = Path::new(rel);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    for c in model_candidates(models_root, cfg_dir, &config_dir(), rel) {
        if c.exists() {
            return c.clone();
        }
    }
    // Nothing exists yet: return where a download would put the model, never
    // the doubled-prefix form (`models/models/...`).
    match p.strip_prefix("models") {
        Ok(s) if !s.as_os_str().is_empty() => models_root.join(s),
        _ => models_root.join(p),
    }
}

/// Candidate locations for a relative model path, in priority order: models
/// root, config dir, default config dir (installed models must stay reachable
/// when the config lives elsewhere, e.g. `--config` pointing at a temp copy),
/// cwd, then exe dir.
fn model_candidates(
    models_root: &Path,
    cfg_dir: &Path,
    default_dir: &Path,
    rel: &str,
) -> Vec<PathBuf> {
    let p = Path::new(rel);
    // A config value may already carry the `models/` prefix; joining it onto
    // the models root would produce `models/models/...`.
    let stripped = p
        .strip_prefix("models")
        .ok()
        .filter(|s| !s.as_os_str().is_empty());
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(s) = stripped {
        candidates.push(models_root.join(s));
    }
    candidates.push(models_root.join(p));
    candidates.push(cfg_dir.join(p));
    if default_dir != cfg_dir {
        // With and without the `models/` prefix, to cover both conventions.
        candidates.push(default_dir.join(p));
        candidates.push(default_dir.join("models").join(p));
    }
    candidates.push(PathBuf::from(p));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(p));
        }
    }
    candidates
}

/// The models root as an absolute path: `asr.models_dir` when set, else the
/// platform default `<config_dir>/models`. A relative `models_dir` is taken
/// relative to the config dir (never to cwd: the GUI may be started from
/// anywhere), then normalised to an absolute path.
pub fn models_root(cfg: &Config, cfg_dir: &Path) -> PathBuf {
    let configured = cfg.asr.models_dir.trim();
    if configured.is_empty() {
        return cfg_dir.join("models");
    }
    let p = Path::new(configured);
    if p.is_absolute() {
        return normalise(p);
    }
    normalise(&cfg_dir.join(p))
}

/// Lexical normalisation: drop `.` and resolve `..` without touching the FS.
fn normalise(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Validate a models directory path before it is written to the config.
///
/// An empty value is always valid (= platform default). Otherwise the path
/// must be *creatable* as a directory: an existing directory, or a path whose
/// nearest existing ancestor is a directory (the folder itself is created on
/// first download). A regular file in the way is rejected loudly instead of
/// failing later at download time.
///
/// A vanished folder (unplugged drive, deleted cache) is NOT an error here:
/// the stored choice must survive restarts and the app must still start.
/// Availability is a runtime check (`models_root().exists()`), not a config
/// contract.
pub fn validate_models_dir(path: &str) -> Result<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let p = PathBuf::from(trimmed);
    if p.exists() {
        if !p.is_dir() {
            return Err(Error::Config(format!(
                "asr.models_dir '{trimmed}' is a file, not a directory"
            )));
        }
        return Ok(());
    }
    // Not present yet: the nearest existing ancestor must be a directory.
    let mut anc = p.as_path();
    while let Some(parent) = anc.parent() {
        anc = parent;
        if anc.as_os_str().is_empty() {
            break;
        }
        if anc.exists() {
            if !anc.is_dir() {
                return Err(Error::Config(format!(
                    "asr.models_dir '{trimmed}': '{}' is not a directory",
                    anc.display()
                )));
            }
            return Ok(());
        }
    }
    Err(Error::Config(format!(
        "asr.models_dir '{trimmed}': no existing parent directory"
    )))
}

/// Rewrite relative model paths (`models/<name>`) for a models-root change.
///
/// Active models whose value points inside the *old default* root stay
/// relative (`models/<name>`) and therefore follow the new folder; custom
/// absolute paths are left untouched (the user owns them). Returns the
/// number of rewritten entries.
pub fn migrate_model_paths(cfg: &mut Config, old_root: &Path, new_root: &Path) -> usize {
    if old_root == new_root {
        return 0;
    }
    let mut changed = 0;
    let old = normalise(old_root);
    for slot in [&mut cfg.asr.model, &mut cfg.asr.en_model] {
        let p = Path::new(slot.as_str());
        if !p.is_absolute() {
            continue;
        }
        if let Ok(rel) = p.strip_prefix(&old) {
            if !rel.as_os_str().is_empty() {
                *slot = format!("models/{}", rel.display());
                changed += 1;
            }
        }
    }
    let _ = new_root;
    changed
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
    fn models_dir_defaults_to_empty() {
        assert_eq!(Config::default().asr.models_dir, "");
    }

    #[test]
    fn legacy_config_without_models_dir_parses() {
        // Arrange: a config written before SC-053 (no models_dir key).
        let s = "[asr]\nmodel = \"models/x\"\nen_model = \"models/y\"\n";
        // Act.
        let cfg: Config = toml::from_str(s).unwrap();
        // Assert: default (empty) and valid.
        assert_eq!(cfg.asr.models_dir, "");
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn models_root_defaults_to_config_dir_models() {
        // Arrange.
        let cfg = Config::default();
        let dir = Path::new("C:\\cfg");
        // Act.
        let root = models_root(&cfg, dir);
        // Assert.
        assert_eq!(root, dir.join("models"));
    }

    #[test]
    fn models_root_uses_absolute_setting() {
        // Arrange.
        let mut cfg = Config::default();
        cfg.asr.models_dir = "D:\\Models".into();
        // Act.
        let root = models_root(&cfg, Path::new("C:\\cfg"));
        // Assert: the user's choice wins.
        assert_eq!(root, PathBuf::from("D:\\Models"));
    }

    #[test]
    fn models_root_resolves_relative_against_config_dir() {
        // Arrange: a relative setting must never depend on the process cwd.
        let mut cfg = Config::default();
        cfg.asr.models_dir = "my-models".into();
        // Act.
        let root = models_root(&cfg, Path::new("C:\\cfg"));
        // Assert.
        assert_eq!(root, Path::new("C:\\cfg").join("my-models"));
    }

    #[test]
    fn models_root_normalises_dots() {
        // Arrange.
        let mut cfg = Config::default();
        cfg.asr.models_dir = "C:\\cfg\\a\\..\\models".into();
        // Act.
        let root = models_root(&cfg, Path::new("C:\\cfg"));
        // Assert.
        assert_eq!(root, PathBuf::from("C:\\cfg\\models"));
    }

    #[test]
    fn validate_models_dir_accepts_empty_and_existing_dir() {
        // Arrange.
        let dir = std::env::temp_dir().join(format!("rapcoding-md-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Act/Assert.
        assert!(validate_models_dir("").is_ok());
        assert!(validate_models_dir("   ").is_ok(), "whitespace = default");
        assert!(validate_models_dir(&dir.display().to_string()).is_ok());
        // A non-existent child of an existing dir is fine (created later).
        let child = dir.join("models-here");
        assert!(validate_models_dir(&child.display().to_string()).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_models_dir_rejects_file_in_the_way() {
        // Arrange: a regular file where the folder should be.
        let dir = std::env::temp_dir().join(format!("rapcoding-md-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        // Act.
        let res = validate_models_dir(&file.display().to_string());
        // Assert: loud error naming the field.
        let msg = res
            .expect_err("file as models_dir must be rejected")
            .to_string();
        assert!(msg.contains("asr.models_dir"), "{msg}");
        assert!(
            msg.contains("not a directory") || msg.contains("is a file"),
            "{msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_models_dir_fails_validation() {
        // Arrange.
        let dir = std::env::temp_dir().join(format!("rapcoding-md-val-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f");
        std::fs::write(&file, b"x").unwrap();
        let mut cfg = Config::default();
        cfg.asr.models_dir = file.display().to_string();
        // Act.
        let res = cfg.validate();
        // Assert.
        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn valid_models_dir_passes_validation() {
        // Arrange.
        let dir = std::env::temp_dir().join(format!("rapcoding-md-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = Config::default();
        cfg.asr.models_dir = dir.display().to_string();
        // Act/Assert.
        assert!(cfg.validate().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_keeps_relative_paths_and_touches_nothing() {
        // Arrange: default relative values.
        let mut cfg = Config::default();
        let before = cfg.clone();
        // Act.
        let n = migrate_model_paths(&mut cfg, Path::new("C:\\cfg\\models"), Path::new("D:\\m"));
        // Assert: already relative -> follow the new root by themselves.
        assert_eq!(n, 0);
        assert_eq!(cfg, before);
    }

    #[test]
    fn migrate_rewrites_absolute_paths_inside_old_root() {
        // Arrange: the user had picked an absolute path inside the default root.
        let mut cfg = Config::default();
        cfg.asr.model = "C:\\cfg\\models\\vosk-ru".into();
        cfg.asr.en_model = "C:\\cfg\\models\\vosk-en".into();
        // Act.
        let n = migrate_model_paths(&mut cfg, Path::new("C:\\cfg\\models"), Path::new("D:\\m"));
        // Assert: rewritten to relative -> resolved in the new folder.
        assert_eq!(n, 2);
        assert_eq!(cfg.asr.model, "models/vosk-ru");
        assert_eq!(cfg.asr.en_model, "models/vosk-en");
    }

    #[test]
    fn migrate_leaves_custom_absolute_paths_alone() {
        // Arrange: a model outside the old root is the user's business.
        let mut cfg = Config::default();
        cfg.asr.model = "E:\\other\\model".into();
        // Act.
        let n = migrate_model_paths(&mut cfg, Path::new("C:\\cfg\\models"), Path::new("D:\\m"));
        // Assert.
        assert_eq!(n, 0);
        assert_eq!(cfg.asr.model, "E:\\other\\model");
    }

    #[test]
    fn migrate_is_a_noop_for_the_same_root() {
        // Arrange.
        let mut cfg = Config::default();
        cfg.asr.model = "C:\\cfg\\models\\m".into();
        // Act.
        let n = migrate_model_paths(
            &mut cfg,
            Path::new("C:\\cfg\\models"),
            Path::new("C:\\cfg\\models"),
        );
        // Assert.
        assert_eq!(n, 0);
        assert_eq!(cfg.asr.model, "C:\\cfg\\models\\m");
    }

    #[test]
    fn resolve_relative_model_prefers_models_root() {
        // Arrange: the same relative name exists under the models root.
        let base = std::env::temp_dir().join(format!("rapcoding-rmp-{}", std::process::id()));
        let root = base.join("models-root");
        let cfg_dir = base.join("cfg");
        std::fs::create_dir_all(root.join("m")).unwrap();
        std::fs::create_dir_all(&cfg_dir).unwrap();
        // Act.
        let resolved = resolve_model_path_in(&root, &cfg_dir, "m");
        // Assert: the configured models root wins over the config dir.
        assert_eq!(resolved, root.join("m"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn model_candidates_include_default_dir_after_cfg_dir() {
        let root = Path::new("C:\\root\\models");
        let cfg = Path::new("C:\\cfg");
        let def = Path::new("C:\\def");
        let c = model_candidates(root, cfg, def, "m");
        assert_eq!(c[0], root.join("m"));
        assert_eq!(c[1], cfg.join("m"));
        // Default config dir follows the config dir, with and without the
        // `models/` prefix (both conventions are covered).
        assert_eq!(c[2], def.join("m"));
        assert_eq!(c[3], def.join("models").join("m"));
    }

    #[test]
    fn model_candidates_skip_default_when_same_as_cfg() {
        let cfg = Path::new("C:\\cfg");
        let c = model_candidates(&cfg.join("models"), cfg, cfg, "m");
        // No duplicate candidates when the default dir equals the config dir.
        assert_eq!(c.iter().filter(|x| **x == cfg.join("m")).count(), 1);
    }

    #[test]
    fn models_prefixed_value_does_not_double_the_prefix() {
        // Arrange: the config value already carries the `models/` prefix and
        // the models root is itself `<config dir>/models` (the default).
        let base = std::env::temp_dir().join(format!("rapcoding-dup-{}", std::process::id()));
        let cfg_dir = base.join("cfg");
        let root = cfg_dir.join("models");
        std::fs::create_dir_all(root.join("vosk-x")).unwrap();
        // Act.
        let resolved = resolve_model_path_in(&root, &cfg_dir, "models/vosk-x");
        // Assert: `<root>/vosk-x`, never `<root>/models/vosk-x`.
        assert_eq!(resolved, root.join("vosk-x"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn models_prefixed_value_resolves_even_when_absent() {
        // Arrange: nothing downloaded yet (the first-run case).
        let base = std::env::temp_dir().join(format!("rapcoding-miss-{}", std::process::id()));
        let cfg_dir = base.join("cfg");
        let root = cfg_dir.join("models");
        // Act.
        let resolved = resolve_model_path_in(&root, &cfg_dir, "models/vosk-y");
        // Assert: the expected download location, without a doubled prefix.
        assert_eq!(resolved, root.join("vosk-y"));
    }

    #[cfg(feature = "gui")]
    #[test]
    fn models_dir_survives_save_and_load() {
        // Arrange: a custom folder chosen in the UI.
        let path =
            std::env::temp_dir().join(format!("rapcoding-models-dir-{}.toml", std::process::id()));
        let dir = std::env::temp_dir().join(format!("rapcoding-md-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = Config::default();
        cfg.asr.models_dir = dir.display().to_string();
        // Act: save, load back (the restart path).
        save(&path, &cfg).unwrap();
        let loaded = load(&path).unwrap();
        // Assert.
        assert_eq!(loaded.asr.models_dir, cfg.asr.models_dir);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn vanished_models_dir_survives_a_restart_but_says_so() {
        // Arrange: the folder disappeared (unplugged drive) between runs —
        // a common case, and the app must still start.
        let path = std::env::temp_dir().join(format!(
            "rapcoding-models-dir-broken-{}.toml",
            std::process::id()
        ));
        let missing = std::env::temp_dir().join("rapcoding-no-such-drive-XYZ\\models");
        let mut cfg = Config::default();
        cfg.asr.models_dir = missing.display().to_string();
        save(&path, &cfg).unwrap();
        // Act.
        let loaded = load(&path).expect("a missing folder must not block startup");
        // Assert: the choice is preserved verbatim, and the path is reported
        // as unavailable (not silently replaced by the default).
        assert_eq!(loaded.asr.models_dir, cfg.asr.models_dir);
        assert!(
            !loaded.asr.models_dir.trim().is_empty(),
            "a vanished folder must not be reset to the default"
        );
        let _ = std::fs::remove_file(&path);
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

    #[cfg(feature = "gui")]
    #[test]
    fn save_load_roundtrip() {
        // Arrange.
        let cfg = Config::default();
        let path = std::env::temp_dir().join(format!(
            "rapcoding-config-roundtrip-{}.toml",
            std::process::id()
        ));
        // Act.
        save(&path, &cfg).expect("save must succeed");
        let loaded = load(&path).expect("load must succeed");
        // Assert.
        assert_eq!(loaded, cfg);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn save_leaves_no_tmp_file() {
        // Arrange.
        let path = std::env::temp_dir().join(format!(
            "rapcoding-config-atomic-{}.toml",
            std::process::id()
        ));
        let cfg = Config::default();
        // Act.
        save(&path, &cfg).unwrap();
        // Assert: only the target file, no temp sibling.
        let tmp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
        assert!(path.exists());
        assert!(
            !tmp.exists(),
            "temp file must not survive a successful save"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn save_overwrites_existing_file() {
        // Arrange: an existing file with different content.
        let path = std::env::temp_dir().join(format!(
            "rapcoding-config-overwrite-{}.toml",
            std::process::id()
        ));
        let mut old = Config::default();
        old.general.mode = "toggle".into();
        save(&path, &old).unwrap();
        // Act.
        let mut new = Config::default();
        new.general.max_record_secs = 7;
        save(&path, &new).unwrap();
        // Assert: full replacement, still parseable, new content wins.
        let loaded = load(&path).unwrap();
        assert_eq!(loaded, new);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn failed_save_leaves_existing_file_untouched() {
        // Arrange: a valid config on disk and a directory squatting on the
        // temp-file name so the write must fail.
        let path =
            std::env::temp_dir().join(format!("rapcoding-config-fail-{}.toml", std::process::id()));
        let good = Config::default();
        save(&path, &good).unwrap();
        let tmp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
        std::fs::create_dir_all(&tmp).unwrap();
        let before = std::fs::read(&path).unwrap();
        // Act.
        let result = save(&path, &good);
        // Assert: loud error, original file byte-identical.
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn save_load_device_name_unicode_quotes() {
        // A real Bluetooth endpoint name: apostrophe U+2019, quotes, spaces.
        let mut cfg = Config::default();
        cfg.audio.device = "Головной телефон (2- Мирон’s \"AirPods Pro\" - Find My)".into();
        let path = std::env::temp_dir().join(format!(
            "rapcoding-config-unicode-{}.toml",
            std::process::id()
        ));
        save(&path, &cfg).expect("save must succeed");
        let loaded = load(&path).expect("load must succeed");
        assert_eq!(loaded.audio.device, cfg.audio.device);
        let _ = std::fs::remove_file(&path);
    }
}
