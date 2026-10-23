//! Native GUI (egui/eframe): a proper app shell — header, side navigation,
//! card-based content, custom light/dark palettes.
//!
//! Compiled only with `--features gui`. The recognition daemon runs on a
//! background thread and reports live status through the shared `Status`;
//! log lines stream through `LiveLog`, management events land in `Journal`,
//! and models are downloaded/extracted by `download`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use eframe::egui::{
    self, text::LayoutJob, Color32, CornerRadius, FontId, Margin, RichText, Stroke, TextFormat,
};

use crate::audio;
use crate::config::{self, Config};
use crate::download;
use crate::error::Result;
use crate::journal::Journal;
use crate::log::{self, LiveLog};
use crate::settings::{
    self, can_save, draft_check, merge_reload, sync_owned, SettingsDraft, SETTINGS_FIELDS,
    SETTINGS_SECTIONS,
};
use crate::status::Status;
use crate::sync;

// ---------------------------------------------------------------- theming

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Theme {
    Light,
    Dark,
}

#[derive(Clone, Copy)]
struct Palette {
    bg: Color32,
    surface: Color32,
    stroke: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    info: Color32,
    success: Color32,
    warn: Color32,
    error: Color32,
}

impl Palette {
    fn light() -> Self {
        Self {
            bg: Color32::from_rgb(0xF4, 0xF6, 0xFB),
            surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            stroke: Color32::from_rgb(0xE2, 0xE7, 0xF0),
            text: Color32::from_rgb(0x18, 0x1D, 0x2B),
            muted: Color32::from_rgb(0x6A, 0x73, 0x87),
            accent: Color32::from_rgb(0x53, 0x63, 0xF0),
            info: Color32::from_rgb(0x0B, 0x72, 0x85),
            success: Color32::from_rgb(0x12, 0xA0, 0x5E),
            warn: Color32::from_rgb(0xE0, 0x82, 0x0B),
            error: Color32::from_rgb(0xD9, 0x2D, 0x20),
        }
    }

    fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(0x0D, 0x10, 0x16),
            surface: Color32::from_rgb(0x16, 0x1B, 0x23),
            stroke: Color32::from_rgb(0x25, 0x2C, 0x38),
            text: Color32::from_rgb(0xE7, 0xEB, 0xF3),
            muted: Color32::from_rgb(0x8A, 0x93, 0xA6),
            accent: Color32::from_rgb(0x7B, 0x8B, 0xFF),
            info: Color32::from_rgb(0x3B, 0xC9, 0xDB),
            success: Color32::from_rgb(0x2E, 0xCC, 0x71),
            warn: Color32::from_rgb(0xF3, 0x9C, 0x12),
            error: Color32::from_rgb(0xF0, 0x52, 0x45),
        }
    }
}

impl Theme {
    fn from_config(s: &str) -> Self {
        match s {
            "dark" => Theme::Dark,
            _ => Theme::Light,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    fn toggle(self) -> Self {
        match self {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        }
    }

    fn palette(self) -> Palette {
        match self {
            Theme::Light => Palette::light(),
            Theme::Dark => Palette::dark(),
        }
    }

    fn visuals(self) -> egui::Visuals {
        let p = self.palette();
        let mut v = match self {
            Theme::Light => egui::Visuals::light(),
            Theme::Dark => egui::Visuals::dark(),
        };
        v.override_text_color = Some(p.text);
        v.panel_fill = p.bg;
        v.window_fill = p.surface;
        v.extreme_bg_color = p.surface;
        v.faint_bg_color = p.surface;
        v.hyperlink_color = p.accent;
        // Selection: a tinted fill that keeps the text readable. The default
        // `selection.stroke` is used as the SELECTED TEXT colour by
        // `Style::interact_selectable` (egui 0.32), so it must be the palette
        // text colour, not the accent — otherwise dark theme draws accent-blue
        // text on an accent-blue fill (invisible).
        v.selection.bg_fill = p.accent.gamma_multiply(0.28);
        v.selection.stroke = Stroke::new(1.0_f32, p.text);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.stroke);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(8);
            w.bg_stroke = Stroke::new(1.0_f32, p.stroke);
            // Text colour per widget state: a hovered/active button must keep
            // readable text instead of inheriting egui's light-theme default.
            w.fg_stroke = Stroke::new(1.0_f32, p.text);
        }
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
        v.widgets.inactive.weak_bg_fill = p.surface;
        v.widgets.inactive.bg_fill = p.surface;
        v.widgets.hovered.weak_bg_fill = p.bg;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
        v.widgets.hovered.bg_fill = p.surface;
        v.widgets.active.weak_bg_fill = p.accent.gamma_multiply(0.22);
        v.widgets.active.bg_fill = p.accent.gamma_multiply(0.22);
        v.widgets.open.weak_bg_fill = p.surface;
        v.widgets.open.bg_fill = p.surface;
        // Text edit / combo internals: dark theme must not fall back to white.
        v.text_cursor.stroke = Stroke::new(1.0_f32, p.text);
        v
    }

    fn style(&self, ctx: &egui::Context) {
        let mut s = (*ctx.style()).clone();
        s.spacing.item_spacing = egui::vec2(10.0, 10.0);
        s.spacing.button_padding = egui::vec2(12.0, 7.0);
        s.visuals.widgets.noninteractive.corner_radius = CornerRadius::same(10);
        ctx.set_style(s);
    }
}

// ------------------------------------------------------------------- pages

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Sound,
    Models,
    Settings,
    Help,
    Log,
    Journal,
}

impl Page {
    const ALL: [Page; 7] = [
        Page::Overview,
        Page::Sound,
        Page::Models,
        Page::Settings,
        Page::Help,
        Page::Log,
        Page::Journal,
    ];

    fn title(self) -> &'static str {
        match self {
            Page::Overview => "Обзор",
            Page::Sound => "Звук",
            Page::Models => "Модели",
            Page::Settings => "Настройки",
            Page::Help => "Инструкция",
            Page::Log => "Лог",
            Page::Journal => "Журнал",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Page::Overview => "Статус, хоткеи и активные модели",
            Page::Sound => "Какой микрофон использовать и куда идёт вывод",
            Page::Models => "Скачать модель, сделать её активной",
            Page::Settings => "Все параметры из config.toml — правка и сохранение",
            Page::Help => "Пошаговая инструкция и голосовые команды",
            Page::Log => "Живой лог программы и экспорт в файл",
            Page::Journal => "История изменений настроек и загрузок",
        }
    }
}

/// State of the current (or last) model download, shared with the worker thread.
#[derive(Clone, PartialEq, Debug)]
enum DlState {
    Idle,
    Downloading {
        name: String,
        received: u64,
        total: u64,
    },
    Extracting {
        name: String,
    },
    Done {
        name: String,
    },
    Canceled,
    Error(String),
}

impl DlState {
    fn is_busy(&self) -> bool {
        matches!(
            self,
            DlState::Downloading { .. } | DlState::Extracting { .. }
        )
    }
}

struct GuiApp {
    cfg: Config,
    config_path: PathBuf,
    status: Arc<Mutex<Status>>,
    live: LiveLog,
    journal: Journal,
    dl_state: Arc<Mutex<DlState>>,
    dl_cancel: Option<Arc<AtomicBool>>,
    delete_confirm: Option<&'static str>,
    delete_error: Option<String>,
    page: Page,
    theme: Theme,
    applied: Theme,
    persist_error: Option<String>,
    export_error: Option<String>,
    pending_checked: bool,
    input_devices: Vec<audio::DeviceInfo>,
    output_devices: Vec<audio::DeviceInfo>,
    sound_page_dirty: bool,
    sound_error: Option<String>,
    /// Draft of the settings page; None until the page is first opened.
    settings: Option<SettingsDraft>,
    /// Field id currently captured by the «Записать» hotkey widget.
    capture: Option<&'static str>,
    /// Shared with the daemon: the live subset is pushed here on save.
    live_cfg: Option<Arc<RwLock<Config>>>,
    /// Shared with the daemon logger: `log_transcripts` applies live.
    transcripts: Option<Arc<AtomicBool>>,
    /// Previous frame's window focus, to detect refocus events.
    focused: bool,
}

/// The app icon, embedded at compile time and decoded once (ICO with an
/// embedded PNG frame; the `image` crate is already in the dependency tree).
fn app_icon() -> Option<egui::IconData> {
    const ICO: &[u8] = include_bytes!("../assets/rapcoding.ico");
    let img = image::load_from_memory_with_format(ICO, image::ImageFormat::Ico).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}

pub fn run(
    cfg: Config,
    config_path: PathBuf,
    status: Arc<Mutex<Status>>,
    live: LiveLog,
    journal: Journal,
    shared_cfg: Arc<RwLock<Config>>,
    transcripts: Arc<AtomicBool>,
    _verbose: bool,
) -> Result<i32> {
    let theme = Theme::from_config(&cfg.gui.theme);
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([980.0, 680.0])
        .with_min_inner_size([760.0, 460.0])
        .with_resizable(true)
        .with_title("rapcoding");
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let app = GuiApp {
        cfg,
        config_path,
        status,
        live,
        journal,
        dl_state: Arc::new(Mutex::new(DlState::Idle)),
        dl_cancel: None,
        delete_confirm: None,
        delete_error: None,
        page: start_page(),
        theme,
        applied: theme,
        persist_error: None,
        export_error: None,
        pending_checked: false,
        input_devices: Vec::new(),
        output_devices: Vec::new(),
        sound_page_dirty: true,
        sound_error: None,
        settings: None,
        capture: None,
        live_cfg: Some(shared_cfg),
        transcripts: Some(transcripts),
        focused: true,
    };
    eframe::run_native(
        "rapcoding",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(theme.visuals());
            theme.style(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| crate::error::Error::Gui(format!("{e}")))?;
    Ok(0)
}

/// Initial page, overridable with `RAPCODING_START_PAGE` (design review and
/// screenshot automation; harmless in normal use).
fn start_page() -> Page {
    match std::env::var("RAPCODING_START_PAGE")
        .unwrap_or_default()
        .as_str()
    {
        "sound" => Page::Sound,
        "models" => Page::Models,
        "settings" => Page::Settings,
        "help" => Page::Help,
        "log" => Page::Log,
        "journal" => Page::Journal,
        _ => Page::Overview,
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.pending_checked {
            self.pending_checked = true;
            self.start_pending_downloads();
        }
        if self.theme != self.applied {
            self.applied = self.theme;
            ctx.set_visuals(self.theme.visuals());
            self.theme.style(ctx);
        }
        let p = self.theme.palette();
        // Refocus with the settings page open -> check for external edits.
        let focused = ctx.input(|i| i.focused);
        if focused && !self.focused && self.page == Page::Settings {
            if let Some(d) = &mut self.settings {
                if file_changed_externally(d, &self.config_path) {
                    d.stale = true;
                }
            }
        }
        self.focused = focused;
        let busy = sync::lock_recover(&self.dl_state).is_busy();
        if busy {
            // Repaint while downloading so the progress bar stays live.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // A hotkey capture must not sit forever waiting for a key: repaint so
        // the frame state (events consumed, focus lost) keeps up.
        if self.capture.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        self.header(ctx, p);
        self.nav(ctx, p);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(Margin::symmetric(20, 18)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Overview => self.overview(ui, p),
                        Page::Sound => self.sound(ui, p),
                        Page::Models => self.models(ui, p),
                        Page::Settings => self.settings_page(ui, p),
                        Page::Help => Self::help(ui, p),
                        Page::Log => self.logs(ui, p),
                        Page::Journal => self.journal_page(ui, p),
                    });
            });
    }
}

impl GuiApp {
    // ------------------------------------------------------------- chrome

    fn header(&mut self, ctx: &egui::Context, p: Palette) {
        egui::TopBottomPanel::top("header")
            .exact_height(64.0)
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(Margin::symmetric(20, 0))
                    .stroke(Stroke::new(1.0_f32, p.stroke)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("rapcoding").size(20.0).strong().color(p.text))
                        .on_hover_text("Голосовой ввод: говорите — текст печатается само");
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(12.0)
                            .color(p.muted),
                    );
                    ui.add_space(12.0);
                    self.status_pill(ui, p);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = match self.theme {
                            Theme::Light => "Тёмная тема",
                            Theme::Dark => "Светлая тема",
                        };
                        if ui
                            .button(label)
                            .on_hover_text("Переключить оформление (сохраняется в конфиг)")
                            .clicked()
                        {
                            self.theme = self.theme.toggle();
                            self.cfg.gui.theme = self.theme.as_str().to_string();
                            // Keep an open settings draft in sync so «Сохранить»
                            // cannot revert a theme switched in the header.
                            if let Some(d) = &mut self.settings {
                                d.cfg.gui.theme = self.cfg.gui.theme.clone();
                            }
                            self.journal
                                .append("config", &format!("theme -> {}", self.theme.as_str()));
                            self.persist_error = match config::save(&self.config_path, &self.cfg) {
                                Ok(()) => None,
                                Err(e) => Some(e.to_string()),
                            };
                        }
                    });
                });
            });
    }

    fn status_pill(&self, ui: &mut egui::Ui, p: Palette) {
        let st = sync::lock_recover(&self.status).clone();
        let (color, text, hint) = match &st {
            Status::Idle => (p.success, "Готов", "Жду нажатия хоткея"),
            Status::Listening(l) => (
                p.success,
                if *l == "en" {
                    "Слушаю · EN"
                } else {
                    "Слушаю · RU"
                },
                "Идёт запись — говорите и отпустите хоткей",
            ),
            Status::Transcribing(l) => (
                p.warn,
                if *l == "en" {
                    "Распознаю · EN"
                } else {
                    "Распознаю · RU"
                },
                "Распознавание речи, обычно около секунды",
            ),
            Status::Error(_) => (p.error, "Ошибка", "Подробности — в разделе «Лог»"),
        };
        let frame = egui::Frame::new()
            .fill(color.gamma_multiply(0.14))
            .corner_radius(CornerRadius::same(20))
            .inner_margin(Margin::symmetric(12, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // A drawn dot: the default font has no bullet glyph.
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.5, color);
                    ui.label(RichText::new(text).size(13.0).color(color).strong());
                });
            });
        frame.response.on_hover_text(hint);
        if let Status::Error(msg) = &st {
            ui.label(RichText::new(msg).size(12.0).color(p.error));
        }
    }

    fn nav(&mut self, ctx: &egui::Context, p: Palette) {
        egui::SidePanel::left("nav")
            .exact_width(190.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(Margin::symmetric(10, 16))
                    .stroke(Stroke::new(1.0_f32, p.stroke)),
            )
            .show(ctx, |ui| {
                for page in Page::ALL {
                    let selected = self.page == page;
                    let (fill, color) = if selected {
                        (p.accent.gamma_multiply(0.14), p.accent)
                    } else {
                        (Color32::TRANSPARENT, p.text)
                    };
                    let resp = egui::Frame::new()
                        .fill(fill)
                        .corner_radius(CornerRadius::same(9))
                        .inner_margin(Margin::symmetric(12, 9))
                        .show(ui, |ui| {
                            ui.label(RichText::new(page.title()).size(14.0).color(color).strong());
                        })
                        .response;
                    let resp = resp
                        .interact(egui::Sense::click())
                        .on_hover_text(page.hint());
                    if resp.clicked() {
                        if self.page != page {
                            if page == Page::Sound {
                                // BT devices appear and disappear; re-enumerate on entry.
                                self.sound_page_dirty = true;
                            }
                            if page == Page::Settings {
                                // Fresh draft from the current live config;
                                // any previous capture is abandoned.
                                self.capture = None;
                                self.settings = Some(SettingsDraft::new(&self.cfg));
                            }
                        }
                        self.page = page;
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if !self.cfg.gui.enabled {
                        ui.label(
                            RichText::new("консольный режим")
                                .size(11.0)
                                .color(p.muted),
                        )
                        .on_hover_text("gui.enabled = false — окно отключено в конфиге");
                    }
                    ui.label(
                        RichText::new(match self.cfg.general.mode.as_str() {
                            "toggle" => "режим: toggle",
                            _ => "режим: hold",
                        })
                        .size(11.0)
                        .color(p.muted),
                    )
                    .on_hover_text(
                        "hold — запись, пока держите клавиши; toggle — нажатие включает и выключает",
                    );
                });
            });
    }

    fn card<R>(ui: &mut egui::Ui, p: Palette, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        egui::Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.stroke))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(16))
            .show(ui, add)
            .inner
    }

    fn section_title(ui: &mut egui::Ui, p: Palette, text: &str) {
        ui.label(RichText::new(text).size(13.0).color(p.muted).strong());
        ui.add_space(2.0);
    }

    // ------------------------------------------------------------ overview

    fn overview(&mut self, ui: &mut egui::Ui, p: Palette) {
        if let Some(err) = &self.persist_error {
            ui.colored_label(p.error, err);
        }
        Self::card(ui, p, |ui| {
            ui.label(RichText::new("Диктовка").size(18.0).strong().color(p.text));
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Зажмите хоткей, говорите, отпустите — текст появится в активном окне.",
                )
                .size(13.0)
                .color(p.muted),
            );
            ui.add_space(14.0);
            egui::Grid::new("hotkeys")
                .num_columns(3)
                .spacing([18.0, 10.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Русский").size(13.0).color(p.muted));
                    ui.label(RichText::new("Английский").size(13.0).color(p.muted));
                    ui.label("");
                    ui.end_row();
                    for (ru, en) in [(&self.cfg.general.hotkey, &self.cfg.general.en_hotkey)] {
                        ui.label(
                            RichText::new(pretty_hotkey(ru))
                                .monospace()
                                .size(14.0)
                                .color(p.text),
                        );
                        ui.label(
                            RichText::new(pretty_hotkey(en))
                                .monospace()
                                .size(14.0)
                                .color(p.text),
                        );
                        ui.label("");
                        ui.end_row();
                    }
                });
        });

        ui.add_space(12.0);
        self.models_card(ui, p);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(match self.cfg.inject.method.as_str() {
                    "type" => "Вставка: печать текста напрямую",
                    _ => "Вставка: через буфер обмена",
                })
                .size(11.5)
                .color(p.muted),
            )
            .on_hover_text(
                "inject.method в конфиге: paste — вставка через буфер (по умолчанию), type — печать посимвольно",
            );
            if !self.cfg.inject.enabled {
                ui.label(RichText::new("· вставка отключена").size(11.5).color(p.error))
                    .on_hover_text("inject.enabled = false — текст не вставляется");
            }
        });
    }

    // ----------------------------------------------------------------- sound

    fn sound(&mut self, ui: &mut egui::Ui, p: Palette) {
        if self.sound_page_dirty {
            self.sound_page_dirty = false;
            match (audio::list_input_devices(), audio::list_output_devices()) {
                (Ok(inputs), Ok(outputs)) => {
                    self.input_devices = inputs;
                    self.output_devices = outputs;
                    self.sound_error = None;
                }
                (Err(e), _) | (_, Err(e)) => {
                    self.sound_error = Some(e.to_string());
                }
            }
        }
        if let Some(err) = &self.sound_error {
            Self::card(ui, p, |ui| {
                ui.label(
                    RichText::new("Не удалось получить список устройств")
                        .size(14.0)
                        .strong()
                        .color(p.error),
                );
                ui.label(RichText::new(err).size(12.5).color(p.error));
            });
            return;
        }
        if let Some(err) = &self.persist_error {
            ui.colored_label(p.error, err);
        }

        let names: Vec<String> = self.input_devices.iter().map(|d| d.name.clone()).collect();
        // Which device the configured filter resolves to right now (None = default).
        let checked: Option<String> =
            audio::pick_index(&self.cfg.audio.device, &names).map(|i| names[i].clone());
        let mut value = checked
            .clone()
            .unwrap_or_else(|| self.cfg.audio.device.clone());
        let mut changed = false;

        Self::card(ui, p, |ui| {
            ui.label(
                RichText::new("Микрофон (ввод)")
                    .size(16.0)
                    .strong()
                    .color(p.text),
            );
            ui.label(
                RichText::new(
                    "Отсюда rapcoding пишет звук для распознавания. Применяется после перезапуска программы.",
                )
                .size(12.5)
                .color(p.muted),
            );
            ui.add_space(8.0);

            let default_name = self
                .input_devices
                .iter()
                .find(|d| d.is_default)
                .map(|d| d.name.as_str())
                .unwrap_or("нет устройства");
            let row = device_row(
                ui,
                p,
                value.is_empty(),
                &format!("Система по умолчанию — {default_name}"),
                &[],
            );
            if row.clicked() {
                value.clear();
                changed = true;
            }

            for d in &self.input_devices {
                let suffix: &[(&str, Color32)] = if d.is_default {
                    &[("по умолчанию", p.accent)]
                } else {
                    &[]
                };
                let row = device_row(ui, p, value.as_str() == d.name.as_str(), &d.name, suffix);
                let sum = d.summary();
                let row = if sum.is_empty() {
                    row
                } else {
                    row.on_hover_text(format!("{} — {sum}", d.name))
                };
                if row.clicked() {
                    value = d.name.clone();
                    changed = true;
                }
            }

            if let Some(i) = audio::pick_index(&value, &names) {
                let sum = self.input_devices[i].summary();
                if !sum.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!("Формат захвата: {sum}"))
                            .size(12.0)
                            .color(p.muted),
                    );
                }
            }
            if !self.cfg.audio.device.is_empty() && checked.is_none() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "Настроенное устройство «{}» сейчас не видно (отключено?). Пока оно не вернётся или не выберете другое — демон не стартует.",
                        self.cfg.audio.device
                    ))
                    .size(12.0)
                    .color(p.warn),
                );
            }
            ui.add_space(6.0);
            if ui
                .button("Обновить список")
                .on_hover_text("Bluetooth-устройства подключаются и отключаются — список перечитывается при входе на страницу и по этой кнопке")
                .clicked()
            {
                self.sound_page_dirty = true;
            }
        });

        if changed && value != self.cfg.audio.device {
            self.cfg.audio.device = value;
            let what = if self.cfg.audio.device.is_empty() {
                "system default".to_string()
            } else {
                format!("\"{}\"", self.cfg.audio.device)
            };
            self.journal
                .append("config", &format!("audio device -> {what}"));
            self.persist_error = match config::save(&self.config_path, &self.cfg) {
                Ok(()) => None,
                Err(e) => Some(e.to_string()),
            };
        }

        ui.add_space(12.0);
        Self::card(ui, p, |ui| {
            ui.label(
                RichText::new("Вывод (наушники/динамики)")
                    .size(16.0)
                    .strong()
                    .color(p.text),
            );
            ui.label(
                RichText::new(
                    "rapcoding ничего не воспроизводит — этот список только для справки. Устройство вывода по умолчанию меняется в Параметрах Windows: Система, Звук.",
                )
                .size(12.5)
                .color(p.muted),
            );
            ui.add_space(8.0);
            if self.output_devices.is_empty() {
                ui.label(
                    RichText::new("Устройства вывода не найдены")
                        .size(12.5)
                        .color(p.warn),
                );
            }
            for d in &self.output_devices {
                let suffix: &[(&str, Color32)] = if d.is_default {
                    &[("по умолчанию", p.accent)]
                } else {
                    &[]
                };
                // Read-only row: same layout as the input list so long device
                // names wrap instead of turning into overflow boxes.
                let row = device_row(ui, p, false, &d.name, suffix);
                let sum = d.summary();
                if !sum.is_empty() {
                    row.on_hover_text(format!("{} — {sum}", d.name));
                }
            }
        });

        ui.add_space(12.0);
        Self::card(ui, p, |ui| {
            ui.label(
                RichText::new("Bluetooth-гарнитуры (AirPods и др.)")
                    .size(13.0)
                    .strong()
                    .color(p.info),
            );
            ui.label(
                RichText::new(
                    "Микрофон наушников доступен только в профиле Hands-Free (моно, телефонное качество 8–16 кГц) — распознавание может быть хуже, чем у встроенного микрофона. «Наушники» (стерео A2DP) микрофона не имеют.",
                )
                .size(12.0)
                .color(p.muted),
            );
        });
    }

    // ------------------------------------------------------------ settings

    fn settings_page(&mut self, ui: &mut egui::Ui, p: Palette) {
        if self.settings.is_none() {
            self.settings = Some(SettingsDraft::new(&self.cfg));
        }
        let mut action: Option<SettingsAction> = None;
        {
            let draft = self.settings.as_mut().expect("settings draft opened above");
            if draft.stale {
                Self::card(ui, p, |ui| {
                    ui.label(
                        RichText::new("config.toml изменён извне")
                            .size(14.0)
                            .strong()
                            .color(p.warn),
                    );
                    ui.label(
                        RichText::new(
                            "В файле есть правки, сделанные вне UI, а у вас — несохранённые. Сохранение заблокировано. «Объединить» берёт ваши правки поверх файла (остальные поля — из файла), «Перечитать» сбрасывает черновик.",
                        )
                        .size(12.5)
                        .color(p.muted),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button("Объединить")
                            .on_hover_text(
                                "Сохранить мои правки, не теряя правки файла в других полях",
                            )
                            .clicked()
                        {
                            action = Some(SettingsAction::Merge);
                        }
                        if ui.button("Перечитать файл").clicked() {
                            action = Some(SettingsAction::Reload);
                        }
                    });
                });
                ui.add_space(10.0);
            }
            if let Some(err) = &draft.global_error {
                ui.colored_label(p.error, err);
            }
            for (section, title) in SETTINGS_SECTIONS {
                ui.add_space(8.0);
                Self::section_title(ui, p, title);
                Self::card(ui, p, |ui| {
                    for meta in SETTINGS_FIELDS.iter().filter(|m| m.id.starts_with(section)) {
                        Self::settings_row(ui, p, draft, meta, &mut self.capture);
                    }
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let (text, color) = if draft.dirty {
                    ("● Несохранённые изменения", p.warn)
                } else {
                    ("Изменений нет", p.muted)
                };
                ui.label(RichText::new(text).size(12.5).color(color));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            can_save(draft),
                            egui::Button::new(RichText::new("Сохранить").strong()),
                        )
                        .on_hover_text(
                            "Записать черновик в config.toml; поля без бейджа «после перезапуска» вступают в силу сразу",
                        )
                        .clicked()
                    {
                        action = Some(SettingsAction::Save);
                    }
                    if ui
                        .button("Сбросить к значениям по умолчанию")
                        .on_hover_text(
                            "Заполнить черновик значениями по умолчанию (запись — только по «Сохранить»)",
                        )
                        .clicked()
                    {
                        action = Some(SettingsAction::Reset);
                    }
                });
            });
        }
        match action {
            Some(SettingsAction::Save) => self.save_settings(),
            Some(SettingsAction::Reset) => {
                if let Some(d) = &mut self.settings {
                    d.cfg = Config::default();
                    d.dirty = true;
                    d.errors.clear();
                    d.global_error = None;
                }
            }
            Some(SettingsAction::Reload) => self.reload_settings_from_file(),
            Some(SettingsAction::Merge) => self.merge_settings(),
            None => {}
        }
    }

    /// Validate the draft, sync fields owned by other pages, write atomically,
    /// push the live subset into the daemon, journal the event.
    fn save_settings(&mut self) {
        let Some(d) = self.settings.as_mut() else {
            return;
        };
        if !can_save(d) {
            return;
        }
        // Fields also edited from other pages (header theme, Sound device,
        // Models activation) are re-read from the live config so «Сохранить»
        // cannot silently revert a change made elsewhere while the draft was
        // open (validate at the boundary, never lose an edit).
        sync_owned(d, &self.cfg);
        let (field_errors, global) = draft_check(&d.cfg);
        if !field_errors.is_empty() || global.is_some() {
            d.errors = field_errors;
            d.global_error = global;
            return;
        }
        match config::save(&self.config_path, &d.cfg) {
            Ok(()) => {
                if let (Some(shared), Some(flag)) = (&self.live_cfg, &self.transcripts) {
                    apply_live(shared, flag, &d.cfg);
                }
                self.cfg = d.cfg.clone();
                self.theme = Theme::from_config(&self.cfg.gui.theme);
                // Models folder changed: move installed models if the user
                // asked for it (default: files stay where they are).
                if let Some((old_root, new_root)) = d.pending_move.clone() {
                    let (old_root, new_root) = (
                        std::path::PathBuf::from(old_root),
                        std::path::PathBuf::from(new_root),
                    );
                    for m in download::MODELS {
                        let outcome = move_model_dir(&old_root, &new_root, m.name);
                        self.journal.append(
                            "download",
                            &format!(
                                "models move {} -> {}: {outcome}",
                                m.name,
                                new_root.display()
                            ),
                        );
                    }
                }
                self.journal.append(
                    "config",
                    &format!("settings saved ({} fields)", SETTINGS_FIELDS.len()),
                );
                d.mark_saved();
                d.move_models = false;
            }
            Err(e) => d.global_error = Some(e.to_string()),
        }
    }

    /// Merge external edits: re-read the file, keep fields edited in the
    /// draft, adopt the rest. Then the normal save path runs (validate +
    /// atomic write), so nothing invalid or partial reaches the disk.
    fn merge_settings(&mut self) {
        let Some(d) = self.settings.as_mut() else {
            return;
        };
        if let Err(e) = merge_reload(d, &self.config_path) {
            d.global_error = Some(format!("Не удалось объединить: {e}"));
            return;
        }
        self.save_settings();
    }

    /// External edit detected: reload from file (validates), re-open the
    /// draft, push the live subset to the daemon. An invalid file is a loud
    /// error: the draft stays stale and save stays blocked.
    fn reload_settings_from_file(&mut self) {
        let loaded = match config::load(&self.config_path) {
            Ok(c) => c,
            Err(e) => {
                if let Some(d) = &mut self.settings {
                    d.global_error = Some(format!("Не удалось перечитать файл: {e}"));
                }
                return;
            }
        };
        if let (Some(shared), Some(flag)) = (&self.live_cfg, &self.transcripts) {
            apply_live(shared, flag, &loaded);
        }
        self.cfg = loaded.clone();
        self.theme = Theme::from_config(&self.cfg.gui.theme);
        let mut fresh = SettingsDraft::new(&self.cfg);
        fresh.fresh = true;
        self.settings = Some(fresh);
        self.journal.append("config", "settings reloaded from file");
    }

    /// One settings row: label column, control, restart badge, error line.
    fn settings_row(
        ui: &mut egui::Ui,
        p: Palette,
        draft: &mut SettingsDraft,
        meta: &settings::FieldMeta,
        capture: &mut Option<&'static str>,
    ) {
        use settings::{field_mut, FieldValue, WidgetKind};
        let mut changed = false;
        ui.horizontal(|ui| {
            // Fixed-width label column keeps the controls aligned; the label
            // is LEFT-aligned (centred labels leave a ragged column).
            let label_w = SETTINGS_LABEL_WIDTH;
            ui.allocate_ui_with_layout(
                egui::vec2(label_w, 22.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new(meta.label).size(13.0).color(p.text))
                        .on_hover_text(meta.hint);
                },
            );
            let Some(value) = field_mut(&mut draft.cfg, meta.id) else {
                ui.colored_label(p.error, format!("неизвестное поле «{}»", meta.id));
                return;
            };
            match (meta.kind, value) {
                (WidgetKind::Bool, FieldValue::Bool(v)) => {
                    changed = ui.checkbox(v, "").changed();
                }
                (WidgetKind::Choice(opts), FieldValue::Str(v)) => {
                    egui::ComboBox::from_id_salt(meta.id)
                        .selected_text(v.as_str())
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            for opt in opts {
                                changed |=
                                    ui.selectable_value(v, opt.to_string(), *opt).changed();
                            }
                        });
                }
                (WidgetKind::Float { min, max }, FieldValue::F32(v)) => {
                    let speed = ((max - min) / 200.0).max(0.001);
                    changed = ui
                        .add(egui::DragValue::new(v).range(min..=max).speed(speed))
                        .changed();
                }
                (WidgetKind::UInt { min, max }, FieldValue::U32(v)) => {
                    let mut val = *v as u64;
                    if ui
                        .add(egui::DragValue::new(&mut val).range(min..=max))
                        .changed()
                    {
                        *v = val as u32;
                        changed = true;
                    }
                }
                (WidgetKind::UInt { min, max }, FieldValue::U64(v)) => {
                    changed = ui.add(egui::DragValue::new(v).range(min..=max)).changed();
                }
                (WidgetKind::Text, FieldValue::Str(v)) => {
                    let w = ui.available_width().min(320.0);
                    changed = ui
                        .add(egui::TextEdit::singleline(v).desired_width(w))
                        .changed();
                }
                (WidgetKind::Dir, FieldValue::Str(v)) => {
                    // Narrow window: text + «Выбрать…» + «Сброс» must fit
                    // next to the label column (min window is 760 px).
                    let w = (ui.available_width() - 170.0).clamp(60.0, 320.0);
                    if ui
                        .add(egui::TextEdit::singleline(v).desired_width(w))
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .button("Выбрать…")
                        .on_hover_text("Системный диалог выбора папки")
                        .clicked()
                    {
                        let start = if v.trim().is_empty() {
                            config::config_dir()
                        } else {
                            std::path::PathBuf::from(models_root_of(&draft.cfg))
                        };
                        let mut dialog = rfd::FileDialog::new()
                            .set_title("Папка для моделей распознавания");
                        if start.exists() {
                            dialog = dialog.set_directory(start);
                        }
                        if let Some(picked) = dialog.pick_folder() {
                            let choice = picked.display().to_string();
                            if set_models_dir(draft, &choice).is_some() {
                                changed = true;
                            }
                        }
                    }
                    if ui
                        .button("Сброс")
                        .on_hover_text("Вернуть папку по умолчанию (<config dir>/models)")
                        .clicked()
                    {
                        if set_models_dir(draft, "").is_some() {
                            changed = true;
                        }
                    }
                }
                (WidgetKind::Hotkey, FieldValue::Str(v)) => {
                    if *capture == Some(meta.id) {
                        ui.label(
                            RichText::new("нажмите комбинацию… (Esc — отмена)")
                                .size(12.5)
                                .color(p.warn),
                        );
                        let (key, mods) = capture_key(ui);
                        match key {
                            Some(egui::Key::Escape) => *capture = None,
                            Some(k) => {
                                if let Some(s) = capture_hotkey(mods, k) {
                                    if crate::hotkey::parse_hotkey(&s).is_ok() {
                                        *v = s;
                                        *capture = None;
                                        changed = true;
                                    }
                                }
                            }
                            None => {}
                        }
                    } else {
                        let w = (ui.available_width() - 90.0).clamp(80.0, 240.0);
                        changed = ui
                            .add(egui::TextEdit::singleline(v).desired_width(w))
                            .changed();
                        if ui
                            .button("Записать")
                            .on_hover_text(
                                "Нажать комбинацию на клавиатуре (клавиша Win не ловится — впишите вручную)",
                            )
                            .clicked()
                        {
                            *capture = Some(meta.id);
                        }
                    }
                }
                _ => {
                    ui.colored_label(p.error, "виджет не соответствует типу поля");
                }
            }
            if meta.restart {
                badge(ui, "после перезапуска", p.muted);
            }
        });
        // The models folder row gets a second line (full width) for the move
        // opt-in, so the button row above stays inside a narrow window.
        if meta.id == "asr.models_dir" {
            ui.horizontal(|ui| {
                ui.add_space(SETTINGS_LABEL_WIDTH + 10.0);
                if ui
                    .checkbox(&mut draft.move_models, "Переместить скачанные модели")
                    .on_hover_text(
                        "При сохранении перенести уже скачанные модели в новую папку. По умолчанию файлы остаются на месте.",
                    )
                    .changed()
                {
                    draft.dirty = true;
                    if draft.move_models {
                        draft.pending_move = Some((
                            models_root_of(&draft.baseline).display().to_string(),
                            models_root_of(&draft.cfg).display().to_string(),
                        ));
                    } else {
                        draft.pending_move = None;
                    }
                }
            });
        }
        if changed {
            draft.dirty = true;
            draft.errors.clear();
            draft.global_error = None;
        }
        if let Some(msg) = draft
            .errors
            .iter()
            .find(|(id, _)| *id == meta.id)
            .map(|(_, m)| m.clone())
        {
            ui.colored_label(p.error, format!("{}: {msg}", meta.label));
        }
        if meta.id == "gui.enabled" {
            if let Some(FieldValue::Bool(v)) = field_mut(&mut draft.cfg, "gui.enabled") {
                if !*v {
                    ui.label(
                        RichText::new(
                            "Окно не будет открываться при запуске — консольный режим. Вернуть окно: rapcoding --gui или правка конфига.",
                        )
                        .size(12.0)
                        .color(p.warn),
                    );
                }
            }
        }
    }

    fn models_card(&self, ui: &mut egui::Ui, p: Palette) {
        Self::card(ui, p, |ui| {
            Self::section_title(ui, p, "АКТИВНЫЕ МОДЕЛИ");
            ui.add_space(6.0);
            model_line(ui, p, "RU", &self.cfg.asr.model);
            ui.add_space(6.0);
            model_line(ui, p, "EN", &self.cfg.asr.en_model);
        });
    }

    // ---------------------------------------------------------------- help

    fn help(ui: &mut egui::Ui, p: Palette) {
        Self::card(ui, p, |ui| {
            ui.label(
                RichText::new("Как пользоваться")
                    .size(18.0)
                    .strong()
                    .color(p.text),
            );
            ui.add_space(10.0);
            let steps = [
                (
                    "1. Подготовьте модель",
                    "Откройте «Модели», нажмите «Скачать», дождитесь окончания, затем — «Сделать активной». Перезапустите программу.",
                ),
                (
                    "2. Поставьте курсор",
                    "Кликните в то место любого окна, куда должен появиться текст: редактор, браузер, мессенджер, терминал.",
                ),
                (
                    "3. Зажмите хоткей и говорите",
                    "Держите клавиши, пока говорите. Индикатор вверху показывает «Слушаю». Отпустите — начнётся распознавание.",
                ),
                (
                    "4. Текст появится сам",
                    "Через секунду текст напечатается в том окне, где был курсор. Не переключайте окна в этот момент.",
                ),
            ];
            for (title, body) in steps {
                ui.label(RichText::new(title).size(13.5).strong().color(p.text));
                ui.label(RichText::new(body).size(12.5).color(p.muted));
                ui.add_space(10.0);
            }
        });

        ui.add_space(12.0);
        Self::card(ui, p, |ui| {
            ui.label(
                RichText::new("Голосовые команды")
                    .size(16.0)
                    .strong()
                    .color(p.text),
            );
            ui.label(
                RichText::new(
                    "Скажите фразу — она заменится символом. Смешивать языки в одной фразе нельзя.",
                )
                .size(12.5)
                .color(p.muted),
            );
            ui.add_space(10.0);
            egui::Grid::new("voice_cmds")
                .num_columns(4)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    for (ru, en) in [
                        ("новая строка", "new line"),
                        ("новый абзац", "new paragraph"),
                        ("открыть скобку", "open parenthesis"),
                        ("закрыть скобку", "close parenthesis"),
                        ("двойное двоеточие", "double colon"),
                        ("точка с запятой", "fat arrow"),
                    ] {
                        ui.label(RichText::new(ru).size(12.5).color(p.text));
                        ui.label(RichText::new(en).monospace().size(12.0).color(p.muted));
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    }
                });
        });

        ui.add_space(12.0);
        Self::card(ui, p, |ui| {
            ui.label(
                RichText::new("Если не работает")
                    .size(16.0)
                    .strong()
                    .color(p.text),
            );
            ui.add_space(8.0);
            for (q, a) in [
                (
                    "Текст не появляется",
                    "Проверьте статус вверху: «Ошибка» — смотрите «Лог». Убедитесь, что модель активна.",
                ),
                (
                    "Ничего не слышно",
                    "В Windows выберите настоящий микрофон: Параметры » Звук » Ввод (не «Стерео микшер»). Список устройств: rapcoding devices.",
                ),
                (
                    "Не скачивается модель",
                    "Сайт Vosk может быть недоступен без VPN. Скачайте zip вручную и распакуйте в %APPDATA%\\rapcoding\\models.",
                ),
                (
                    "Не работает в программе от администратора",
                    "Windows блокирует ввод из обычного процесса в привилегированный: запустите rapcoding тоже от администратора.",
                ),
            ] {
                ui.label(RichText::new(q).size(13.0).strong().color(p.text));
                ui.label(RichText::new(a).size(12.5).color(p.muted));
                ui.add_space(9.0);
            }
        });
    }

    // -------------------------------------------------------------- models

    fn models(&mut self, ui: &mut egui::Ui, p: Palette) {
        let root = config::models_root(&self.cfg, &config::config_dir());
        let state = sync::lock_recover(&self.dl_state).clone();
        let busy = state.is_busy();

        // Where models go (SC-053): shown here because this is where people
        // look when a download lands somewhere unexpected.
        Self::card(ui, p, |ui| {
            Self::section_title(ui, p, "ПАПКА МОДЕЛЕЙ");
            ui.label(
                RichText::new(models_dir_display(&self.cfg))
                    .size(12.5)
                    .monospace()
                    .color(p.text),
            );
            if !root.exists() {
                ui.label(
                    RichText::new("Папки ещё нет — она будет создана при первой загрузке")
                        .size(12.0)
                        .color(p.muted),
                );
            }
            ui.label(
                RichText::new("Папку можно изменить в «Настройки», раздел «Папка моделей»")
                    .size(12.0)
                    .color(p.muted),
            );
        });
        ui.add_space(12.0);

        if let Some(err) = &self.delete_error {
            Self::card(ui, p, |ui| {
                ui.label(
                    RichText::new("Не удалось удалить модель")
                        .size(13.0)
                        .color(p.error)
                        .strong(),
                );
                ui.label(RichText::new(err).size(12.0).color(p.muted));
            });
            ui.add_space(12.0);
        }

        Self::card(ui, p, |ui| {
            ui.label(
                RichText::new("Более тяжёлые модели распознают точнее")
                    .size(16.0)
                    .strong()
                    .color(p.text),
            );
            ui.label(
                RichText::new(
                    "Скачайте модель, затем нажмите «Сделать активной» и перезапустите программу.",
                )
                .size(12.5)
                .color(p.muted),
            );
        });
        ui.add_space(12.0);

        for m in download::MODELS {
            let installed = download::model_dir(&root, m.name).exists();
            let active = if m.lang == "ru" {
                self.cfg.asr.model == m.config_value()
            } else {
                self.cfg.asr.en_model == m.config_value()
            };
            let is_current = match &state {
                DlState::Downloading { name, .. } | DlState::Extracting { name } => name == m.name,
                _ => false,
            };

            Self::card(ui, p, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(m.name).monospace().size(14.0).color(p.text));
                    ui.add_space(6.0);
                    badge(ui, if m.lang == "ru" { "RU" } else { "EN" }, p.accent);
                    ui.label(
                        RichText::new(download::human_size(m.size_bytes))
                            .size(12.0)
                            .color(p.muted),
                    );
                    if active {
                        badge(ui, "активна", p.success);
                    } else if installed {
                        badge(ui, "установлена", p.muted);
                    }
                });

                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if is_current && busy {
                        if ui
                            .button("Остановить")
                            .on_hover_text("Прервать загрузку и удалить неполный файл")
                            .clicked()
                        {
                            if let Some(flag) = &self.dl_cancel {
                                flag.store(true, Ordering::Relaxed);
                            }
                        }
                    } else if !installed {
                        if ui
                            .add_enabled(!busy, egui::Button::new("Скачать"))
                            .on_hover_text(
                                "Скачать модель с сайта Vosk (нужен интернет; размер указан рядом)",
                            )
                            .clicked()
                        {
                            self.start_download(m);
                        }
                    } else {
                        if !active {
                            if ui
                                .button("Сделать активной")
                                .on_hover_text(
                                    "Запишет модель в asr.model / asr.en_model; применится после перезапуска",
                                )
                                .clicked()
                            {
                                self.activate_model(m);
                            }
                        }
                        if self.delete_confirm == Some(m.name) {
                            if ui
                                .button("Да, удалить")
                                .on_hover_text("Удалить папку модели с диска — освободит место")
                                .clicked()
                            {
                                self.delete_model(m);
                            }
                            if ui.button("Отмена").clicked() {
                                self.delete_confirm = None;
                            }
                        } else if ui
                            .button("Удалить")
                            .on_hover_text("Стереть скачанную модель с диска")
                            .clicked()
                        {
                            self.delete_confirm = Some(m.name);
                        }
                    }
                });

                if self.delete_confirm == Some(m.name) {
                    ui.add_space(6.0);
                    if active {
                        ui.label(
                            RichText::new(
                                "Это активная модель: после удаления распознавание не заработает, пока не выберете другую.",
                            )
                            .size(11.5)
                            .color(p.error),
                        );
                    } else {
                        ui.label(
                            RichText::new(
                                "Будет удалена папка модели — место на диске освободится.",
                            )
                            .size(11.5)
                            .color(p.muted),
                        );
                    }
                }

                if is_current {
                    match &state {
                        DlState::Downloading {
                            received, total, ..
                        } => {
                            let frac = if *total > 0 {
                                (*received as f32 / *total as f32).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            ui.add_space(8.0);
                            // Cap the bar to the visible width: an infinite
                            // desired width overflows the card on Windows.
                            let bar_w = ui.available_width();
                            ui.add(egui::ProgressBar::new(frac).desired_width(bar_w).text(
                                format!(
                                    "{} / {}",
                                    download::human_size(*received),
                                    download::human_size(*total)
                                ),
                            ));
                        }
                        DlState::Extracting { .. } => {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new());
                                ui.label(RichText::new("распаковка…").size(12.5).color(p.muted));
                            });
                        }
                        _ => {}
                    }
                }
            });
            ui.add_space(10.0);

            if is_current
                && matches!(
                    state,
                    DlState::Done { .. } | DlState::Canceled | DlState::Error(_)
                )
            {
                break;
            }
        }

        match &state {
            DlState::Done { name } => {
                Self::card(ui, p, |ui| {
                    ui.label(
                        RichText::new(format!("{name}: скачивание завершено"))
                            .size(13.0)
                            .color(p.success)
                            .strong(),
                    );
                });
            }
            DlState::Canceled => {
                Self::card(ui, p, |ui| {
                    ui.label(
                        RichText::new("Скачивание остановлено")
                            .size(13.0)
                            .color(p.muted),
                    );
                });
            }
            DlState::Error(e) => {
                Self::card(ui, p, |ui| {
                    ui.label(
                        RichText::new("Не удалось скачать")
                            .size(13.0)
                            .color(p.error)
                            .strong(),
                    );
                    ui.label(RichText::new(e).size(12.0).color(p.muted));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "Проверьте подключение: сайт Vosk недоступен без VPN в некоторых сетях.",
                        )
                        .size(12.0)
                        .color(p.muted),
                    );
                });
            }
            _ => {}
        }
    }

    // ----------------------------------------------------------------- log

    fn logs(&mut self, ui: &mut egui::Ui, p: Palette) {
        Self::card(ui, p, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Живой лог").size(16.0).strong().color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Экспорт…").clicked() {
                        if let Some(dst) = rfd::FileDialog::new()
                            .set_file_name("rapcoding.log")
                            .save_file()
                        {
                            let src = config::config_dir().join("rapcoding.log");
                            self.journal
                                .append("log", &format!("export -> {}", dst.display()));
                            self.export_error = match log::export_log(&src, &dst) {
                                Ok(()) => None,
                                Err(e) => Some(e.to_string()),
                            };
                        }
                    }
                });
            });
            if let Some(err) = &self.export_error {
                ui.colored_label(p.error, err);
            }
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(p.bg)
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(460.0)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let job = styled_log(&self.live.snapshot(), &p);
                            ui.label(job);
                        });
                });
        });
    }

    fn journal_page(&mut self, ui: &mut egui::Ui, p: Palette) {
        Self::card(ui, p, |ui| {
            ui.label(
                RichText::new("Журнал управления")
                    .size(16.0)
                    .strong()
                    .color(p.text),
            )
            .on_hover_text("Старт сессий, смена темы, загрузка моделей и активация");
            ui.add_space(8.0);
            let entries = self.journal.read();
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(p.bg)
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(440.0)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let job = styled_journal(&entries, &p);
                            ui.label(job);
                        });
                });
        });
    }

    // ------------------------------------------------------------- actions

    fn new_cancel_flag(&mut self) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.dl_cancel = Some(flag.clone());
        flag
    }

    fn start_pending_downloads(&mut self) {
        let path = config::config_dir().join("pending-models.txt");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let _ = std::fs::remove_file(&path);
        let wanted: Vec<&'static download::Model> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .filter_map(|name| download::MODELS.iter().find(|m| m.name == name))
            .collect();
        if wanted.is_empty() {
            return;
        }
        let state = self.dl_state.clone();
        let journal = self.journal.clone();
        let root = config::models_root(&self.cfg, &config::config_dir());
        let cancel = self.new_cancel_flag();
        self.page = Page::Models;
        journal.append(
            "download",
            &format!("queued by installer: {} model(s)", wanted.len()),
        );
        std::thread::spawn(move || {
            let set = |s: DlState| {
                *sync::lock_recover(&state) = s;
            };
            for (idx, m) in wanted.iter().enumerate() {
                let name = m.name.to_string();
                set(DlState::Downloading {
                    name: name.clone(),
                    received: 0,
                    total: m.size_bytes,
                });
                let zip_tmp = root.join(format!("{name}.zip.part"));
                let result = download::download(
                    m.archive_url(),
                    &zip_tmp,
                    &mut |done, total| {
                        set(DlState::Downloading {
                            name: name.clone(),
                            received: done,
                            total,
                        });
                    },
                    &cancel,
                )
                .and_then(|()| {
                    set(DlState::Extracting { name: name.clone() });
                    download::extract_zip(&zip_tmp, &root)
                });
                let _ = std::fs::remove_file(&zip_tmp);
                if cancel.load(Ordering::Relaxed) {
                    journal.append("download", &format!("canceled {name}"));
                    set(DlState::Canceled);
                    return;
                }
                match result {
                    Ok(()) => journal.append("download", &format!("ok {name}")),
                    Err(e) => {
                        journal.append("download", &format!("fail {name}: {e}"));
                        set(DlState::Error(e.to_string()));
                        return;
                    }
                }
                if idx + 1 == wanted.len() {
                    set(DlState::Done {
                        name: format!("моделей: {}", wanted.len()),
                    });
                }
            }
        });
    }

    fn start_download(&mut self, m: &'static download::Model) {
        let state = self.dl_state.clone();
        let journal = self.journal.clone();
        let root = config::models_root(&self.cfg, &config::config_dir());
        let name = m.name.to_string();
        let url = m.archive_url();
        let size = m.size_bytes;
        let cancel = self.new_cancel_flag();
        journal.append("download", &format!("start {name}"));
        std::thread::spawn(move || {
            let set = |s: DlState| {
                *sync::lock_recover(&state) = s;
            };
            set(DlState::Downloading {
                name: name.clone(),
                received: 0,
                total: size,
            });
            let zip_tmp = root.join(format!("{name}.zip.part"));
            let result = download::download(
                url,
                &zip_tmp,
                &mut |done, total| {
                    set(DlState::Downloading {
                        name: name.clone(),
                        received: done,
                        total,
                    });
                },
                &cancel,
            )
            .and_then(|()| {
                set(DlState::Extracting { name: name.clone() });
                download::extract_zip(&zip_tmp, &root)
            });
            let _ = std::fs::remove_file(&zip_tmp);
            if cancel.load(Ordering::Relaxed) {
                journal.append("download", &format!("canceled {name}"));
                set(DlState::Canceled);
                return;
            }
            match result {
                Ok(()) => {
                    journal.append("download", &format!("ok {name}"));
                    set(DlState::Done { name });
                }
                Err(e) => {
                    journal.append("download", &format!("fail {name}: {e}"));
                    set(DlState::Error(e.to_string()));
                }
            }
        });
    }

    /// Remove an installed model from disk (with confirmation done in the UI).
    fn delete_model(&mut self, m: &'static download::Model) {
        let root = config::models_root(&self.cfg, &config::config_dir());
        match download::remove_model(&root, m.name) {
            Ok(()) => {
                self.journal
                    .append("download", &format!("delete model {}", m.name));
                self.delete_confirm = None;
                self.delete_error = None;
            }
            Err(e) => {
                self.delete_error = Some(e.to_string());
            }
        }
    }

    fn activate_model(&mut self, m: &'static download::Model) {
        let value = m.config_value();
        if m.lang == "ru" {
            self.cfg.asr.model = value;
        } else {
            self.cfg.asr.en_model = value;
        }
        self.journal
            .append("config", &format!("activate model {} ({})", m.name, m.lang));
        self.persist_error = match config::save(&self.config_path, &self.cfg) {
            Ok(()) => None,
            Err(e) => Some(e.to_string()),
        };
    }
}

// ------------------------------------------------------- settings machinery (ui)

/// Width of the settings label column, in points. Also used to indent the
/// second line of the models-folder row (min window width is 760 px).
const SETTINGS_LABEL_WIDTH: f32 = 160.0;

/// What a user action on the settings page resolves to (executed after the
/// render loop so the draft borrow can end first).
enum SettingsAction {
    Save,
    Reset,
    Reload,
    Merge,
}

/// Push a saved config into the shared daemon state: the live subset takes
/// effect on the next utterance / loop tick without a restart.
fn apply_live(shared: &RwLock<Config>, transcripts: &AtomicBool, cfg: &Config) {
    *sync::write_recover(shared) = cfg.clone();
    transcripts.store(cfg.general.log_transcripts, Ordering::Relaxed);
}

/// True when the file on disk holds values the draft was not based on.
fn file_changed_externally(d: &SettingsDraft, path: &std::path::Path) -> bool {
    match config::load(path) {
        Ok(current) if !d.dirty => {
            settings::field_differs(&current, "general.mode", &d.baseline)
                || settings::external_change(d, &current).is_some()
        }
        Ok(current) => settings::is_stale(d, &current),
        // An unreadable/invalid file still means "it changed on us".
        Err(_) => true,
    }
}

/// Compose a hotkey string from egui modifiers + main key in the spelling
/// [`crate::hotkey::parse_hotkey`] accepts. None when the key cannot be a
/// main key. Note: egui does not report the Win key, so "win" combos must
/// be typed by hand (the text field accepts them).
fn capture_hotkey(mods: egui::Modifiers, key: egui::Key) -> Option<String> {
    let main = egui_key_name(key)?;
    let mut parts: Vec<&str> = Vec::new();
    if mods.ctrl {
        parts.push("ctrl");
    }
    if mods.alt {
        parts.push("alt");
    }
    if mods.shift {
        parts.push("shift");
    }
    parts.push(main);
    Some(parts.join("+"))
}

/// Lowercase name of an egui key, matching what the hotkey parser accepts.
/// Covers letters, digits, F1-F24, punctuation, arrows and navigation —
/// enough for any practical shortcut; unknown keys return None (and the
/// capture widget ignores them, never emitting an unparseable string).
fn egui_key_name(k: egui::Key) -> Option<&'static str> {
    use egui::Key as K;
    Some(match k {
        K::A => "a",
        K::B => "b",
        K::C => "c",
        K::D => "d",
        K::E => "e",
        K::F => "f",
        K::G => "g",
        K::H => "h",
        K::I => "i",
        K::J => "j",
        K::K => "k",
        K::L => "l",
        K::M => "m",
        K::N => "n",
        K::O => "o",
        K::P => "p",
        K::Q => "q",
        K::R => "r",
        K::S => "s",
        K::T => "t",
        K::U => "u",
        K::V => "v",
        K::W => "w",
        K::X => "x",
        K::Y => "y",
        K::Z => "z",
        K::Num0 => "0",
        K::Num1 => "1",
        K::Num2 => "2",
        K::Num3 => "3",
        K::Num4 => "4",
        K::Num5 => "5",
        K::Num6 => "6",
        K::Num7 => "7",
        K::Num8 => "8",
        K::Num9 => "9",
        K::F1 => "f1",
        K::F2 => "f2",
        K::F3 => "f3",
        K::F4 => "f4",
        K::F5 => "f5",
        K::F6 => "f6",
        K::F7 => "f7",
        K::F8 => "f8",
        K::F9 => "f9",
        K::F10 => "f10",
        K::F11 => "f11",
        K::F12 => "f12",
        K::F13 => "f13",
        K::F14 => "f14",
        K::F15 => "f15",
        K::F16 => "f16",
        K::F17 => "f17",
        K::F18 => "f18",
        K::F19 => "f19",
        K::F20 => "f20",
        K::F21 => "f21",
        K::F22 => "f22",
        K::F23 => "f23",
        K::F24 => "f24",
        K::Space => "space",
        K::Enter => "enter",
        K::Tab => "tab",
        K::Backspace => "backspace",
        K::Escape => "escape",
        K::Comma => "comma",
        K::Period => "period",
        K::Slash => "slash",
        K::Semicolon => "semicolon",
        K::Quote => "quote",
        K::OpenBracket => "bracketleft",
        K::CloseBracket => "bracketright",
        K::Backslash => "backslash",
        K::Backtick => "backquote",
        K::Minus => "minus",
        K::Equals => "equal",
        K::ArrowUp => "arrowup",
        K::ArrowDown => "arrowdown",
        K::ArrowLeft => "arrowleft",
        K::ArrowRight => "arrowright",
        K::Home => "home",
        K::End => "end",
        K::PageUp => "pageup",
        K::PageDown => "pagedown",
        K::Insert => "insert",
        K::Delete => "delete",
        _ => return None,
    })
}

/// Extract one pressed key + its modifiers from this frame's input. Key
/// events are consumed so a capture in progress is not typed into other
/// text fields; everything else (mouse, focus) passes through.
fn capture_key(ui: &mut egui::Ui) -> (Option<egui::Key>, egui::Modifiers) {
    let mut out = (None, egui::Modifiers::NONE);
    ui.input_mut(|i| {
        i.events.retain(|ev| match ev {
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } => {
                out = (Some(*key), *modifiers);
                false
            }
            _ => true,
        });
    });
    out
}

// ------------------------------------------------------- models root (SC-053)

/// The models root for a config, without touching the filesystem.
fn models_root_of(cfg: &Config) -> std::path::PathBuf {
    config::models_root(cfg, &config::config_dir())
}

/// Human-readable models folder: the configured path, or the platform
/// default with an explicit marker (so "empty" never looks like a bug).
fn models_dir_display(cfg: &Config) -> String {
    if cfg.asr.models_dir.trim().is_empty() {
        format!("по умолчанию: {}", models_root_of(cfg).display())
    } else {
        models_root_of(cfg).display().to_string()
    }
}

/// Apply a models-folder change to a draft in place: validate, migrate
/// active model paths, keep the old root for follow-up bookkeeping (moving
/// installed model folders) and record the journal note.
///
/// Returns the number of model paths that followed the new root; a validation
/// error is left in `draft.errors` and the draft is not changed.
fn set_models_dir(draft: &mut SettingsDraft, new_dir: &str) -> Option<(usize, String)> {
    let trimmed = new_dir.trim();
    if let Err(e) = config::validate_models_dir(trimmed) {
        draft.errors.retain(|(id, _)| id != "asr.models_dir");
        draft.errors.push(("asr.models_dir".into(), e.to_string()));
        return None;
    }
    let old_root = models_root_of(&draft.cfg);
    draft.cfg.asr.models_dir = trimmed.to_string();
    let new_root = models_root_of(&draft.cfg);
    // migrate_model_paths keeps relative values relative; absolute values
    // inside the old root are rewritten so they follow the new folder.
    let moved = config::migrate_model_paths(&mut draft.cfg, &old_root, &new_root);
    // An absolute stored folder switched back to a different one: make the
    // relative paths absolute again so resolution cannot fall back to the
    // old default root.
    for slot in [&mut draft.cfg.asr.model, &mut draft.cfg.asr.en_model] {
        let p = std::path::Path::new(slot.as_str());
        if !p.is_absolute() {
            *slot = new_root.join(p).display().to_string();
        }
    }
    draft.dirty = true;
    draft.baseline.asr.model = draft.cfg.asr.model.clone();
    draft.baseline.asr.en_model = draft.cfg.asr.en_model.clone();
    draft.baseline.asr.models_dir = draft.cfg.asr.models_dir.clone();
    Some((
        moved,
        format!(
            "models dir -> {} ({} model path(s) followed)",
            if trimmed.is_empty() {
                format!("default {}", new_root.display())
            } else {
                new_root.display().to_string()
            },
            moved
        ),
    ))
}

/// Recursively move an installed model folder from the old root to the new
/// one. Never overwrites: an existing target is left alone and reported.
/// Returns "moved", "skipped-exists" or "skipped-absent".
fn move_model_dir(
    old_root: &std::path::Path,
    new_root: &std::path::Path,
    name: &str,
) -> &'static str {
    let from = download::model_dir(old_root, name);
    let to = download::model_dir(new_root, name);
    if !from.is_dir() {
        return "skipped-absent";
    }
    if to.exists() {
        return "skipped-exists";
    }
    if let Some(parent) = to.parent() {
        if let Err(_e) = std::fs::create_dir_all(parent) {
            return "failed";
        }
    }
    match std::fs::rename(&from, &to) {
        Ok(()) => "moved",
        // Cross-device move: fall back to copy + remove.
        Err(_) => match copy_dir_all(&from, &to) {
            Ok(()) => {
                let _ = std::fs::remove_dir_all(&from);
                "moved"
            }
            Err(_) => "failed",
        },
    }
}

fn copy_dir_all(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------ sound rows

/// One device row: full-width, wrapped text, readable in every widget state.
///
/// Built by hand instead of `ui.selectable_value` because (a) `Button`
/// truncates by default, so a long Windows endpoint name (70+ chars) turns
/// into clipping boxes instead of wrapping, and (b) the selected fill must
/// keep the palette text colour on top (egui uses `selection.stroke` as the
/// selected text colour, which we set to the text colour in `visuals`).
fn device_row(
    ui: &mut egui::Ui,
    p: Palette,
    selected: bool,
    name: &str,
    suffix: &[(&str, Color32)],
) -> egui::Response {
    let width = ui.available_width();
    // Wrap the long endpoint name instead of truncating it (the "boxes" in the
    // device lists were clipped glyphs from the default truncating button).
    let mut job = LayoutJob::default();
    job.wrap.max_width = (width - 22.0).max(80.0);
    job.append(
        name,
        0.0,
        TextFormat {
            font_id: FontId::proportional(13.0),
            color: p.text,
            ..Default::default()
        },
    );
    for (label, color) in suffix {
        job.append(
            &format!("  {label}"),
            0.0,
            TextFormat {
                font_id: FontId::proportional(11.5),
                color: *color,
                ..Default::default()
            },
        );
    }
    let galley = ui.fonts(|f| f.layout_job(job));
    // Row height follows the actual galley (one or two lines), + padding.
    let height = galley.size().y + 14.0;
    let desired = egui::vec2(width, height);
    // Predict the hover state cheaply: `interact` needs the rect, and the
    // background must be painted BEFORE the text, so use the previous frame's
    // pointer position against the rect we are about to allocate.
    let mut row_rect = ui.available_rect_before_wrap();
    row_rect.max.y = row_rect.min.y + height;
    let hovered = ui
        .ctx()
        .pointer_latest_pos()
        .map_or(false, |p| row_rect.contains(p))
        && ui.ctx().is_pointer_over_area();
    let fill = if selected {
        p.accent.gamma_multiply(0.28)
    } else if hovered {
        p.surface
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(row_rect, CornerRadius::same(8), fill);
    }
    let inner = ui.allocate_ui(desired, |ui| {
        ui.set_min_size(desired);
        ui.add_space(7.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(egui::WidgetText::Galley(galley));
        });
        ui.interact(
            ui.max_rect(),
            ui.id().with(("device_row", name)),
            egui::Sense::click(),
        )
    });
    let response = inner.inner;
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

// ------------------------------------------------------------- small views

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.14))
        .corner_radius(CornerRadius::same(20))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color).strong());
        });
}

fn model_line(ui: &mut egui::Ui, p: Palette, lang: &str, value: &str) {
    ui.horizontal(|ui| {
        badge(ui, lang, p.accent);
        ui.label(
            RichText::new(value.rsplit('/').next().unwrap_or(value))
                .monospace()
                .size(12.5)
                .color(p.text),
        );
    });
}

// ------------------------------------------------- styled log / journal

/// Split a leading `[...]` stamp: `"[ts] body"` -> `(Some("[ts]"), " body")`.
fn split_stamp(line: &str) -> (Option<&str>, &str) {
    if let Some(end) = line.strip_prefix('[').and_then(|r| r.find(']')) {
        let cut = end + 2; // '[' + chars + ']'
        (Some(&line[..cut]), &line[cut..])
    } else {
        (None, line)
    }
}

/// Split `" kind rest"` -> `(" ", "kind", " rest")` (leading space kept).
fn split_kind(rest: &str) -> (&str, &str, &str) {
    let body = rest.trim_start();
    let lead = &rest[..rest.len() - body.len()];
    match body.find(' ') {
        Some(i) => (lead, &body[..i], &body[i..]),
        None => (lead, body, ""),
    }
}

fn fmt(color: Color32) -> TextFormat {
    TextFormat {
        font_id: FontId::monospace(11.5),
        color,
        ..Default::default()
    }
}

/// Colour of a journal event kind (and its outcome) in the current theme.
fn journal_color(kind: &str, msg: &str, p: &Palette) -> Color32 {
    match kind {
        "session" => p.accent,
        "config" => p.info,
        "log" => p.muted,
        "download" => {
            let m = msg.trim_start();
            if m.starts_with("ok") {
                p.success
            } else if m.starts_with("fail") {
                p.error
            } else if m.starts_with("canceled") {
                p.muted
            } else {
                p.warn
            }
        }
        _ => p.text,
    }
}

/// Colour of a log level.
fn log_color(level: &str, p: &Palette) -> Color32 {
    match level {
        "WARN" => p.warn,
        "DEBUG" => p.muted,
        _ => p.info,
    }
}

fn styled_journal(entries: &[String], p: &Palette) -> LayoutJob {
    let mut job = LayoutJob::default();
    if entries.is_empty() {
        job.append("Пока пусто.", 0.0, fmt(p.muted));
        return job;
    }
    for (i, line) in entries.iter().enumerate() {
        if i > 0 {
            job.append("\n", 0.0, fmt(p.text));
        }
        let (stamp, rest) = split_stamp(line);
        if let Some(s) = stamp {
            job.append(s, 0.0, fmt(p.muted));
        }
        let (lead, kind, msg) = split_kind(rest);
        if !lead.is_empty() {
            job.append(lead, 0.0, fmt(p.muted));
        }
        if !kind.is_empty() {
            job.append(kind, 0.0, fmt(journal_color(kind, msg, p)));
        }
        if !msg.is_empty() {
            job.append(msg, 0.0, fmt(p.text));
        }
    }
    job
}

fn styled_log(lines: &[String], p: &Palette) -> LayoutJob {
    let mut job = LayoutJob::default();
    if lines.is_empty() {
        job.append("Пока нет записей.", 0.0, fmt(p.muted));
        return job;
    }
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            job.append("\n", 0.0, fmt(p.text));
        }
        let (stamp, rest) = split_stamp(line);
        if let Some(s) = stamp {
            job.append(s, 0.0, fmt(p.muted));
        }
        let (lead, level, msg) = split_kind(rest);
        if !lead.is_empty() {
            job.append(lead, 0.0, fmt(p.muted));
        }
        if !level.is_empty() {
            job.append(level, 0.0, fmt(log_color(level, p)));
        }
        if !msg.is_empty() {
            job.append(msg, 0.0, fmt(p.text));
        }
    }
    job
}

/// "ctrl+alt+space" -> "Ctrl + Alt + Space" for readability.
fn pretty_hotkey(s: &str) -> String {
    s.split('+')
        .map(|t| {
            let t = t.trim();
            let mut c = t.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_stamp_handles_journal_line() {
        let (stamp, rest) = split_stamp("[2026-10-21T00:00:00Z] session start");
        assert_eq!(stamp, Some("[2026-10-21T00:00:00Z]"));
        assert_eq!(rest, " session start");
    }

    #[test]
    fn split_stamp_without_bracket() {
        let (stamp, rest) = split_stamp("plain line");
        assert_eq!(stamp, None);
        assert_eq!(rest, "plain line");
    }

    #[test]
    fn split_kind_extracts_token() {
        assert_eq!(split_kind(" session start"), (" ", "session", " start"));
        assert_eq!(split_kind(" INFO  hello"), (" ", "INFO", "  hello"));
        assert_eq!(split_kind("alone"), ("", "alone", ""));
    }

    #[test]
    fn journal_colors_by_kind() {
        let p = Palette::light();
        assert_eq!(journal_color("session", " start", &p), p.accent);
        assert_eq!(journal_color("config", " theme", &p), p.info);
        assert_eq!(journal_color("log", " export", &p), p.muted);
        assert_eq!(journal_color("download", " ok model", &p), p.success);
        assert_eq!(journal_color("download", " fail model", &p), p.error);
        assert_eq!(journal_color("download", " start model", &p), p.warn);
        assert_eq!(journal_color("download", " canceled model", &p), p.muted);
    }

    #[test]
    fn log_colors_by_level() {
        let p = Palette::dark();
        assert_eq!(log_color("WARN", &p), p.warn);
        assert_eq!(log_color("DEBUG", &p), p.muted);
        assert_eq!(log_color("INFO", &p), p.info);
    }

    #[test]
    fn styled_journal_keeps_text_intact() {
        // Colouring must not drop or reorder characters.
        let p = Palette::light();
        let lines = vec!["[ts] download ok model".to_string()];
        let job = styled_journal(&lines, &p);
        assert_eq!(job.text, "[ts] download ok model");
    }

    #[test]
    fn styled_log_keeps_text_intact() {
        let p = Palette::dark();
        let lines = vec!["[  1.000s] INFO  hello".to_string()];
        let job = styled_log(&lines, &p);
        assert_eq!(job.text, "[  1.000s] INFO  hello");
    }

    #[test]
    fn pretty_hotkey_capitalizes() {
        assert_eq!(pretty_hotkey("ctrl+alt+space"), "Ctrl + Alt + Space");
        assert_eq!(pretty_hotkey("f9"), "F9");
    }

    #[test]
    fn embedded_icon_decodes() {
        let icon = app_icon().expect("embedded ICO must decode");
        assert!(icon.width > 0 && icon.height > 0);
        assert_eq!(icon.rgba.len() as u32, icon.width * icon.height * 4);
    }

    #[test]
    fn theme_roundtrip() {
        assert_eq!(Theme::from_config("dark").as_str(), "dark");
        assert_eq!(Theme::from_config("light").as_str(), "light");
        assert_eq!(Theme::from_config("bogus").as_str(), "light");
        assert_eq!(Theme::Light.toggle(), Theme::Dark);
    }

    #[test]
    fn dl_state_busy() {
        assert!(DlState::Downloading {
            name: "x".into(),
            received: 1,
            total: 2
        }
        .is_busy());
        assert!(DlState::Extracting { name: "x".into() }.is_busy());
        assert!(!DlState::Idle.is_busy());
        assert!(!DlState::Canceled.is_busy());
    }

    #[test]
    fn capture_composes_parseable_hotkeys() {
        // Arrange: ctrl+alt pressed, matrix of main keys.
        let mods = egui::Modifiers {
            ctrl: true,
            alt: true,
            ..egui::Modifiers::NONE
        };
        for (key, main) in [
            (egui::Key::F9, "f9"),
            (egui::Key::A, "a"),
            (egui::Key::Space, "space"),
            (egui::Key::Comma, "comma"),
        ] {
            // Act.
            let s = capture_hotkey(mods, key).expect("must compose");
            // Assert: parses back and equals the canonical spelling.
            let parsed = crate::hotkey::parse_hotkey(&s).expect("must parse");
            assert_eq!(
                crate::hotkey::format_hotkey(&parsed).unwrap(),
                format!("ctrl+alt+{main}")
            );
        }
    }

    #[test]
    fn capture_rejects_non_main_keys() {
        // Assert: clipboard/editing pseudo-keys are not usable main keys.
        assert!(egui_key_name(egui::Key::Cut).is_none());
        assert!(egui_key_name(egui::Key::Copy).is_none());
        assert!(egui_key_name(egui::Key::Paste).is_none());
        // escape is a valid main key by itself.
        assert_eq!(
            capture_hotkey(egui::Modifiers::NONE, egui::Key::Escape).as_deref(),
            Some("escape")
        );
    }

    // ---------------------------------------------------- models dir (SC-053)

    #[test]
    fn models_dir_display_marks_the_default() {
        // Arrange.
        let cfg = Config::default();
        // Act.
        let shown = models_dir_display(&cfg);
        // Assert: an empty setting is explained, not shown as blank.
        assert!(shown.starts_with("по умолчанию:"), "{shown}");
        assert!(shown.contains("models"), "{shown}");
    }

    #[test]
    fn models_dir_display_shows_a_configured_path() {
        // Arrange.
        let dir = std::env::temp_dir().join(format!("rapcoding-md-show-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = Config::default();
        cfg.asr.models_dir = dir.display().to_string();
        // Act.
        let shown = models_dir_display(&cfg);
        // Assert.
        assert!(!shown.contains("по умолчанию"), "{shown}");
        assert!(shown.contains("rapcoding-md-show"), "{shown}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_models_dir_rejects_a_file_and_keeps_the_draft() {
        // Arrange: a regular file in place of the folder.
        let base = std::env::temp_dir().join(format!("rapcoding-md-rej-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("f");
        std::fs::write(&file, b"x").unwrap();
        let mut d = SettingsDraft::new(&Config::default());
        let before = d.cfg.clone();
        // Act.
        let res = set_models_dir(&mut d, &file.display().to_string());
        // Assert: loud field error, config unchanged, not marked dirty.
        assert!(res.is_none());
        assert_eq!(d.cfg, before);
        assert!(!d.dirty);
        assert!(d.errors.iter().any(|(id, _)| id == "asr.models_dir"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn set_models_dir_accepts_an_existing_folder() {
        // Arrange.
        let dir = std::env::temp_dir().join(format!("rapcoding-md-acc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = SettingsDraft::new(&Config::default());
        // Act.
        let res = set_models_dir(&mut d, &dir.display().to_string());
        // Assert: stored, dirty, journal note mentions the folder.
        let (_moved, note) = res.expect("valid folder must be accepted");
        assert_eq!(d.cfg.asr.models_dir, dir.display().to_string());
        assert!(d.dirty);
        assert!(note.contains("models dir ->"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_models_dir_pins_relative_model_paths_to_the_new_root() {
        // Arrange: default relative values; switch to a custom folder.
        let dir = std::env::temp_dir().join(format!("rapcoding-md-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = SettingsDraft::new(&Config::default());
        // Act.
        set_models_dir(&mut d, &dir.display().to_string()).unwrap();
        // Assert: active models now resolve inside the new folder, not the
        // old default (which still contains them).
        let new_root = config::models_root(&d.cfg, &config::config_dir());
        assert!(std::path::Path::new(&d.cfg.asr.model).is_absolute());
        assert!(std::path::Path::new(&d.cfg.asr.model).starts_with(&new_root));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_models_dir_back_to_default_restores_relative_paths() {
        // Arrange: custom folder, then «По умолчанию».
        let dir = std::env::temp_dir().join(format!("rapcoding-md-back-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = SettingsDraft::new(&Config::default());
        set_models_dir(&mut d, &dir.display().to_string()).unwrap();
        // Act.
        set_models_dir(&mut d, "").unwrap();
        // Assert: empty setting; model paths resolve in the default root.
        assert_eq!(d.cfg.asr.models_dir, "");
        let default_root = config::config_dir().join("models");
        assert!(std::path::Path::new(&d.cfg.asr.model).starts_with(&default_root));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_model_dir_moves_an_installed_folder() {
        // Arrange: a fake model in the old root.
        let base = std::env::temp_dir().join(format!("rapcoding-md-move-{}", std::process::id()));
        let old = base.join("old");
        let new = base.join("new");
        std::fs::create_dir_all(old.join("m1")).unwrap();
        std::fs::write(old.join("m1").join("am.bin"), b"model-bytes").unwrap();
        // Act.
        let outcome = move_model_dir(&old, &new, "m1");
        // Assert: moved, content intact, source gone.
        assert_eq!(outcome, "moved");
        assert_eq!(
            std::fs::read(new.join("m1").join("am.bin")).unwrap(),
            b"model-bytes"
        );
        assert!(!old.join("m1").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn move_model_dir_never_overwrites_an_existing_target() {
        // Arrange: both roots already hold a folder with that name.
        let base = std::env::temp_dir().join(format!("rapcoding-md-clash-{}", std::process::id()));
        let old = base.join("old");
        let new = base.join("new");
        std::fs::create_dir_all(old.join("m1")).unwrap();
        std::fs::create_dir_all(new.join("m1")).unwrap();
        std::fs::write(new.join("m1").join("keep"), b"keep").unwrap();
        // Act.
        let outcome = move_model_dir(&old, &new, "m1");
        // Assert: target untouched, source intact, reported as skipped.
        assert_eq!(outcome, "skipped-exists");
        assert_eq!(std::fs::read(new.join("m1").join("keep")).unwrap(), b"keep");
        assert!(old.join("m1").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn move_model_dir_reports_a_missing_source() {
        // Arrange: nothing installed.
        let base = std::env::temp_dir().join(format!("rapcoding-md-absent-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        // Act.
        let outcome = move_model_dir(&base.join("old"), &base.join("new"), "nope");
        // Assert.
        assert_eq!(outcome, "skipped-absent");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn move_models_flag_is_off_by_default() {
        // Arrange/Act: a fresh draft must not promise a move.
        let d = SettingsDraft::new(&Config::default());
        // Assert: files stay put unless the user opts in.
        assert!(!d.move_models);
        assert!(d.pending_move.is_none());
    }
}
