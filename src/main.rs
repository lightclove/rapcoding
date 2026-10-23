mod asr;
mod audio;
mod config;
#[cfg(feature = "gui")]
mod download;
mod error;
#[cfg(feature = "gui")]
mod gui;
mod hotkey;
mod inject;
mod journal;
mod log;
mod settings;
mod single;
mod status;
mod sync;
mod text;
mod wav;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use config::Config;
use error::{Error, Result};
use journal::Journal;
use log::{Level, LiveLog, Logger};
use status::Status;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(args) {
        Ok(code) => code,
        Err(e) => {
            // Fallible + sanitized: never panic while reporting, and never let
            // a control char in an error message forge/color the terminal.
            log::write_line(
                &mut std::io::stderr(),
                &format!("rapcoding error: {}", log::sanitize(&e.to_string())),
            );
            1
        }
    };
    std::process::exit(code);
}

fn run(args: Vec<String>) -> Result<i32> {
    let mut config_override: Option<PathBuf> = None;
    let mut verbose = false;
    let mut gui_flag = false;
    let mut no_gui = false;
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
            "--gui" => gui_flag = true,
            "--no-gui" => no_gui = true,
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
        "run" => {
            if gui_flag && no_gui {
                return Err(Error::Config(
                    "--gui and --no-gui are mutually exclusive".into(),
                ));
            }
            let path = config::config_path(config_override.as_deref());
            let cfg = config::load(&path)?;
            // One instance only: a second copy would fight over the global
            // hotkeys and inject recognized text twice.
            let _single = match single::acquire() {
                Ok(Some(guard)) => guard,
                Ok(None) => {
                    return Err(Error::Config(
                        "rapcoding уже запущен — закройте другой экземпляр".into(),
                    ));
                }
                Err(e) => return Err(e),
            };
            #[cfg(feature = "gui")]
            {
                if !no_gui && (gui_flag || cfg.gui.enabled) {
                    return cmd_gui(cfg, path, verbose);
                }
            }
            #[cfg(not(feature = "gui"))]
            {
                if gui_flag {
                    return Err(Error::Config(
                        "GUI support not compiled (rebuild with --features gui)".into(),
                    ));
                }
            }
            cmd_run(cfg, path, verbose)
        }
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
        match id {
            0 => Lang::Ru,
            1 => Lang::En,
            other => unreachable!("unknown hotkey id {other}"),
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

fn cmd_run(cfg: Config, path: PathBuf, verbose: bool) -> Result<i32> {
    let status = Arc::new(Mutex::new(Status::Idle));
    let journal = Journal::new(config::config_dir().join("journal.log"));
    let shared_cfg = Arc::new(RwLock::new(cfg.clone()));
    let transcripts = Arc::new(AtomicBool::new(cfg.general.log_transcripts));
    let daemon = Daemon::start(
        shared_cfg,
        transcripts,
        &path,
        verbose,
        status,
        None,
        &journal,
    )?;
    daemon.run()
}

#[cfg(feature = "gui")]
fn cmd_gui(cfg: Config, path: PathBuf, verbose: bool) -> Result<i32> {
    let status = Arc::new(Mutex::new(Status::Idle));
    let live = LiveLog::new(500);
    let journal = Journal::new(config::config_dir().join("journal.log"));
    // Config shared between the daemon and the GUI: the settings page writes
    // here on save, and the daemon snapshots it per utterance, so the live
    // subset (text.*, audio thresholds, max_record_secs, log_transcripts)
    // applies without a restart. Structural fields (hotkeys, device,
    // inject.*, models, mode) are read once at Daemon::start.
    let shared_cfg = Arc::new(RwLock::new(cfg.clone()));
    let transcripts = Arc::new(AtomicBool::new(cfg.general.log_transcripts));
    // The window must open even when the daemon cannot start (missing model,
    // no microphone, hotkey conflict): the user needs the UI to fix it, and an
    // invisible failure would look like the app simply does nothing.
    match Daemon::start(
        shared_cfg.clone(),
        transcripts.clone(),
        &path,
        verbose,
        status.clone(),
        Some(live.clone()),
        &journal,
    ) {
        Ok(daemon) => {
            std::thread::Builder::new()
                .name("rapcoding-daemon".into())
                .spawn(move || {
                    if let Err(e) = daemon.run() {
                        log::write_line(
                            &mut std::io::stderr(),
                            &format!(
                                "rapcoding daemon stopped: {}",
                                log::sanitize(&e.to_string())
                            ),
                        );
                    }
                })
                .map_err(|e| Error::Gui(format!("daemon thread: {e}")))?;
        }
        Err(e) => {
            let msg = e.to_string();
            *sync::lock_recover(&status) = Status::Error(msg.clone());
            journal.append("session", &format!("daemon failed: {msg}"));
        }
    }
    gui::run(cfg, path, status, live, journal, shared_cfg, transcripts, verbose)
}

/// Everything the recognition loop needs. Setup runs synchronously so early
/// failures (bad config, missing audio/model, hotkey conflict) surface to the
/// caller instead of dying silently on a background thread.
struct Daemon {
    /// Shared config: snapshot per utterance (dispatch) and per loop tick
    /// (max_record_secs). See the live-subset contract in `cmd_gui`.
    cfg: Arc<RwLock<Config>>,
    mode: Mode,
    logger: Logger,
    audio: audio::AudioCapture,
    asr: asr::VoskAsr,
    en_model_dir: PathBuf,
    injector: inject::Injector,
    hot_rx: mpsc::Receiver<hotkey::HotMsg>,
    _hotkeys: std::thread::JoinHandle<()>,
    work_tx: mpsc::Sender<WorkResult>,
    work_rx: mpsc::Receiver<WorkResult>,
    busy: Arc<AtomicBool>,
    en: Arc<Mutex<Option<asr::VoskAsr>>>,
    status: Arc<Mutex<Status>>,
}

impl Daemon {
    fn start(
        cfg: Arc<RwLock<Config>>,
        transcripts: Arc<AtomicBool>,
        path: &Path,
        verbose: bool,
        status: Arc<Mutex<Status>>,
        live: Option<LiveLog>,
        journal: &Journal,
    ) -> Result<Self> {
        let init = sync::read_recover(&cfg).clone();
        let level = if verbose { Level::Debug } else { Level::Info };
        let log_path = config::config_dir().join("rapcoding.log");
        let logger = Logger::new_with_live(log_path, level, transcripts, live)?;
        logger.info(&format!("rapcoding {VERSION} starting; config {}", path.display()));
        journal.append("session", &format!("start; config {}", path.display()));

        let mode = parse_mode(&init.general.mode)?;
        let audio = audio::AudioCapture::new(&init.audio.device)?;
        logger.info(&format!(
            "audio device '{}' {} Hz {} ch",
            audio.device_name(),
            audio.device_rate(),
            audio.channels()
        ));

        let base_dir = config_dir_of(path);
        let model_dir = config::resolve_model_path(&base_dir, &init.asr.model);
        let en_model_dir = config::resolve_model_path(&base_dir, &init.asr.en_model);
        let asr = asr::VoskAsr::new(&model_dir)?;
        if let Err(e) = asr.check() {
            logger.warn(&format!("asr not ready: {e} (recognition will fail)"));
        }
        logger.debug(&format!("asr: model={} en_model={}", init.asr.model, init.asr.en_model));
        let injector = inject::Injector::new(
            init.inject.enabled,
            &init.inject.method,
            &init.inject.paste_shortcut,
            init.inject.restore_clipboard,
            init.inject.settle_ms,
        )?;

        let (hot_tx, hot_rx) = mpsc::channel::<hotkey::HotMsg>();
        let bindings = vec![init.general.hotkey.clone(), init.general.en_hotkey.clone()];
        let hotkeys = hotkey::spawn_hotkeys(&bindings, hot_tx)?;
        for b in &bindings {
            logger.info(&format!("hotkey '{b}' registered (mode {})", init.general.mode));
        }

        let (work_tx, work_rx) = mpsc::channel::<WorkResult>();
        Ok(Daemon {
            cfg,
            mode,
            logger,
            audio,
            asr,
            en_model_dir,
            injector,
            hot_rx,
            _hotkeys: hotkeys,
            work_tx,
            work_rx,
            busy: Arc::new(AtomicBool::new(false)),
            en: Arc::new(Mutex::new(None::<asr::VoskAsr>)),
            status,
        })
    }

    fn set_status(&self, s: Status) {
        *sync::lock_recover(&self.status) = s;
    }

    fn dispatch(&self, samples: Vec<f32>, lang: Lang) {
        if samples.is_empty() {
            return;
        }
        self.busy.store(true, Ordering::Relaxed);
        self.set_status(Status::Transcribing(lang.name()));
        self.logger.info(&format!("transcribing… ({})", lang.name()));
        // Snapshot the shared config: the live subset edited in the settings
        // page (text.*, audio thresholds) takes effect on the next utterance.
        let cfg = sync::read_recover(&self.cfg).clone();
        spawn_worker(
            samples,
            self.audio.device_rate(),
            lang,
            cfg,
            self.asr.clone(),
            self.en.clone(),
            self.en_model_dir.clone(),
            self.injector.clone(),
            self.logger.clone(),
            self.work_tx.clone(),
        );
    }

    fn run(self) -> Result<i32> {
        let mut recording: Option<usize> = None;

        loop {
            // Hotkey events.
            let mut fatal: Option<String> = None;
            while let Ok(msg) = self.hot_rx.try_recv() {
                match msg {
                    hotkey::HotMsg::Fatal(e) => {
                        fatal = Some(e);
                        break;
                    }
                    hotkey::HotMsg::Event { id, state } => match (self.mode, state) {
                        (Mode::Hold, hotkey::HotEvent::Pressed) => {
                            if self.busy.load(Ordering::Relaxed) {
                                self.logger.warn("busy: ignoring hotkey");
                            } else if recording.is_none() {
                                self.audio.start();
                                recording = Some(id);
                                self.set_status(Status::Listening(Lang::of(id).name()));
                                self.logger.info(&format!("listening ({})", Lang::of(id).name()));
                            }
                        }
                        (Mode::Hold, hotkey::HotEvent::Released) => {
                            if recording == Some(id) {
                                recording = None;
                                let samples = self.audio.stop();
                                self.dispatch(samples, Lang::of(id));
                            }
                        }
                        (Mode::Toggle, hotkey::HotEvent::Pressed) => {
                            if let Some(start_id) = recording.take() {
                                let samples = self.audio.stop();
                                self.dispatch(samples, Lang::of(start_id));
                            } else if self.busy.load(Ordering::Relaxed) {
                                self.logger.warn("busy: ignoring hotkey");
                            } else {
                                self.audio.start();
                                recording = Some(id);
                                self.set_status(Status::Listening(Lang::of(id).name()));
                                self.logger.info(&format!("listening ({})", Lang::of(id).name()));
                            }
                        }
                        (_, hotkey::HotEvent::Released) => {}
                    },
                }
            }
            if let Some(e) = fatal {
                self.logger.warn(&format!("fatal: {e}"));
                self.set_status(Status::Error(e.clone()));
                return Err(Error::Hotkey(e));
            }

            // Worker results.
            while let Ok(r) = self.work_rx.try_recv() {
                if r.ok {
                    self.logger.info(&format!(
                        "recognized {} chars; asr {:?}; inject {:?} ({})",
                        r.text_len, r.asr_ms, r.inject_ms, r.message
                    ));
                } else {
                    self.logger.warn(&r.message);
                }
                self.busy.store(false, Ordering::Relaxed);
                self.set_status(Status::Idle);
            }

            // Auto-stop runaway recordings. The limit is read live from the
            // shared config: a settings change applies without a restart.
            if let Some(start_id) = recording {
                if let Some(el) = self.audio.elapsed() {
                    let max = Duration::from_secs(
                        sync::read_recover(&self.cfg).general.max_record_secs.max(1),
                    );
                    if el >= max {
                        recording = None;
                        self.logger.info("auto-stop at max duration");
                        let samples = self.audio.stop();
                        self.dispatch(samples, Lang::of(start_id));
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Run CPU-heavy transcription at below-normal priority so the real-time
/// WASAPI audio thread is not starved (which surfaces as buffer underruns).
#[cfg(target_os = "windows")]
fn lower_worker_priority() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(target_os = "windows"))]
fn lower_worker_priority() {}

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
        lower_worker_priority();
        // Always deliver a result so `busy` is never left stuck by a panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_utterance(
                samples,
                rate,
                lang,
                &cfg,
                &asr,
                &en,
                &en_model_dir,
                &injector,
                &logger,
            )
        }))
        .unwrap_or_else(|payload| {
            let msg = panic_message(&payload);
            logger.warn(&format!("worker {msg}"));
            fail(msg)
        });
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
    let mut guard = sync::lock_recover(en.as_ref());
    if guard.is_none() {
        logger.info("loading english model…");
        *guard = Some(asr::VoskAsr::new(dir)?);
    }
    let Some(asr) = guard.as_ref() else {
        return Err(Error::Asr("english model unavailable".into()));
    };
    asr.transcribe_samples(speech)
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
    let target = asr::ASR_RATE;
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

/// Extract a human-readable message from a panic payload.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        format!("panic: {s}")
    } else if let Some(s) = payload.downcast_ref::<String>() {
        format!("panic: {s}")
    } else {
        "panic (unknown payload)".to_string()
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
    let inputs = audio::list_input_devices()?;
    let mut n = 0;
    for d in &inputs {
        let mark = if d.is_default { " (default)" } else { "" };
        println!("{}{mark}", d.name);
        n += 1;
    }
    if n == 0 {
        println!("no input devices found");
    }
    let outputs = audio::list_output_devices()?;
    if !outputs.is_empty() {
        println!();
        println!("output:");
        for d in &outputs {
            let mark = if d.is_default { " (default)" } else { "" };
            println!("{}{mark}", d.name);
        }
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
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => config::config_dir(),
    }
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
  --gui            Open the GUI window (requires --features gui)
  --no-gui         Run headless even if gui.enabled = true
  -v, --verbose    Debug logging
  -V, --version    Print version
  -h, --help       Show this help

ENV:
  RAPCODING_CONFIG       Override the config file path
  RAPCODING_CONFIG_DIR   Override the config directory
"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_of_maps_hotkey_ids() {
        assert!(matches!(Lang::of(0), Lang::Ru));
        assert!(matches!(Lang::of(1), Lang::En));
    }

    #[test]
    #[should_panic]
    fn lang_of_rejects_unknown_id() {
        let _ = Lang::of(2);
    }
}
