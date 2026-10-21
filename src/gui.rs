//! Native GUI (egui/eframe): a proper app shell — header, side navigation,
//! card-based content, custom light/dark palettes.
//!
//! Compiled only with `--features gui`. The recognition daemon runs on a
//! background thread and reports live status through the shared `Status`;
//! log lines stream through `LiveLog`, management events land in `Journal`,
//! and models are downloaded/extracted by `download`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke};

use crate::config::{self, Config};
use crate::download;
use crate::error::Result;
use crate::journal::Journal;
use crate::log::{self, LiveLog};
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
        v.selection.bg_fill = p.accent.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0_f32, p.accent);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.stroke);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(8);
            w.bg_stroke = Stroke::new(1.0_f32, p.stroke);
        }
        v.widgets.inactive.weak_bg_fill = p.surface;
        v.widgets.inactive.bg_fill = p.surface;
        v.widgets.hovered.weak_bg_fill = p.bg;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
        v.widgets.active.weak_bg_fill = p.accent.gamma_multiply(0.25);
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
    Models,
    Log,
    Journal,
}

impl Page {
    const ALL: [Page; 4] = [Page::Overview, Page::Models, Page::Log, Page::Journal];

    fn title(self) -> &'static str {
        match self {
            Page::Overview => "Обзор",
            Page::Models => "Модели",
            Page::Log => "Лог",
            Page::Journal => "Журнал",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::Overview => "◈",
            Page::Models => "◍",
            Page::Log => "≡",
            Page::Journal => "◷",
        }
    }
}

/// State of the current (or last) model download, shared with the worker thread.
#[derive(Clone, PartialEq, Debug)]
enum DlState {
    Idle,
    Downloading { name: String, received: u64, total: u64 },
    Extracting { name: String },
    Done { name: String },
    Canceled,
    Error(String),
}

impl DlState {
    fn is_busy(&self) -> bool {
        matches!(self, DlState::Downloading { .. } | DlState::Extracting { .. })
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
    page: Page,
    theme: Theme,
    applied: Theme,
    persist_error: Option<String>,
    export_error: Option<String>,
    pending_checked: bool,
}

pub fn run(
    cfg: Config,
    config_path: PathBuf,
    status: Arc<Mutex<Status>>,
    live: LiveLog,
    journal: Journal,
    _verbose: bool,
) -> Result<i32> {
    let theme = Theme::from_config(&cfg.gui.theme);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("rapcoding"),
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
        page: Page::Overview,
        theme,
        applied: theme,
        persist_error: None,
        export_error: None,
        pending_checked: false,
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
        let busy = sync::lock_recover(&self.dl_state).is_busy();
        if busy {
            // Repaint while downloading so the progress bar stays live.
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
                        Page::Models => self.models(ui, p),
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
                    ui.label(RichText::new("rapcoding").size(20.0).strong().color(p.text));
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(12.0)
                            .color(p.muted),
                    );
                    ui.add_space(12.0);
                    self.status_pill(ui, p);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (icon, label) = match self.theme {
                            Theme::Light => ("🌙", "Тёмная тема"),
                            Theme::Dark => ("☀", "Светлая тема"),
                        };
                        if ui.button(format!("{icon}  {label}")).clicked() {
                            self.theme = self.theme.toggle();
                            self.cfg.gui.theme = self.theme.as_str().to_string();
                            self.journal
                                .append("config", &format!("theme -> {}", self.theme.as_str()));
                            self.persist_error =
                                match config::save(&self.config_path, &self.cfg) {
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
        let (color, text) = match &st {
            Status::Idle => (p.success, "Готов"),
            Status::Listening(l) => (p.success, if *l == "en" { "Слушаю · EN" } else { "Слушаю · RU" }),
            Status::Transcribing(l) => (p.warn, if *l == "en" { "Распознаю · EN" } else { "Распознаю · RU" }),
            Status::Error(_) => (p.error, "Ошибка"),
        };
        egui::Frame::new()
            .fill(color.gamma_multiply(0.14))
            .corner_radius(CornerRadius::same(20))
            .inner_margin(Margin::symmetric(12, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").size(11.0).color(color));
                    ui.label(RichText::new(text).size(13.0).color(color).strong());
                });
            });
        if let Status::Error(msg) = &st {
            ui.label(RichText::new(msg).size(12.0).color(p.error));
        }
    }

    fn nav(&mut self, ctx: &egui::Context, p: Palette) {
        egui::SidePanel::left("nav")
            .exact_width(180.0)
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
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(page.icon()).size(14.0).color(color));
                                ui.label(
                                    RichText::new(page.title())
                                        .size(14.0)
                                        .color(color)
                                        .strong(),
                                );
                            });
                        })
                        .response;
                    let resp = resp.interact(egui::Sense::click());
                    if resp.clicked() {
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
                        );
                    }
                    ui.label(
                        RichText::new(match self.cfg.general.mode.as_str() {
                            "toggle" => "режим: toggle",
                            _ => "режим: hold",
                        })
                        .size(11.0)
                        .color(p.muted),
                    );
                });
            });
    }

    fn card<R>(
        ui: &mut egui::Ui,
        p: Palette,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
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
                RichText::new("Зажмите хоткей, говорите, отпустите — текст появится в активном окне.")
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
                        ui.label(RichText::new(pretty_hotkey(ru)).monospace().size(14.0).color(p.text));
                        ui.label(RichText::new(pretty_hotkey(en)).monospace().size(14.0).color(p.text));
                        ui.label("");
                        ui.end_row();
                    }
                });
        });

        ui.add_space(12.0);
        ui.columns(2, |cols| {
            Self::card(&mut cols[0], p, |ui| {
                Self::section_title(ui, p, "МОДЕЛИ");
                ui.add_space(6.0);
                model_line(ui, p, "RU", &self.cfg.asr.model);
                ui.add_space(6.0);
                model_line(ui, p, "EN", &self.cfg.asr.en_model);
            });
            Self::card(&mut cols[1], p, |ui| {
                Self::section_title(ui, p, "ВСТАВКА И РЕЖИМ");
                ui.add_space(6.0);
                kv(ui, p, "Метод", if self.cfg.inject.method == "type" { "печать" } else { "буфер обмена" });
                kv(ui, p, "Режим", if self.cfg.general.mode == "toggle" { "toggle" } else { "hold" });
                kv(ui, p, "Вставка включена", if self.cfg.inject.enabled { "да" } else { "нет" });
            });
        });
    }

    // -------------------------------------------------------------- models

    fn models(&mut self, ui: &mut egui::Ui, p: Palette) {
        let root = config::config_dir().join("models");
        let state = sync::lock_recover(&self.dl_state).clone();
        let busy = state.is_busy();

        Self::card(ui, p, |ui| {
            ui.label(
                RichText::new("Более тяжёлые модели распознают точнее")
                    .size(16.0)
                    .strong()
                    .color(p.text),
            );
            ui.label(
                RichText::new("Скачайте модель, затем нажмите «Сделать активной» и перезапустите программу.")
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
                ui.horizontal(|ui| {
                    ui.label(RichText::new(m.name).monospace().size(14.0).color(p.text));
                    ui.add_space(6.0);
                    badge(ui, if m.lang == "ru" { "RU" } else { "EN" }, p.accent);
                    ui.label(
                        RichText::new(download::human_size(m.size_bytes))
                            .size(12.0)
                            .color(p.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if active {
                            badge(ui, "активна", p.success);
                        } else if installed {
                            badge(ui, "установлена", p.muted);
                        }
                    });
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if is_current && busy {
                        if ui.button("Стоп").clicked() {
                            if let Some(flag) = &self.dl_cancel {
                                flag.store(true, Ordering::Relaxed);
                            }
                        }
                    } else if !installed {
                        if ui
                            .add_enabled(!busy, egui::Button::new("Скачать"))
                            .clicked()
                        {
                            self.start_download(m);
                        }
                    } else if !active {
                        if ui.button("Сделать активной").clicked() {
                            self.activate_model(m);
                        }
                    }
                });

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
                            ui.add(
                                egui::ProgressBar::new(frac)
                                    .desired_width(f32::INFINITY)
                                    .text(format!(
                                        "{} / {}",
                                        download::human_size(*received),
                                        download::human_size(*total)
                                    )),
                            );
                        }
                        DlState::Extracting { .. } => {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new());
                                ui.label(
                                    RichText::new("распаковка…").size(12.5).color(p.muted),
                                );
                            });
                        }
                        _ => {}
                    }
                }
            });
            ui.add_space(10.0);

            if is_current && matches!(state, DlState::Done { .. } | DlState::Canceled | DlState::Error(_)) {
                break;
            }
        }

        match &state {
            DlState::Done { name } => {
                Self::card(ui, p, |ui| {
                    ui.label(
                        RichText::new(format!("✓ {name}: скачивание завершено"))
                            .size(13.0)
                            .color(p.success)
                            .strong(),
                    );
                });
            }
            DlState::Canceled => {
                Self::card(ui, p, |ui| {
                    ui.label(RichText::new("Скачивание остановлено").size(13.0).color(p.muted));
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
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in self.live.snapshot() {
                                ui.label(RichText::new(line).monospace().size(11.5).color(p.text));
                            }
                        });
                });
        });
    }

    fn journal_page(&mut self, ui: &mut egui::Ui, p: Palette) {
        Self::card(ui, p, |ui| {
            ui.label(RichText::new("Журнал управления").size(16.0).strong().color(p.text));
            ui.add_space(8.0);
            let entries = self.journal.read();
            if entries.is_empty() {
                ui.label(RichText::new("Пока пусто.").size(13.0).color(p.muted));
            } else {
                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in entries {
                            ui.label(RichText::new(line).monospace().size(11.5).color(p.text));
                        }
                    });
            }
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
        let root = config::config_dir().join("models");
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
                let result = download::download(m.url, &zip_tmp, &mut |done, total| {
                    set(DlState::Downloading {
                        name: name.clone(),
                        received: done,
                        total,
                    });
                }, &cancel)
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
        let root = config::config_dir().join("models");
        let name = m.name.to_string();
        let url = m.url;
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
            let result = download::download(url, &zip_tmp, &mut |done, total| {
                set(DlState::Downloading {
                    name: name.clone(),
                    received: done,
                    total,
                });
            }, &cancel)
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

fn kv(ui: &mut egui::Ui, p: Palette, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).size(12.5).color(p.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(12.5).color(p.text));
        });
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
    fn pretty_hotkey_capitalizes() {
        assert_eq!(pretty_hotkey("ctrl+alt+space"), "Ctrl + Alt + Space");
        assert_eq!(pretty_hotkey("f9"), "F9");
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
        assert!(DlState::Downloading { name: "x".into(), received: 1, total: 2 }.is_busy());
        assert!(DlState::Extracting { name: "x".into() }.is_busy());
        assert!(!DlState::Idle.is_busy());
        assert!(!DlState::Canceled.is_busy());
    }
}
