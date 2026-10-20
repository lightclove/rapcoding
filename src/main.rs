mod asr;
mod audio;
mod config;
mod error;
mod hotkey;
mod inject;
mod log;
mod text;
mod wav;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use error::{Error, Result};
use log::{Level, Logger};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("rapcoding error: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn run(args: Vec<String>) -> Result<i32> {
    let mut config_override: Option<PathBuf> = None;
    let mut verbose = false;
    let mut positional: Vec<String> = Vec::new();

    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => {
                let p = it
                    .next()
                    .ok_or_else(|| Error::Config("--config needs a path".into()))?;
                config_override = Some(PathBuf::from(p));
            }
            "--verbose" | "-v" => verbose = true,
            "--help" | "-h" => {
                print_help();
                return Ok(0);
            }
            "--version" | "-V" => {
                println!("rapcoding {VERSION}");
                return Ok(0);
            }
            s if s.starts_with("--") => return Err(Error::Config(format!("unknown option '{s}'"))),
            s => positional.push(s.to_string()),
        }
    }

    let cmd = positional.first().map(|s| s.as_str()).unwrap_or("run");
    match cmd {
        "run" => cmd_run(config_override.as_deref(), verbose),
        "check" => cmd_check(config_override.as_deref()),
        "devices" => cmd_devices(),
        "init" => cmd_init(config_override.as_deref()),
        "transcribe" => {
            let path = positional
                .get(1)
                .map(PathBuf::from)
                .ok_or_else(|| Error::Config("usage: rapcoding transcribe <wav>".into()))?;
            cmd_transcribe(config_override.as_deref(), &path)
        }
        "inject" => {
            let text = positional[1..].join(" ");
            if text.trim().is_empty() {
                return Err(Error::Config("usage: rapcoding inject <text>".into()));
            }
            cmd_inject(config_override.as_deref(), &text)
        }
        "help" => {
            print_help();
            Ok(0)
        }
        other => Err(Error::Config(format!("unknown command '{other}'"))),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Hold,
    Toggle,
}

/// Which hotkey/model an utterance belongs to.
#[derive(Clone, Copy, PartialEq)]
enum Lang {
    Ru,
    En,
}

impl Lang {
    fn of(id: usize) -> Self {
        if id == 0 {
            Lang::Ru
        } else {
            Lang::En
        }
    }

    fn name(self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }
}

fn parse_mode(s: &str) -> Result<Mode> {
    match s {
        "hold" => Ok(Mode::Hold),
        "toggle" => Ok(Mode::Toggle),
        other => Err(Error::Config(format!(
            "general.mode must be 'hold' or 'toggle', got '{other}'"
        ))),
    }
}

struct WorkResult {
    ok: bool,
    text_len: usize,
    asr_ms: Duration,
    inject_ms: Duration,
    message: String,
}

fn cmd_run(override_path: Option<&Path>, verbose: bool) -> Result<i32> {
    let path = config::config_path(override_path);
    let cfg = config::load(&path)?;
    let level = if verbose { Level::Debug } else { Level::Info };
    let log_path = config::config_dir().join("rapcoding.log");
    let logger = Logger::new(log_path, level, cfg.general.log_transcripts)?;
    logger.info(&format!(
        "rapcoding {VERSION} starting; config {}",
        path.display()
    ));

    let mode = parse_mode(&cfg.general.mode)?;
    let audio = audio::AudioCapture::new(&cfg.audio.device)?;
    logger.info(&format!(
        "audio device '{}' {} Hz {} ch",
        audio.device_name(),
        audio.device_rate(),
        audio.channels()
    ));

    let base_dir = config_dir_of(&path);
    let model_dir = config::resolve_model_path(&base_dir, &cfg.asr.model);
    let en_model_dir = config::resolve_model_path(&base_dir, &cfg.asr.en_model);
    let asr = asr::VoskAsr::new(&model_dir)?;
    if let Err(e) = asr.check() {
        logger.warn(&format!("asr not ready: {e} (recognition will fail)"));
    }
    logger.debug(&format!(
        "asr: model={} en_model={}",
        cfg.asr.model, cfg.asr.en_model
    ));
    let injector = inject::Injector::new(
        cfg.inject.enabled,
        &cfg.inject.method,
        &cfg.inject.paste_shortcut,
        cfg.inject.restore_clipboard,
        cfg.inject.settle_ms,
    )?;

    let (hot_tx, hot_rx) = mpsc::channel::<hotkey::HotMsg>();
    let bindings = vec![cfg.general.hotkey.clone(), cfg.general.en_hotkey.clone()];
    let _hk = hotkey::spawn_hotkeys(&bindings, hot_tx)?;
    for b in &bindings {
        logger.info(&format!(
            "hotkey '{b}' registered (mode {})",
            cfg.general.mode
        ));
    }

    let (work_tx, work_rx) = mpsc::channel::<WorkResult>();
    let busy = Arc::new(AtomicBool::new(false));
    // English model loads lazily on first EN utterance to save RAM.
    let en = Arc::new(Mutex::new(None::<asr::VoskAsr>));
    let max = Duration::from_secs(cfg.general.max_record_secs.max(1));
    let mut recording: Option<usize> = None;

    let dispatch = |samples: Vec<f32>, lang: Lang| {
        if samples.is_empty() {
            return;
        }
        busy.store(true, Ordering::Relaxed);
        logger.info(&format!("transcribing… ({})", lang.name()));
        spawn_worker(
            samples,
            audio.device_rate(),
            lang,
            cfg.clone(),
            asr.clone(),
            en.clone(),
            en_model_dir.clone(),
            injector.clone(),
            logger.clone(),
            work_tx.clone(),
        );
    };

    loop {
        // Hotkey events.
        let mut fatal: Option<String> = None;
        while let Ok(msg) = hot_rx.try_recv() {
            match msg {
                hotkey::HotMsg::Fatal(e) => {
                    fatal = Some(e);
                    break;
                }
                hotkey::HotMsg::Event { id, state } => match (mode, state) {
                    (Mode::Hold, hotkey::HotEvent::Pressed) => {
                        if busy.load(Ordering::Relaxed) {
                            logger.warn("busy: ignoring hotkey");
                        } else if recording.is_none() {
                            audio.start();
                            recording = Some(id);
                            logger.info(&format!("listening ({})", Lang::of(id).name()));
                        }
                    }
                    (Mode::Hold, hotkey::HotEvent::Released) => {
                        if recording == Some(id) {
                            recording = None;
                            dispatch(audio.stop(), Lang::of(id));
                        }
                    }
                    (Mode::Toggle, hotkey::HotEvent::Pressed) => {
                        if let Some(start_id) = recording.take() {
                            dispatch(audio.stop(), Lang::of(start_id));
                        } else if busy.load(Ordering::Relaxed) {
                            logger.warn("busy: ignoring hotkey");
                        } else {
                            audio.start();
                            recording = Some(id);
                            logger.info(&format!("listening ({})", Lang::of(id).name()));
                        }
                    }
                    (_, hotkey::HotEvent::Released) => {}
                },
            }
        }
        if let Some(e) = fatal {
            return Err(Error::Hotkey(e));
        }

        // Worker results.
        while let Ok(r) = work_rx.try_recv() {
            if r.ok {
                logger.info(&format!(
                    "recognized {} chars; asr {:?}; inject {:?}",
                    r.text_len, r.asr_ms, r.inject_ms
                ));
            } else {
                logger.warn(&r.message);
            }
            busy.store(false, Ordering::Relaxed);
        }

        // Auto-stop runaway recordings.
        if recording.is_some() {
            if let Some(el) = audio.elapsed() {
                if el >= max {
                    let start_id = recording.take().unwrap();
                    logger.info("auto-stop at max duration");
                    dispatch(audio.stop(), Lang::of(start_id));
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

fn spawn_worker(
    samples: Vec<f32>,
    rate: u32,
    lang: Lang,
    cfg: config::Config,
    asr: asr::VoskAsr,
    en: Arc<Mutex<Option<asr::VoskAsr>>>,
    en_model_dir: PathBuf,
    injector: inject::Injector,
    logger: Logger,
    tx: Sender<WorkResult>,
) {
    std::thread::spawn(move || {
        let result = process_utterance(
            samples,
            rate,
            lang,
            &cfg,
            &asr,
            &en,
            &en_model_dir,
            &injector,
            &logger,
        );
        let _ = tx.send(result);
    });
}

/// Transcribe with the English model, loading it on first use.
fn transcribe_en(
    speech: &[f32],
    en: &Arc<Mutex<Option<asr::VoskAsr>>>,
    dir: &Path,
    logger: &Logger,
) -> Result<String> {
    let mut guard = en.lock().unwrap();
    if guard.is_none() {
        logger.info("loading english model…");
        *guard = Some(asr::VoskAsr::new(dir)?);
    }
    guard.as_ref().unwrap().transcribe_samples(speech)
}

fn process_utterance(
    samples: Vec<f32>,
    rate: u32,
    lang: Lang,
    cfg: &config::Config,
    asr: &asr::VoskAsr,
    en: &Arc<Mutex<Option<asr::VoskAsr>>>,
    en_model_dir: &Path,
    injector: &inject::Injector,
    logger: &Logger,
) -> WorkResult {
    let target = cfg.audio.sample_rate;
    let mono = audio::resample(&samples, rate, target);
    let pad = (cfg.audio.silence_padding_ms as usize * target as usize) / 1000;
    let speech = audio::trim_silence(&mono, cfg.audio.silence_threshold, pad);
    let min_samples = (cfg.audio.min_speech_ms as usize * target as usize) / 1000;
    if speech.len() < min_samples {
        let ms = speech.len() * 1000 / target as usize;
        return WorkResult {
            ok: false,
            text_len: 0,
            asr_ms: Duration::ZERO,
            inject_ms: Duration::ZERO,
            message: format!("too little speech ({ms} ms), ignored"),
        };
    }

    let peak = speech.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    logger.info(&format!(
        "captured {} ms audio, peak {:.3}",
        speech.len() * 1000 / target as usize,
        peak
    ));

    let t0 = Instant::now();
    let raw = match lang {
        Lang::Ru => asr.transcribe_samples(&speech),
        Lang::En => transcribe_en(&speech, en, en_model_dir, logger),
    };
    let raw = match raw {
        Ok(s) => s,
        Err(e) => {
            return fail(format!("asr: {e}"));
        }
    };
    let asr_ms = t0.elapsed();

    let text = text::postprocess(
        &raw,
        &cfg.text.voice_commands,
        cfg.text.remove_sound_tags,
        cfg.text.strip_punctuation,
    );
    if text.trim().is_empty() {
        return fail("empty transcript".into());
    }

    let t1 = Instant::now();
    let note = match injector.inject(&text) {
        Ok(n) => n,
        Err(e) => {
            return fail(format!("inject failed (text left in clipboard): {e}"));
        }
    };
    let inject_ms = t1.elapsed();

    logger.transcript(&format!("transcript: {text}"));

    WorkResult {
        ok: true,
        text_len: text.chars().count(),
        asr_ms,
        inject_ms,
        message: note,
    }
}

fn fail(message: String) -> WorkResult {
    WorkResult {
        ok: false,
        text_len: 0,
        asr_ms: Duration::ZERO,
        inject_ms: Duration::ZERO,
        message,
    }
}

fn cmd_check(override_path: Option<&Path>) -> Result<i32> {
    let path = config::config_path(override_path);
    let cfg = config::load(&path)?;
    let base_dir = config_dir_of(&path);
    let mut ok = true;

    println!("config: {}", path.display());
    match parse_mode(&cfg.general.mode) {
        Ok(_) => println!("  ok: general.mode = {}", cfg.general.mode),
        Err(e) => {
            println!("  FAIL: {e}");
            ok = false;
        }
    }
    match hotkey::parse_hotkey(&cfg.general.hotkey) {
        Ok(_) => println!("  ok: general.hotkey = {}", cfg.general.hotkey),
        Err(e) => {
            println!("  FAIL: {e}");
            ok = false;
        }
    }
    match hotkey::parse_hotkey(&cfg.general.en_hotkey) {
        Ok(_) => println!("  ok: general.en_hotkey = {}", cfg.general.en_hotkey),
        Err(e) => {
            println!("  FAIL: {e}");
            ok = false;
        }
    }
    if cfg.inject.paste_shortcut.is_empty() {
        println!("  ok: inject.paste_shortcut = platform default");
    } else {
        match inject::parse_chord(&cfg.inject.paste_shortcut) {
            Ok(_) => println!(
                "  ok: inject.paste_shortcut = {}",
                cfg.inject.paste_shortcut
            ),
            Err(e) => {
                println!("  FAIL: {e}");
                ok = false;
            }
        }
    }

    match audio::AudioCapture::new(&cfg.audio.device) {
        Ok(a) => println!(
            "  ok: audio device '{}' {} Hz {} ch",
            a.device_name(),
            a.device_rate(),
            a.channels()
        ),
        Err(e) => {
            println!("  FAIL: {e}");
            ok = false;
        }
    }

    match asr::VoskAsr::new(&config::resolve_model_path(&base_dir, &cfg.asr.model)) {
        Ok(a) => match a.check() {
            Ok(()) => println!("  ok: asr model '{}' present", cfg.asr.model),
            Err(e) => {
                println!("  FAIL: {e}");
                ok = false;
            }
        },
        Err(e) => {
            println!("  FAIL: {e}");
            ok = false;
        }
    }

    match asr::VoskAsr::new(&config::resolve_model_path(&base_dir, &cfg.asr.en_model)) {
        Ok(a) => match a.check() {
            Ok(()) => println!("  ok: asr en_model '{}' present", cfg.asr.en_model),
            Err(e) => {
                println!("  FAIL: {e}");
                ok = false;
            }
        },
        Err(e) => {
            println!("  FAIL: {e}");
            ok = false;
        }
    }

    if !cfg.inject.enabled {
        println!("  note: injection disabled (inject.enabled = false)");
    }

    if ok {
        println!("all checks passed");
        Ok(0)
    } else {
        println!("some checks FAILED");
        Ok(1)
    }
}

fn cmd_devices() -> Result<i32> {
    use cpal::traits::HostTrait;
    let host = cpal::default_host();
    let default = host
        .default_input_device()
        .map(|d| d.to_string())
        .unwrap_or_default();
    let mut n = 0;
    for d in host
        .input_devices()
        .map_err(|e| Error::Audio(format!("{e}")))?
    {
        let name = d.to_string();
        let mark = if name == default { " (default)" } else { "" };
        println!("{name}{mark}");
        n += 1;
    }
    if n == 0 {
        println!("no input devices found");
    }
    Ok(0)
}

fn cmd_init(override_path: Option<&Path>) -> Result<i32> {
    let path = config::config_path(override_path);
    if path.exists() {
        println!("config already exists: {}", path.display());
        return Ok(0);
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, config::DEFAULT_CONFIG)?;
    println!("wrote default config: {}", path.display());
    Ok(0)
}

fn cmd_transcribe(override_path: Option<&Path>, wav: &Path) -> Result<i32> {
    let path = config::config_path(override_path);
    let cfg = config::load(&path)?;
    let base_dir = config_dir_of(&path);
    let model_dir = config::resolve_model_path(&base_dir, &cfg.asr.model);
    let asr = asr::VoskAsr::new(&model_dir)?;
    asr.check()?;
    println!("transcribing {} …", wav.display());
    let samples = wav::read_wav(wav)?;
    let raw = asr.transcribe_samples(&samples)?;
    let text = text::postprocess(
        &raw,
        &cfg.text.voice_commands,
        cfg.text.remove_sound_tags,
        cfg.text.strip_punctuation,
    );
    println!("{text}");
    Ok(0)
}

fn cmd_inject(override_path: Option<&Path>, text: &str) -> Result<i32> {
    let path = config::config_path(override_path);
    let cfg = config::load(&path)?;
    let injector = inject::Injector::new(
        true,
        &cfg.inject.method,
        &cfg.inject.paste_shortcut,
        cfg.inject.restore_clipboard,
        cfg.inject.settle_ms,
    )?;
    let note = injector.inject(text)?;
    println!("injected ({note})");
    Ok(0)
}

fn config_dir_of(path: &Path) -> PathBuf {
    path.parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(config::config_dir)
}

fn print_help() {
    println!(
        "rapcoding {VERSION} — push-to-talk voice input for code and prompts

USAGE:
  rapcoding [OPTIONS] [COMMAND]

COMMANDS:
  run              Run the voice input daemon (default)
  check            Validate config, audio device, hotkey, ASR command/model
  devices          List input devices
  transcribe <wav> Transcribe a 16 kHz mono WAV and print the text
  inject <text>    Inject text at the cursor (test)
  init             Write a default config file
  help             Show this help

OPTIONS:
  --config <path>  Use a specific config file
  -v, --verbose    Debug logging
  -V, --version    Print version
  -h, --help       Show this help

ENV:
  RAPCODING_CONFIG       Override the config file path
  RAPCODING_CONFIG_DIR   Override the config directory
"
    );
}
