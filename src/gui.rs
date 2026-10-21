//! Native GUI (egui/eframe): settings/status window with light/dark themes.
//! Compiled only with `--features gui`. The recognition daemon runs on a
//! background thread and reports live status through the shared `Status`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::config::{self, Config};
use crate::error::Result;
use crate::status::Status;
use crate::sync;

/// Mirrors `gui.theme` ("light" | "dark").
#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme {
    Light,
    Dark,
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

    fn visuals(self) -> egui::Visuals {
        let mut v = match self {
            Theme::Light => egui::Visuals::light(),
            Theme::Dark => egui::Visuals::dark(),
        };
        let accent = egui::Color32::from_rgb(0x4f, 0x8c, 0xff);
        v.selection.bg_fill = accent;
        v
    }
}

struct GuiApp {
    cfg: Config,
    config_path: PathBuf,
    status: Arc<Mutex<Status>>,
    theme: Theme,
    applied: Theme,
    persist_error: Option<String>,
}

pub fn run(
    cfg: Config,
    config_path: PathBuf,
    status: Arc<Mutex<Status>>,
    _verbose: bool,
) -> Result<i32> {
    let theme = Theme::from_config(&cfg.gui.theme);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 540.0])
            .with_min_inner_size([520.0, 360.0])
            .with_title("rapcoding"),
        ..Default::default()
    };
    let app = GuiApp {
        cfg,
        config_path,
        status,
        theme,
        applied: theme,
        persist_error: None,
    };
    eframe::run_native(
        "rapcoding",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(theme.visuals());
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| crate::error::Error::Gui(format!("{e}")))?;
    Ok(0)
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.theme != self.applied {
            ctx.set_visuals(self.theme.visuals());
            self.applied = self.theme;
        }

        egui::TopBottomPanel::top("topbar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("rapcoding");
                ui.separator();
                ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn = match self.theme {
                        Theme::Light => "☀  Светлая",
                        Theme::Dark => "🌙  Тёмная",
                    };
                    if ui.button(btn).clicked() {
                        self.theme = self.theme.toggle();
                        self.cfg.gui.theme = self.theme.as_str().to_string();
                        self.persist_error = match config::save(&self.config_path, &self.cfg) {
                            Ok(()) => None,
                            Err(e) => Some(e.to_string()),
                        };
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(err) = &self.persist_error {
                    ui.colored_label(egui::Color32::from_rgb(0xe0, 0x50, 0x50), err);
                    ui.add_space(4.0);
                }
                self.status_line(ui);
                ui.add_space(8.0);
                self.settings_section(ui);
                ui.add_space(8.0);
                egui::CollapsingHeader::new("Лог")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label("Живой лог появится в шаге B-09.");
                    });
                egui::CollapsingHeader::new("Модели")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label("Загрузчик тяжёлых моделей появится в шаге B-11.");
                    });
                egui::CollapsingHeader::new("Журнал")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label("Журнал управления появится в шаге B-10.");
                    });
            });
        });
    }
}

impl GuiApp {
    fn current_status(&self) -> Status {
        sync::lock_recover(&self.status).clone()
    }

    fn status_line(&self, ui: &mut egui::Ui) {
        let st = self.current_status();
        let color = match &st {
            Status::Idle => egui::Color32::from_rgb(0x90, 0x90, 0x90),
            Status::Listening(_) => egui::Color32::from_rgb(0x2e, 0xcc, 0x71),
            Status::Transcribing(_) => egui::Color32::from_rgb(0xf3, 0x9c, 0x12),
            Status::Error(_) => egui::Color32::from_rgb(0xe0, 0x50, 0x50),
        };
        ui.horizontal(|ui| {
            ui.label("Статус:");
            ui.colored_label(color, st.label());
        });
    }

    fn settings_section(&self, ui: &mut egui::Ui) {
        egui::Grid::new("settings_grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label("Режим");
                ui.label(if self.cfg.general.mode == "toggle" {
                    "toggle"
                } else {
                    "hold"
                });
                ui.end_row();
                ui.label("Хоткей RU");
                ui.monospace(&self.cfg.general.hotkey);
                ui.end_row();
                ui.label("Хоткей EN");
                ui.monospace(&self.cfg.general.en_hotkey);
                ui.end_row();
                ui.label("Вставка");
                ui.label(&self.cfg.inject.method);
                ui.end_row();
                ui.label("Модель RU");
                ui.monospace(&self.cfg.asr.model);
                ui.end_row();
                ui.label("Модель EN");
                ui.monospace(&self.cfg.asr.en_model);
                ui.end_row();
            });
    }
}
