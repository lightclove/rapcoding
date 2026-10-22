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

use eframe::egui::{self, text::LayoutJob, Color32, CornerRadius, FontId, Margin, RichText, Stroke, TextFormat};

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
    Help,
    Log,
    Journal,
}

impl Page {
    const ALL: [Page; 5] = [
        Page::Overview,
        Page::Models,
        Page::Help,
        Page::Log,
        Page::Journal,
    ];

    fn title(self) -> &'static str {
        match self {
            Page::Overview => "Обзор",
            Page::Models => "Модели",
            Page::Help => "Инструкция",
            Page::Log => "Лог",
            Page::Journal => "Журнал",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Page::Overview => "Статус, хоткеи и активные модели",
            Page::Models => "Скачать модель, сделать её активной",
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
    delete_confirm: Option<&'static str>,
    delete_error: Option<String>,
    page: Page,
    theme: Theme,
    applied: Theme,
    persist_error: Option<String>,
    export_error: Option<String>,
    pending_checked: bool,
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
    match std::env::var("RAPCODING_START_PAGE").unwrap_or_default().as_str() {
        "models" => Page::Models,
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
                if *l == "en" { "Слушаю · EN" } else { "Слушаю · RU" },
                "Идёт запись — говорите и отпустите хоткей",
            ),
            Status::Transcribing(l) => (
                p.warn,
                if *l == "en" { "Распознаю · EN" } else { "Распознаю · RU" },
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
            ui.label(RichText::new("Как пользоваться").size(18.0).strong().color(p.text));
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
        let root = config::config_dir().join("models");
        let state = sync::lock_recover(&self.dl_state).clone();
        let busy = state.is_busy();

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
                            RichText::new("Будет удалена папка модели — место на диске освободится.")
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
                            ui.add(
                                egui::ProgressBar::new(frac)
                                    .desired_width(bar_w)
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
                        RichText::new(format!("{name}: скачивание завершено"))
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

    /// Remove an installed model from disk (with confirmation done in the UI).
    fn delete_model(&mut self, m: &'static download::Model) {
        let root = config::config_dir().join("models");
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
        assert!(DlState::Downloading { name: "x".into(), received: 1, total: 2 }.is_busy());
        assert!(DlState::Extracting { name: "x".into() }.is_busy());
        assert!(!DlState::Idle.is_busy());
        assert!(!DlState::Canceled.is_busy());
    }
}
