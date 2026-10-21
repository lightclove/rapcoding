//! Native GUI (egui/eframe): settings/status window with light/dark themes.
//! Compiled only with `--features gui`. The recognition daemon runs on a
//! background thread and reports live status through the shared `Status`;
//! log lines stream through `LiveLog`, management events land in `Journal`,
//! and heavier models are downloaded/extracted in `download`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::config::{self, Config};
use crate::download;
use crate::error::Result;
use crate::journal::Journal;
use crate::log::{self, LiveLog};
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

/// State of the current (or last) model download, shared with the download thread.
#[derive(Clone)]
enum DlState {
    Idle,
    Downloading { name: String, received: u64, total: u64 },
    Extracting { name: String },
    Done { name: String },
    Error(String),
}

struct GuiApp {
    cfg: Config,
    config_path: PathBuf,
    status: Arc<Mutex<Status>>,
    live: LiveLog,
    journal: Journal,
    dl_state: Arc<Mutex<DlState>>,
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
            .with_inner_size([760.0, 620.0])
            .with_min_inner_size([520.0, 360.0])
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
                        self.journal
                            .append("config", &format!("theme -> {}", self.theme.as_str()));
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
                self.models_section(ui);
                ui.add_space(8.0);
                self.log_section(ui);
                ui.add_space(8.0);
                self.journal_section(ui);
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

    fn models_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Модели")
            .default_open(true)
            .show(ui, |ui| {
                let root = config::config_dir().join("models");
                let state = sync::lock_recover(&self.dl_state).clone();
                let busy = !matches!(
                    state,
                    DlState::Idle | DlState::Done { .. } | DlState::Error(_)
                );

                for m in download::MODELS {
                    let installed = download::model_dir(&root, m.name).exists();
                    let active = if m.lang == "ru" {
                        self.cfg.asr.model == m.config_value()
                    } else {
                        self.cfg.asr.en_model == m.config_value()
                    };
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{} ({} · {})",
                            m.name,
                            m.lang,
                            download::human_size(m.size_bytes)
                        ));
                        if installed {
                            if active {
                                ui.colored_label(
                                    egui::Color32::from_rgb(0x2e, 0xcc, 0x71),
                                    "✓ активна",
                                );
                            } else if ui
                                .button(format!("Сделать активной ({})", m.lang.to_uppercase()))
                                .clicked()
                            {
                                self.activate_model(m);
                            }
                        } else if ui
                            .add_enabled(!busy, egui::Button::new("Скачать"))
                            .clicked()
                        {
                            self.start_download(m);
                        }
                    });
                }
                ui.small("Активная модель применяется после перезапуска rapcoding.");

                ui.separator();
                match &state {
                    DlState::Idle => {}
                    DlState::Downloading {
                        name,
                        received,
                        total,
                    } => {
                        let frac = if *total > 0 {
                            (*received as f32 / *total as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        ui.label(format!("Скачиваю {name}…"));
                        ui.add(egui::ProgressBar::new(frac).show_percentage());
                        ui.label(format!(
                            "{} / {}",
                            download::human_size(*received),
                            download::human_size(*total)
                        ));
                    }
                    DlState::Extracting { name } => {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new());
                            ui.label(format!("Распаковываю {name}…"));
                        });
                    }
                    DlState::Done { name } => {
                        ui.colored_label(
                            egui::Color32::from_rgb(0x2e, 0xcc, 0x71),
                            format!("{name} установлена (перезапустите rapcoding)"),
                        );
                    }
                    DlState::Error(e) => {
                        ui.colored_label(egui::Color32::from_rgb(0xe0, 0x50, 0x50), format!("Ошибка: {e}"));
                    }
                }
            });
    }

    /// Models picked in the installer are queued in `pending-models.txt`.
    /// Download them on first launch (progress bar here, not in the installer).
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
        journal.append(
            "download",
            &format!("queued by installer: {} model(s)", wanted.len()),
        );
        std::thread::spawn(move || {
            let set = |s: DlState| {
                *sync::lock_recover(&state) = s;
            };
            for m in wanted {
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
                })
                .and_then(|()| {
                    set(DlState::Extracting { name: name.clone() });
                    download::extract_zip(&zip_tmp, &root)
                });
                let _ = std::fs::remove_file(&zip_tmp);
                match result {
                    Ok(()) => journal.append("download", &format!("ok {name}")),
                    Err(e) => {
                        journal.append("download", &format!("fail {name}: {e}"));
                        set(DlState::Error(e.to_string()));
                        return;
                    }
                }
            }
            set(DlState::Done {
                name: "выбранные модели".into(),
            });
        });
    }

    fn start_download(&self, m: &'static download::Model) {
        let state = self.dl_state.clone();
        let journal = self.journal.clone();
        let root = config::config_dir().join("models");
        let name = m.name.to_string();
        let url = m.url;
        let size = m.size_bytes;
        self.journal.append("download", &format!("start {name}"));
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
            let dl = download::download(url, &zip_tmp, &mut |done, total| {
                set(DlState::Downloading {
                    name: name.clone(),
                    received: done,
                    total,
                });
            });
            let result = match dl {
                Ok(()) => {
                    set(DlState::Extracting { name: name.clone() });
                    download::extract_zip(&zip_tmp, &root)
                }
                Err(e) => Err(e),
            };
            let _ = std::fs::remove_file(&zip_tmp);
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

    fn log_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Лог")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
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
                    if let Some(err) = &self.export_error {
                        ui.colored_label(egui::Color32::from_rgb(0xe0, 0x50, 0x50), err);
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in self.live.snapshot() {
                            ui.monospace(line);
                        }
                    });
            });
    }

    fn journal_section(&self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Журнал")
            .default_open(true)
            .show(ui, |ui| {
                let entries = self.journal.read();
                if entries.is_empty() {
                    ui.label("Пока пусто.");
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in entries {
                                ui.monospace(line);
                            }
                        });
                }
            });
    }
}
