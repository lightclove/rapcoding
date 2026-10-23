//! Settings UI logic, framework-independent (no egui), so it can be unit
//! tested without a window: field mapping, draft validation, external-edit
//! detection/merge, and the save pipeline.
//!
//! The egui page in `gui.rs` is a thin renderer over these functions; the
//! GUI is on by default, so this module is built unconditionally.

use std::path::Path;

use crate::config::{self, Config};
use crate::error::Result;
use crate::hotkey;

/// UI control kind for a config field.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WidgetKind {
    Bool,
    Choice(&'static [&'static str]),
    Float {
        min: f32,
        max: f32,
    },
    UInt {
        min: u64,
        max: u64,
    },
    Text,
    Hotkey,
    /// Filesystem directory; rendered as a text field + «Выбрать…» dialog.
    Dir,
}

/// Declarative mapping config field -> UI control. `id` must equal the
/// dotted TOML key; `settings_fields_cover_all_toml_keys` pins this set
/// against the serialized schema, so the UI can never silently diverge from
/// the config file.
#[derive(Clone, Copy, Debug)]
pub struct FieldMeta {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub kind: WidgetKind,
    /// The daemon reads this field only at startup -> applies after restart.
    pub restart: bool,
}

pub const SETTINGS_FIELDS: &[FieldMeta] = &[
    FieldMeta { id: "general.mode", label: "Режим записи", hint: "hold — запись, пока держите хоткей; toggle — одно нажатие включает, второе выключает", kind: WidgetKind::Choice(&["hold", "toggle"]), restart: true },
    FieldMeta { id: "general.hotkey", label: "Хоткей (русский)", hint: "Например: f9, ctrl+alt+space. Клавишу Win виджет захвата не ловит — впишите вручную", kind: WidgetKind::Hotkey, restart: true },
    FieldMeta { id: "general.en_hotkey", label: "Хоткей (английский)", hint: "Должен отличаться от русского хоткея", kind: WidgetKind::Hotkey, restart: true },
    FieldMeta { id: "general.max_record_secs", label: "Макс. длительность записи, с", hint: "Авто-остановка, чтобы запись не шла бесконечно", kind: WidgetKind::UInt { min: 1, max: 3600 }, restart: false },
    FieldMeta { id: "general.log_transcripts", label: "Писать расшифровки в лог", hint: "Приватность: по умолчанию текст распознавания в лог не попадает", kind: WidgetKind::Bool, restart: false },
    FieldMeta { id: "audio.device", label: "Устройство ввода", hint: "Пусто — системное по умолчанию; иначе подстрока имени. Удобный выбор — на странице «Звук»", kind: WidgetKind::Text, restart: true },
    FieldMeta { id: "audio.silence_threshold", label: "Порог тишины", hint: "Доля амплитуды 0..1: тише — считается тишиной", kind: WidgetKind::Float { min: 0.0, max: 0.999 }, restart: false },
    FieldMeta { id: "audio.silence_padding_ms", label: "Паузы вокруг речи, мс", hint: "Сколько тишины оставлять по краям фразы", kind: WidgetKind::UInt { min: 0, max: 5000 }, restart: false },
    FieldMeta { id: "audio.min_speech_ms", label: "Минимум речи, мс", hint: "Более короткие звуки отбрасываются как шум", kind: WidgetKind::UInt { min: 0, max: 60000 }, restart: false },
    FieldMeta { id: "asr.model", label: "Модель (русский)", hint: "Путь к распакованной модели; активация — на странице «Модели»", kind: WidgetKind::Text, restart: true },
    FieldMeta { id: "asr.en_model", label: "Модель (английский)", hint: "Загружается при первом использовании английского хоткея", kind: WidgetKind::Text, restart: true },
    FieldMeta { id: "asr.models_dir", label: "Папка моделей", hint: "Куда скачивать модели. Пусто — по умолчанию (<config dir>/models). Выбор — кнопкой «Выбрать…»", kind: WidgetKind::Dir, restart: true },
    FieldMeta { id: "text.voice_commands", label: "Голосовые команды", hint: "off — выключены; safe — многословные; aggressive — + однословные", kind: WidgetKind::Choice(&["off", "safe", "aggressive"]), restart: false },
    FieldMeta { id: "text.remove_sound_tags", label: "Убирать звуковые теги", hint: "[звук], [шум] и т.п. удаляются из текста", kind: WidgetKind::Bool, restart: false },
    FieldMeta { id: "text.strip_punctuation", label: "Убирать пунктуацию", hint: "Только слова: точки, запятые, кавычки удаляются", kind: WidgetKind::Bool, restart: false },
    FieldMeta { id: "inject.enabled", label: "Вставлять текст", hint: "Выключено — распознавание работает, но текст не печатается", kind: WidgetKind::Bool, restart: true },
    FieldMeta { id: "inject.method", label: "Способ ввода", hint: "paste — через буфер обмена; type — посимвольная эмуляция клавиатуры", kind: WidgetKind::Choice(&["paste", "type"]), restart: true },
    FieldMeta { id: "inject.paste_shortcut", label: "Свой шорткат вставки", hint: "Пусто — ctrl+v; например ctrl+shift+v для терминалов", kind: WidgetKind::Text, restart: true },
    FieldMeta { id: "inject.restore_clipboard", label: "Восстанавливать буфер", hint: "После вставки вернуть прежнее содержимое буфера обмена", kind: WidgetKind::Bool, restart: true },
    FieldMeta { id: "inject.settle_ms", label: "Пауза после вставки, мс", hint: "Ожидание перед восстановлением буфера", kind: WidgetKind::UInt { min: 0, max: 10000 }, restart: true },
    FieldMeta { id: "gui.enabled", label: "Окно при запуске", hint: "Выключено — консольный режим: окно не откроется при старте (вернуть: rapcoding --gui)", kind: WidgetKind::Bool, restart: true },
    FieldMeta { id: "gui.theme", label: "Тема", hint: "Переключается и в шапке окна", kind: WidgetKind::Choice(&["light", "dark"]), restart: false },
];

/// Note: the deprecated `audio.sample_rate` (ignored, kept for old configs)
/// is deliberately absent from the UI.
pub const SETTINGS_SECTIONS: &[(&str, &str)] = &[
    ("general", "Общие"),
    ("audio", "Запись"),
    ("asr", "Модели распознавания"),
    ("text", "Обработка текста"),
    ("inject", "Ввод текста"),
    ("gui", "Интерфейс"),
];

/// Live view of one config field, for the widgets.
#[derive(Debug)]
pub enum FieldValue<'a> {
    Bool(&'a mut bool),
    Str(&'a mut String),
    F32(&'a mut f32),
    U32(&'a mut u32),
    U64(&'a mut u64),
}

/// Resolve a dotted field id to the live value. `None` for unknown ids —
/// callers show a loud error instead of silently skipping a field.
pub fn field_mut<'a>(cfg: &'a mut Config, id: &str) -> Option<FieldValue<'a>> {
    Some(match id {
        "general.mode" => FieldValue::Str(&mut cfg.general.mode),
        "general.hotkey" => FieldValue::Str(&mut cfg.general.hotkey),
        "general.en_hotkey" => FieldValue::Str(&mut cfg.general.en_hotkey),
        "general.max_record_secs" => FieldValue::U64(&mut cfg.general.max_record_secs),
        "general.log_transcripts" => FieldValue::Bool(&mut cfg.general.log_transcripts),
        "audio.device" => FieldValue::Str(&mut cfg.audio.device),
        "audio.silence_threshold" => FieldValue::F32(&mut cfg.audio.silence_threshold),
        "audio.silence_padding_ms" => FieldValue::U32(&mut cfg.audio.silence_padding_ms),
        "audio.min_speech_ms" => FieldValue::U32(&mut cfg.audio.min_speech_ms),
        "asr.model" => FieldValue::Str(&mut cfg.asr.model),
        "asr.en_model" => FieldValue::Str(&mut cfg.asr.en_model),
        "asr.models_dir" => FieldValue::Str(&mut cfg.asr.models_dir),
        "text.voice_commands" => FieldValue::Str(&mut cfg.text.voice_commands),
        "text.remove_sound_tags" => FieldValue::Bool(&mut cfg.text.remove_sound_tags),
        "text.strip_punctuation" => FieldValue::Bool(&mut cfg.text.strip_punctuation),
        "inject.enabled" => FieldValue::Bool(&mut cfg.inject.enabled),
        "inject.method" => FieldValue::Str(&mut cfg.inject.method),
        "inject.paste_shortcut" => FieldValue::Str(&mut cfg.inject.paste_shortcut),
        "inject.restore_clipboard" => FieldValue::Bool(&mut cfg.inject.restore_clipboard),
        "inject.settle_ms" => FieldValue::U64(&mut cfg.inject.settle_ms),
        "gui.enabled" => FieldValue::Bool(&mut cfg.gui.enabled),
        "gui.theme" => FieldValue::Str(&mut cfg.gui.theme),
        _ => return None,
    })
}

/// The edit buffer of the settings page: a working copy of the config plus
/// bookkeeping. Nothing here touches the disk until «Сохранить».
pub struct SettingsDraft {
    pub cfg: Config,
    pub dirty: bool,
    /// Field-attributed errors from the last validation: (id, message).
    pub errors: Vec<(String, String)>,
    /// Non-attributable error from the last validation or write.
    pub global_error: Option<String>,
    /// `baseline` was loaded/reloaded from the file: external edits are
    /// merged into the draft rather than blocking it.
    pub fresh: bool,
    /// The file changed while the draft had unsaved edits -> save blocked.
    pub stale: bool,
    /// What the file was last known to contain.
    pub baseline: Config,
    /// UI state of the models-folder row (persisted in nothing: it is a
    /// one-shot user choice for the next save).
    pub move_models: bool,
    /// (old_root, new_root) to move installed models between, after save.
    pub pending_move: Option<(String, String)>,
}

impl SettingsDraft {
    pub fn new(cfg: &Config) -> Self {
        SettingsDraft {
            cfg: cfg.clone(),
            dirty: false,
            errors: Vec::new(),
            global_error: None,
            fresh: false,
            stale: false,
            baseline: cfg.clone(),
            move_models: false,
            pending_move: None,
        }
    }

    /// Flush draft bookkeeping after the draft was written to disk.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
        self.errors.clear();
        self.global_error = None;
        self.fresh = false;
        self.stale = false;
        self.baseline = self.cfg.clone();
        self.pending_move = None;
    }
}

pub fn can_save(d: &SettingsDraft) -> bool {
    d.dirty && !d.stale
}

/// Copy fields also edited from other pages (header theme, Sound device,
/// Models activation) from the live config, so «Сохранить» cannot silently
/// revert a change made elsewhere while the draft was open. A draft that is
/// itself `fresh` (just reloaded) already holds the file's values.
pub fn sync_owned(d: &mut SettingsDraft, live: &Config) {
    if d.fresh {
        return;
    }
    d.cfg.gui.theme = live.gui.theme.clone();
    d.cfg.audio.device = live.audio.device.clone();
    d.cfg.asr.model = live.asr.model.clone();
    d.cfg.asr.en_model = live.asr.en_model.clone();
}

/// Validate a draft: hotkey syntax errors are attributed to their row;
/// everything else surfaces as one global error (same contract as
/// `rapcoding check`).
pub fn draft_check(cfg: &Config) -> (Vec<(String, String)>, Option<String>) {
    let mut fields: Vec<(String, String)> = Vec::new();
    if let Err(e) = hotkey::parse_hotkey(&cfg.general.hotkey) {
        fields.push(("general.hotkey".into(), e.to_string()));
    }
    if let Err(e) = hotkey::parse_hotkey(&cfg.general.en_hotkey) {
        fields.push(("general.en_hotkey".into(), e.to_string()));
    }
    if !fields.is_empty() {
        // Config::validate stops at the first hotkey error; it would duplicate
        // the message already attributed above.
        return (fields, None);
    }
    match cfg.validate() {
        Ok(()) => (fields, None),
        Err(e) => {
            let msg = e.to_string();
            // "hotkey and en_hotkey must differ" concerns both rows.
            if msg.contains("hotkey and general.en_hotkey must differ") {
                fields.push(("general.en_hotkey".into(), msg));
                return (fields, None);
            }
            (fields, Some(msg))
        }
    }
}

/// Read the config from disk while a draft may hold edits.
///
/// - No unsaved edits: the draft is replaced by the file content (reloaded).
/// - Unsaved edits: merge per field — fields edited in the draft are kept,
///   the rest follow the file — and mark the draft `dirty` again.
///
/// An unreadable/invalid file is a loud error: the draft stays untouched and
/// nothing is merged silently.
pub fn merge_reload(d: &mut SettingsDraft, path: &Path) -> Result<()> {
    let file = config::load(path)?;
    if !d.dirty && !d.stale {
        d.cfg = file.clone();
        d.baseline = file.clone();
        d.fresh = true;
        d.errors.clear();
        d.global_error = None;
        return Ok(());
    }
    let mut merged = file.clone();
    for m in SETTINGS_FIELDS {
        let in_draft_edited =
            field_differs(&d.cfg, m.id, &d.baseline) && field_differs(&d.cfg, m.id, &file);
        if in_draft_edited {
            copy_field(&mut merged, &d.cfg, m.id);
        }
    }
    d.cfg = merged;
    d.dirty = true;
    d.fresh = false;
    d.stale = false;
    d.errors.clear();
    d.global_error = None;
    Ok(())
}

/// True when the value of field `id` differs between two configs.
pub fn field_differs(a: &Config, id: &str, b: &Config) -> bool {
    let mut ac = a.clone();
    let mut bc = b.clone();
    let (Some(va), Some(vb)) = (field_mut(&mut ac, id), field_mut(&mut bc, id)) else {
        return true;
    };
    match (va, vb) {
        (FieldValue::Bool(x), FieldValue::Bool(y)) => *x != *y,
        (FieldValue::Str(x), FieldValue::Str(y)) => *x != *y,
        (FieldValue::F32(x), FieldValue::F32(y)) => *x != *y,
        (FieldValue::U32(x), FieldValue::U32(y)) => *x != *y,
        (FieldValue::U64(x), FieldValue::U64(y)) => *x != *y,
        _ => true,
    }
}

/// Copy one field between configs (no-op when the id is unknown).
fn copy_field(dst: &mut Config, src: &Config, id: &str) {
    let mut s = src.clone();
    let (Some(vs), Some(vd)) = (field_mut(&mut s, id), field_mut(dst, id)) else {
        return;
    };
    match (vs, vd) {
        (FieldValue::Bool(s), FieldValue::Bool(d)) => *d = *s,
        (FieldValue::Str(s), FieldValue::Str(d)) => *d = s.clone(),
        (FieldValue::F32(s), FieldValue::F32(d)) => *d = *s,
        (FieldValue::U32(s), FieldValue::U32(d)) => *d = *s,
        (FieldValue::U64(s), FieldValue::U64(d)) => *d = *s,
        _ => {}
    }
}

/// Existing file content compared with the draft's history: who changed what
/// since the draft was opened? `Ok(None)` when the file is unchanged.
pub fn external_change(d: &SettingsDraft, current: &Config) -> Option<Vec<&'static str>> {
    let changed: Vec<&'static str> = SETTINGS_FIELDS
        .iter()
        .filter(|m| field_differs(current, m.id, &d.baseline))
        .map(|m| m.id)
        .collect();
    if changed.is_empty() {
        None
    } else {
        Some(changed)
    }
}

/// A file edited outside while unsaved edits exist must not be overwritten
/// silently. Returns true when saving is unsafe.
pub fn is_stale(d: &SettingsDraft, current: &Config) -> bool {
    d.dirty && external_change(d, current).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rapcoding-settings-{}-{name}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn settings_fields_cover_all_toml_keys() {
        // Arrange: dotted keys of the serialized default config.
        let toml_s = toml::to_string_pretty(&Config::default()).unwrap();
        let root: toml::Value = toml::from_str(&toml_s).unwrap();
        let mut keys = std::collections::BTreeSet::new();
        fn walk(prefix: &str, v: &toml::Value, keys: &mut std::collections::BTreeSet<String>) {
            match v {
                toml::Value::Table(t) => {
                    for (k, val) in t {
                        walk(&format!("{prefix}{k}."), val, keys);
                    }
                }
                _ => {
                    keys.insert(prefix.trim_end_matches('.').to_string());
                }
            }
        }
        walk("", &root, &mut keys);
        // Act.
        let meta: std::collections::BTreeSet<String> =
            SETTINGS_FIELDS.iter().map(|m| m.id.to_string()).collect();
        // Assert: the UI mapping equals the schema exactly (no drift).
        assert_eq!(
            keys, meta,
            "settings UI must map every config key exactly once"
        );
    }

    #[test]
    fn label_column_fits_narrow_window() {
        // Arrange/Act: the fixed label column plus the widest control budget.
        let labels = SETTINGS_FIELDS.iter().map(|m| m.label.chars().count());
        // Assert: labels stay short enough for the 1000-1200 px windows the
        // app opens with (210 px label column, 13 px font).
        for m in SETTINGS_FIELDS {
            assert!(m.label.chars().count() <= 32, "label too wide: {}", m.id);
            assert!(!m.hint.is_empty(), "hint missing: {}", m.id);
        }
        assert!(labels.max().unwrap() > 0);
    }

    #[test]
    fn field_mut_resolves_every_meta_id() {
        // Arrange.
        let mut cfg = Config::default();
        // Act/Assert: every declared id resolves; unknown/deprecated do not.
        for m in SETTINGS_FIELDS {
            assert!(field_mut(&mut cfg, m.id).is_some(), "{} must resolve", m.id);
        }
        assert!(field_mut(&mut cfg, "general.bogus").is_none());
        assert!(
            field_mut(&mut cfg, "audio.sample_rate").is_none(),
            "deprecated field must stay hidden"
        );
    }

    #[test]
    fn numeric_ui_bounds_match_validation_edges() {
        // Arrange: UI bounds must sit exactly on the validation contract
        // (inside passes, the next step out fails).
        for m in SETTINGS_FIELDS {
            let mut c = Config::default();
            {
                match (m.kind, field_mut(&mut c, m.id)) {
                    (WidgetKind::Float { min, .. }, Some(FieldValue::F32(v))) => *v = min,
                    (WidgetKind::UInt { min, .. }, Some(FieldValue::U32(v))) => *v = min as u32,
                    (WidgetKind::UInt { min, .. }, Some(FieldValue::U64(v))) => *v = min,
                    _ => continue,
                }
            }
            assert!(c.validate().is_ok(), "{} min must validate", m.id);
            {
                match (m.kind, field_mut(&mut c, m.id)) {
                    (WidgetKind::Float { max, .. }, Some(FieldValue::F32(v))) => *v = max,
                    (WidgetKind::UInt { max, .. }, Some(FieldValue::U32(v))) => *v = max as u32,
                    (WidgetKind::UInt { max, .. }, Some(FieldValue::U64(v))) => *v = max,
                    _ => continue,
                }
            }
            assert!(c.validate().is_ok(), "{} max must validate", m.id);
            {
                match (m.kind, field_mut(&mut c, m.id)) {
                    (WidgetKind::Float { max, .. }, Some(FieldValue::F32(v))) => *v = max + 0.1,
                    (WidgetKind::UInt { max, .. }, Some(FieldValue::U32(v))) => *v = max as u32 + 1,
                    (WidgetKind::UInt { max, .. }, Some(FieldValue::U64(v))) => *v = max + 1,
                    _ => continue,
                }
            }
            assert!(c.validate().is_err(), "{} beyond max must fail", m.id);
        }
    }

    #[test]
    fn choice_options_validate() {
        // Arrange: every allowed option must pass the config contract.
        for m in SETTINGS_FIELDS {
            if let WidgetKind::Choice(opts) = m.kind {
                for opt in opts {
                    let mut c = Config::default();
                    if let Some(FieldValue::Str(v)) = field_mut(&mut c, m.id) {
                        *v = opt.to_string();
                        assert!(c.validate().is_ok(), "{} = {} must validate", m.id, opt);
                    }
                }
            }
        }
    }

    #[test]
    fn live_restart_contract_matches_spec() {
        // Arrange: the spec (Q1 = Option B) pins the live subset.
        let expected_live: std::collections::BTreeSet<&str> = [
            "general.max_record_secs",
            "general.log_transcripts",
            "audio.silence_threshold",
            "audio.silence_padding_ms",
            "audio.min_speech_ms",
            "text.voice_commands",
            "text.remove_sound_tags",
            "text.strip_punctuation",
            "gui.theme", // GUI-applied instantly; not daemon state
        ]
        .into();
        // Act.
        let live: std::collections::BTreeSet<&str> = SETTINGS_FIELDS
            .iter()
            .filter(|m| !m.restart)
            .map(|m| m.id)
            .collect();
        // Assert.
        assert_eq!(live, expected_live, "restart flags drifted from the spec");
    }

    #[test]
    fn draft_save_logic() {
        // Arrange.
        let mut d = SettingsDraft::new(&Config::default());
        // Assert: fresh draft is clean and not saveable.
        assert!(!d.dirty);
        assert!(!d.stale);
        assert!(!can_save(&d));
        // Act: edit.
        d.cfg.general.max_record_secs = 9;
        d.dirty = true;
        // Assert.
        assert!(can_save(&d));
        // Act: external edit lands on disk.
        d.stale = is_stale(&d, &{
            let mut f = Config::default();
            f.general.mode = "toggle".into();
            f
        });
        // Assert.
        assert!(d.stale);
        assert!(!can_save(&d), "stale draft must block save");
        // Act/Assert: no unsaved edits -> never stale.
        let clean = SettingsDraft::new(&Config::default());
        let mut f = Config::default();
        f.general.mode = "toggle".into();
        assert!(!is_stale(&clean, &f));
    }

    #[test]
    fn draft_check_attributes_hotkey_errors() {
        // Arrange: wrong token order.
        let mut c = Config::default();
        c.general.hotkey = "ctrl+space+alt".into();
        // Act.
        let (fields, global) = draft_check(&c);
        // Assert.
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "general.hotkey");
        assert!(global.is_none());
    }

    #[test]
    fn draft_check_attributes_identical_hotkeys() {
        // Arrange: two hotkeys equal each other (validation-level conflict).
        let mut c = Config::default();
        c.general.en_hotkey = c.general.hotkey.clone();
        // Act.
        let (fields, global) = draft_check(&c);
        // Assert: attributed to the english row, not a vague global error.
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "general.en_hotkey");
        assert!(global.is_none());
    }

    #[test]
    fn draft_check_global_error_for_non_hotkey_fields() {
        // Arrange.
        let mut c = Config::default();
        c.general.mode = "latch".into();
        // Act.
        let (fields, global) = draft_check(&c);
        // Assert.
        assert!(fields.is_empty());
        assert!(global.is_some());
    }

    #[test]
    fn draft_check_clean_config_passes() {
        // Act.
        let (fields, global) = draft_check(&Config::default());
        // Assert.
        assert!(fields.is_empty());
        assert!(global.is_none());
    }

    #[test]
    fn sync_owned_respects_fresh_draft() {
        // Arrange: a fresh draft holds the file's values.
        let mut d = SettingsDraft::new(&Config::default());
        d.cfg.gui.theme = "dark".into();
        d.fresh = true;
        let mut live = Config::default();
        live.gui.theme = "light".into();
        // Act.
        sync_owned(&mut d, &live);
        // Assert: no overwrite of a just-reloaded draft.
        assert_eq!(d.cfg.gui.theme, "dark");
    }

    #[test]
    fn sync_owned_copies_other_page_fields_only() {
        // Arrange.
        let mut d = SettingsDraft::new(&Config::default());
        d.cfg.general.max_record_secs = 42; // settings-owned edit
        d.cfg.gui.theme = "dark".into(); // other-page field
        let mut live = Config::default();
        live.gui.theme = "light".into();
        live.audio.device = "Mic".into();
        live.asr.model = "m1".into();
        live.asr.en_model = "m2".into();
        // Act.
        sync_owned(&mut d, &live);
        // Assert.
        assert_eq!(d.cfg.gui.theme, "light");
        assert_eq!(d.cfg.audio.device, "Mic");
        assert_eq!(d.cfg.asr.model, "m1");
        assert_eq!(d.cfg.asr.en_model, "m2");
        assert_eq!(d.cfg.general.max_record_secs, 42);
    }

    #[test]
    fn merge_reload_without_edits_takes_file() {
        // Arrange: draft clean, file changed elsewhere.
        let path = tmp("merge-clean");
        let mut d = SettingsDraft::new(&Config::default());
        let mut c = Config::default();
        c.general.mode = "toggle".into();
        config::save(&path, &c).unwrap();
        // Act.
        merge_reload(&mut d, &path).unwrap();
        // Assert.
        assert_eq!(d.cfg.general.mode, "toggle");
        assert!(d.fresh);
        assert!(!d.dirty);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn merge_reload_with_edits_keeps_draft_fields() {
        // Arrange: draft has an unsaved edit to max_record_secs; the file was
        // changed externally in mode and in max_record_secs too.
        let path = tmp("merge-dirty");
        let mut d = SettingsDraft::new(&Config::default());
        d.cfg.general.max_record_secs = 99;
        d.dirty = true;
        let mut c = Config::default();
        c.general.mode = "toggle".into();
        c.general.max_record_secs = 7;
        c.text.strip_punctuation = true;
        config::save(&path, &c).unwrap();
        // Act.
        merge_reload(&mut d, &path).unwrap();
        // Assert: draft edit wins for max_record_secs; other external changes
        // are merged in; draft stays dirty.
        assert_eq!(d.cfg.general.max_record_secs, 99, "draft edit must survive");
        assert_eq!(d.cfg.general.mode, "toggle", "external change must merge");
        assert!(d.cfg.text.strip_punctuation);
        assert!(d.dirty);
        assert!(!d.stale);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn merge_reload_reports_broken_file_loudly() {
        // Arrange: invalid config on disk.
        let path = tmp("merge-broken");
        std::fs::write(&path, "not a config = [").unwrap();
        let mut d = SettingsDraft::new(&Config::default());
        d.dirty = true;
        // Act.
        let res = merge_reload(&mut d, &path);
        // Assert: loud error, draft untouched.
        assert!(res.is_err());
        assert!(d.dirty);
        assert!(!d.fresh);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn external_change_lists_changed_fields() {
        // Arrange.
        let d = SettingsDraft::new(&Config::default());
        let mut c = Config::default();
        c.general.mode = "toggle".into();
        c.inject.settle_ms = 500;
        // Act.
        let changed = external_change(&d, &c).expect("must detect changes");
        // Assert.
        assert_eq!(changed, vec!["general.mode", "inject.settle_ms"]);
        assert!(external_change(&d, &Config::default()).is_none());
    }

    #[test]
    fn mark_saved_clears_state() {
        // Arrange.
        let mut d = SettingsDraft::new(&Config::default());
        d.dirty = true;
        d.stale = true;
        d.fresh = true;
        d.errors.push(("x".into(), "y".into()));
        d.cfg.general.max_record_secs = 5;
        // Act.
        d.mark_saved();
        // Assert.
        assert!(!d.dirty);
        assert!(!d.stale);
        assert!(!d.fresh);
        assert!(d.errors.is_empty());
        assert_eq!(d.baseline.general.max_record_secs, 5);
    }
}
