use chrono::{Local, TimeZone};
use eframe::egui::{
    self, Align, Color32, FontFamily, FontId, Layout, RichText, Sense, Stroke, Vec2,
};
use opsearch_companion::{
    CURRENT_VERSION, Command, CoreHandle, DEFAULT_SERVER, Event, Player, Roster, Status,
    UpdateChecker, UpdateInfo,
};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

// The opsearch.pro palette, flattened to opaque colours: a near-black page, glassy grey panels,
// a light primary button and a mint accent.
const BG: Color32 = Color32::from_rgb(11, 13, 17);
const SURFACE: Color32 = Color32::from_rgb(23, 26, 32);
const SURFACE_RAISED: Color32 = Color32::from_rgb(32, 35, 42);
const SURFACE_HOVER: Color32 = Color32::from_rgb(41, 44, 52);
const BORDER: Color32 = Color32::from_rgb(42, 45, 52);
const TEXT: Color32 = Color32::from_rgb(238, 241, 245);
const MUTED: Color32 = Color32::from_rgb(161, 169, 181);
const DIM: Color32 = Color32::from_rgb(147, 155, 168);
const PRIMARY: Color32 = Color32::from_rgb(244, 246, 248);
const ON_PRIMARY: Color32 = Color32::from_rgb(11, 13, 17);
const ACCENT: Color32 = Color32::from_rgb(201, 245, 215);
const LINK: Color32 = ACCENT;
const GREEN: Color32 = ACCENT;
const AMBER: Color32 = Color32::from_rgb(243, 201, 139);
const RED: Color32 = Color32::from_rgb(241, 166, 166);

pub struct CompanionApp {
    core: Option<CoreHandle>,
    status: Status,
    owner_id: Option<String>,
    roster: Option<Roster>,
    code: String,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
    help_open: bool,
    last_event: Option<String>,
    startup_error: Option<String>,
    updates: Option<UpdateChecker>,
    update: Option<UpdateInfo>,
}

impl CompanionApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        let (core, startup_error) = match CoreHandle::new() {
            Ok(core) => {
                let _ = core.send(Command::Start);
                (Some(core), None)
            }
            Err(error) => (None, Some(humanize_error(&error.to_string()))),
        };
        Self {
            core,
            status: Status::Idle,
            owner_id: None,
            roster: None,
            code: String::new(),
            busy: false,
            error: None,
            notice: None,
            help_open: false,
            last_event: None,
            startup_error,
            updates: Some(UpdateChecker::spawn()),
            update: None,
        }
    }

    fn receive_events(&mut self, ctx: &egui::Context) {
        let events = self.core.as_ref().map(CoreHandle::poll).unwrap_or_default();
        for event in events {
            match event {
                Event::Status(status) => {
                    if matches!(status, Status::Pairing) {
                        self.busy = true;
                    }
                    match &status {
                        Status::NeedsPairing { steam_id64 } | Status::Watching { steam_id64 } => {
                            if self
                                .owner_id
                                .as_deref()
                                .is_some_and(|old| old != steam_id64)
                            {
                                self.busy = false;
                                self.roster = None;
                                self.code.clear();
                                self.error = None;
                                self.notice = None;
                            }
                            self.owner_id = Some(steam_id64.clone());
                        }
                        Status::SignedOut => {
                            self.busy = false;
                            self.owner_id = None;
                            self.roster = None;
                            self.code.clear();
                            self.notice = None;
                        }
                        _ => {}
                    }
                    self.status = status;
                    self.last_event = Some("Статус обновлён".into());
                }
                Event::PairComplete { steam_id64 } => {
                    self.busy = false;
                    self.code.clear();
                    self.error = None;
                    self.notice =
                        Some("Устройство подключено. Включаем наблюдение за Steam…".into());
                    if self
                        .owner_id
                        .as_deref()
                        .is_some_and(|old| old != steam_id64)
                    {
                        self.roster = None;
                    }
                    self.owner_id = Some(steam_id64);
                    if let Err(error) = self.send(Command::Start) {
                        self.busy = false;
                        self.notice = None;
                        self.error = Some(humanize_error(&error));
                    }
                    self.last_event = Some("Устройство подключено".into());
                }
                Event::Roster(roster) => {
                    self.error = None;
                    self.notice = None;
                    self.owner_id = Some(roster.owner_steamid64.clone());
                    self.roster = Some(roster);
                    self.last_event = Some("Список игроков обновлён".into());
                }
                Event::Error(message) => {
                    self.busy = false;
                    self.error = Some(humanize_error(&message));
                    self.notice = None;
                    self.last_event = Some("Требуется внимание".into());
                }
            }
        }
        if let Some(update) = self.updates.as_ref().and_then(UpdateChecker::poll) {
            self.update = Some(update);
        }
        ctx.request_repaint_after(Duration::from_millis(160));
    }

    fn paired(&self) -> bool {
        matches!(self.status, Status::Watching { .. } | Status::Stopped) && self.owner_id.is_some()
    }

    fn connect(&mut self) {
        let code: String = self
            .code
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(12)
            .collect();
        if code.len() != 12 {
            self.error = Some("Проверьте код: в нём должно быть 12 букв или цифр.".into());
            return;
        }
        self.code = code.clone();
        self.error = None;
        self.notice = Some("Подключаем устройство… Обычно это занимает несколько секунд.".into());
        self.busy = true;
        if let Err(err) = self.send(Command::Pair {
            code,
            server: DEFAULT_SERVER.into(),
        }) {
            self.busy = false;
            self.notice = None;
            self.error = Some(humanize_error(&err.to_string()));
        }
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.core
            .as_ref()
            .ok_or_else(|| "Core worker unavailable".to_owned())?
            .send(command)
            .map_err(|error| error.to_string())
    }

    fn status_visual(&self) -> (Color32, &'static str, &'static str) {
        match &self.status {
            Status::Idle => (AMBER, "…", "Ожидание Steam"),
            Status::SignedOut => (AMBER, "!", "Steam не запущен"),
            Status::NeedsPairing { .. } => (LINK, "•", "Steam обнаружен"),
            Status::Pairing => (ACCENT, "…", "Подключение"),
            Status::Watching { .. } => (GREEN, "✓", "Подключено"),
            Status::Stopped => (AMBER, "Ⅱ", "Наблюдение остановлено"),
        }
    }

    fn render_header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            brand_mark(ui);
            ui.add_space(11.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("OPSearch")
                        .font(FontId::new(15.0, FontFamily::Name(Arc::from("unbounded"))))
                        .color(TEXT),
                );
                ui.label(
                    RichText::new(format!("Companion · v{CURRENT_VERSION}"))
                        .size(10.0)
                        .color(DIM),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (color, glyph, label) = self.status_visual();
                status_pill(ui, color, glyph, label);
            });
        });
        ui.add_space(17.0);
        ui.separator();
        ui.add_space(18.0);
    }

    fn render_update(&self, ui: &mut egui::Ui) {
        let Some(update) = &self.update else { return };
        egui::Frame::new()
            .fill(ACCENT.gamma_multiply(0.08))
            .stroke(Stroke::new(1.0, ACCENT.gamma_multiply(0.3)))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    status_dot(ui, ACCENT);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!("Доступна версия {}", update.version))
                                .size(13.0)
                                .strong()
                                .color(TEXT),
                        );
                        ui.label(
                            RichText::new(format!(
                                "У вас {CURRENT_VERSION}. Скачайте новую сборку и замените ею этот файл."
                            ))
                            .size(11.0)
                            .color(MUTED),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let button = egui::Button::new(
                            RichText::new("Скачать")
                                .size(12.0)
                                .strong()
                                .color(ON_PRIMARY),
                        )
                        .fill(PRIMARY)
                        .corner_radius(9.0)
                        .min_size(Vec2::new(96.0, 34.0));
                        if ui.add(button).clicked() {
                            ui.ctx()
                                .open_url(egui::OpenUrl::new_tab(update.url.clone()));
                        }
                    });
                });
            });
        ui.add_space(16.0);
    }

    fn render_hero(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("ПРИЛОЖЕНИЕ ДЛЯ WINDOWS")
                .size(10.0)
                .strong()
                .color(DIM),
        );
        ui.add_space(7.0);
        ui.label(
            RichText::new("CS2 Desktop Companion")
                .font(FontId::new(26.0, FontFamily::Name(Arc::from("unbounded"))))
                .color(TEXT),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("Подключите приложение, чтобы видеть недавно замеченных игроков CS2 в OPSearch. Данные Steam могут быть неполными.")
                .size(12.0)
                .color(MUTED),
        );
        ui.add_space(18.0);
    }

    fn render_account(&self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("АККАУНТ STEAM")
                            .size(10.0)
                            .strong()
                            .color(MUTED),
                    );
                    ui.add_space(5.0);
                    match &self.owner_id {
                        Some(id) => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("SteamID64  {id}"))
                                        .size(15.0)
                                        .monospace()
                                        .color(TEXT),
                                );
                                let copy = ui.add(
                                    egui::Button::new(
                                        RichText::new("Копировать").size(10.0).color(LINK),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0, BORDER))
                                    .corner_radius(egui::CornerRadius::same(7))
                                    .min_size(Vec2::new(86.0, 28.0)),
                                );
                                if copy.clicked() {
                                    ui.ctx().copy_text(id.clone());
                                }
                            });
                            ui.add_space(3.0);
                            ui.label(
                                RichText::new("Этот аккаунт используется для синхронизации")
                                    .size(12.0)
                                    .color(MUTED),
                            );
                        }
                        None => {
                            ui.label(
                                RichText::new("Ожидаем Steam")
                                    .size(16.0)
                                    .strong()
                                    .color(TEXT),
                            );
                            ui.add_space(3.0);
                            ui.label(
                                RichText::new("Запустите Steam и войдите в свой аккаунт")
                                    .size(12.0)
                                    .color(MUTED),
                            );
                        }
                    }
                });
            });
        });
    }

    fn render_pairing(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.label(
                RichText::new("ПОДКЛЮЧЕНИЕ УСТРОЙСТВА")
                    .size(10.0)
                    .strong()
                    .color(DIM),
            );
            ui.add_space(5.0);
            ui.label(
                RichText::new("Подключить компьютер")
                    .font(FontId::new(16.0, FontFamily::Name(Arc::from("unbounded"))))
                    .color(TEXT),
            );
            ui.label(
                RichText::new(
                    "Введите код с сайта OPSearch, чтобы связать этот компьютер с аккаунтом Steam.",
                )
                .size(12.0)
                .color(MUTED),
            );
            ui.add_space(16.0);
            ui.label(
                RichText::new("КОД ПОДКЛЮЧЕНИЯ")
                    .size(10.0)
                    .strong()
                    .color(MUTED),
            );
            ui.add_space(7.0);
            ui.horizontal(|ui| {
                let field = egui::TextEdit::singleline(&mut self.code)
                    .font(FontId::monospace(16.0))
                    .char_limit(12)
                    .desired_width(205.0)
                    .margin(Vec2::new(12.0, 11.0))
                    .hint_text("Вставьте код с сайта OPSearch");
                let response = ui.add_sized([236.0, 46.0], field);
                if response.changed() {
                    self.code = self
                        .code
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric())
                        .map(|c| c.to_ascii_uppercase())
                        .take(12)
                        .collect();
                    self.error = None;
                }
                ui.add_space(10.0);
                let button = egui::Button::new(
                    RichText::new(if self.busy {
                        "Подключаем…"
                    } else {
                        "Подключить"
                    })
                    .size(14.0)
                    .strong()
                    .color(ON_PRIMARY),
                )
                .fill(if self.busy { MUTED } else { PRIMARY })
                .corner_radius(10.0)
                .min_size(Vec2::new(145.0, 46.0));
                if ui
                    .add_enabled(
                        !self.busy && matches!(self.status, Status::NeedsPairing { .. }),
                        button,
                    )
                    .clicked()
                {
                    self.connect();
                }
            });
            ui.add_space(8.0);
            if matches!(self.status, Status::SignedOut | Status::Idle) {
                ui.horizontal_wrapped(|ui| {
                    status_dot(ui, AMBER);
                    ui.label(RichText::new("Сначала запустите Steam и войдите в аккаунт — после этого можно подключить устройство.")
                        .size(11.0).color(MUTED));
                });
            }
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Код действует, пока не будет использован или заменён новым.")
                        .size(11.0)
                        .color(DIM),
                );
                let link = ui.add(
                    egui::Label::new(
                        RichText::new("Получить код на сайте")
                            .size(11.0)
                            .color(LINK)
                            .underline(),
                    )
                    .sense(Sense::click()),
                );
                if link.clicked() {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab("https://opsearch.pro/companion"));
                }
            });
            self.render_feedback(ui);
        });
    }

    fn render_monitoring(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("СОСТОЯНИЕ УСТРОЙСТВА")
                            .size(10.0)
                            .strong()
                            .color(DIM),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Синхронизация")
                            .font(FontId::new(16.0, FontFamily::Name(Arc::from("unbounded"))))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new("Список недавних игроков CS2")
                            .size(12.0)
                            .color(MUTED),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let watching = matches!(self.status, Status::Watching { .. });
                    let label = if watching {
                        "Остановить"
                    } else {
                        "Запустить"
                    };
                    let fill = if watching { SURFACE_RAISED } else { PRIMARY };
                    let text_color = if watching { TEXT } else { ON_PRIMARY };
                    let button = egui::Button::new(
                        RichText::new(label).size(12.0).strong().color(text_color),
                    )
                    .fill(fill)
                    .stroke(Stroke::new(1.0, if watching { BORDER } else { PRIMARY }))
                    .corner_radius(9.0)
                    .min_size(Vec2::new(112.0, 40.0));
                    if ui.add_enabled(!self.busy, button).clicked() {
                        self.error = None;
                        let command = if watching {
                            Command::Stop
                        } else {
                            Command::Start
                        };
                        if let Err(err) = self.send(command) {
                            self.error = Some(humanize_error(&err));
                        } else if watching {
                            self.notice = Some(
                                "Наблюдение остановлено. Его можно включить снова в любой момент."
                                    .into(),
                            );
                        } else {
                            self.notice = Some("Ищем свежие данные Steam…".into());
                        }
                    }
                });
            });
            ui.add_space(13.0);
            ui.horizontal(|ui| {
                metric(
                    ui,
                    "ИГРОКОВ",
                    self.roster
                        .as_ref()
                        .map(|r| r.players.len().to_string())
                        .unwrap_or_else(|| "—".into()),
                );
                ui.add_space(8.0);
                metric(
                    ui,
                    "ПОСЛЕДНЕЕ ОБНОВЛЕНИЕ",
                    self.roster
                        .as_ref()
                        .map(|r| local_time(r.observed_at))
                        .unwrap_or_else(|| "Ожидаем данные".into()),
                );
                if let Some(roster) = &self.roster {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let stale = roster.observed_at.elapsed().unwrap_or_default()
                            > Duration::from_secs(120);
                        status_pill(
                            ui,
                            if stale { AMBER } else { GREEN },
                            if stale { "!" } else { "✓" },
                            if stale {
                                "Данные устарели"
                            } else {
                                "Актуально"
                            },
                        );
                    });
                }
            });
            self.render_feedback(ui);
        });

        ui.add_space(14.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("НЕДАВНИЕ ИГРОКИ")
                        .size(10.0)
                        .strong()
                        .color(MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.roster.is_some() {
                        ui.label(RichText::new("CS2 · App 730").size(10.0).color(DIM));
                    }
                });
            });
            ui.add_space(10.0);
            if let Some(roster) = &self.roster {
                if roster.players.is_empty() {
                    empty_state(
                        ui,
                        "Пока нет недавних игроков",
                        "Когда Steam обновит список, игроки появятся здесь.",
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(230.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (index, player) in roster.players.iter().enumerate() {
                                player_row(ui, player, index + 1);
                                if index + 1 < roster.players.len() {
                                    ui.add_space(2.0);
                                }
                            }
                        });
                }
            } else {
                empty_state(
                    ui,
                    "Ждём данные Steam",
                    "Оставьте Steam открытым — синхронизация начнётся автоматически.",
                );
            }
        });
    }

    fn render_feedback(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = self.error.as_ref() {
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                status_dot(ui, RED);
                ui.label(RichText::new(error).size(12.0).color(RED));
            });
        } else if let Some(notice) = self.notice.as_ref() {
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                status_dot(ui, LINK);
                ui.label(RichText::new(notice).size(12.0).color(MUTED));
            });
        }
    }

    fn render_help(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        egui::CollapsingHeader::new(RichText::new("Как это работает и что читает приложение").size(11.0).color(MUTED))
            .default_open(self.help_open)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("Приложение читает локальный список недавних игроков Steam и отправляет его в OPSearch для подключённого аккаунта.").size(11.0).color(MUTED));
                ui.add_space(4.0);
                if let Some(event) = &self.last_event {
                    ui.label(RichText::new(format!("Последнее событие: {event}")).size(10.0).color(DIM));
                }
                ui.label(RichText::new("Оно не обращается к памяти игры и не запускает её.").size(11.0).color(MUTED));
            });
    }
}

impl eframe::App for CompanionApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive_events(ui.ctx());
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.render_header(ui);
                        self.render_update(ui);
                        if let Some(error) = &self.startup_error {
                            card(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    status_dot(ui, RED);
                                    ui.label(RichText::new(error).size(12.0).color(RED));
                                });
                            });
                            ui.add_space(14.0);
                        }
                        self.render_hero(ui);
                        self.render_account(ui);
                        ui.add_space(14.0);
                        if self.paired() {
                            self.render_monitoring(ui);
                        } else {
                            self.render_pairing(ui);
                        }
                        self.render_help(ui);
                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(9.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(
                                    "Читает локальный список Steam. Без доступа к памяти игры.",
                                )
                                .size(10.0)
                                .color(DIM),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let link = ui.add(
                                    egui::Label::new(
                                        RichText::new("Исходный код ↗")
                                            .size(10.0)
                                            .color(MUTED)
                                            .underline(),
                                    )
                                    .sense(Sense::click()),
                                );
                                if link.clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(
                                        "https://github.com/MascotWorld/opsearch-companion",
                                    ));
                                }
                            });
                        });
                    });
            });
    }
}

fn apply_theme(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "onest".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Onest-Variable.ttf"
        ))),
    );
    fonts.font_data.insert(
        "unbounded".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Unbounded-Variable.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "onest".to_owned());
    fonts.families.insert(
        FontFamily::Name(Arc::from("unbounded")),
        vec!["unbounded".to_owned()],
    );
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = BG;
    style.visuals.text_edit_bg_color = Some(Color32::from_rgb(16, 18, 23));
    style.visuals.faint_bg_color = SURFACE_RAISED;
    style.visuals.widgets.noninteractive.bg_fill = SURFACE;
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    style.visuals.widgets.inactive.bg_fill = SURFACE_RAISED;
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE_RAISED;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
    style.visuals.widgets.hovered.weak_bg_fill = SURFACE_HOVER;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(64, 68, 76));
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(52, 56, 64);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.selection.bg_fill = Color32::from_rgb(64, 92, 76);
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(14.0, 8.0);
    ctx.set_style_of(egui::Theme::Dark, style);
    // egui follows the system theme by default, and only the dark style is customised: with a
    // light Windows theme the text field and separators came out white.
    ctx.set_theme(egui::ThemePreference::Dark);
}

fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(egui::CornerRadius::same(16))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add_contents(ui)
        })
        .inner
}

fn brand_mark(ui: &mut egui::Ui) {
    let (response, painter) = ui.allocate_painter(Vec2::new(40.0, 40.0), Sense::hover());
    let rect = response.rect;
    painter.circle_filled(rect.center(), 18.0, PRIMARY);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "OP",
        FontId::new(11.0, FontFamily::Name(Arc::from("unbounded"))),
        ON_PRIMARY,
    );
}

fn status_pill(ui: &mut egui::Ui, color: Color32, glyph: &str, label: &str) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.13))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.38)))
        .corner_radius(egui::CornerRadius::same(20))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(glyph).size(11.0).strong().color(color));
                ui.label(RichText::new(label).size(10.0).strong().color(color));
            });
        });
}

fn metric(ui: &mut egui::Ui, label: &str, value: String) {
    egui::Frame::new()
        .fill(SURFACE_RAISED)
        .corner_radius(egui::CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(11, 8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(label).size(9.0).strong().color(DIM));
                ui.label(RichText::new(value).size(12.0).color(TEXT));
            });
        });
}

fn player_row(ui: &mut egui::Ui, player: &Player, rank: usize) {
    ui.horizontal(|ui| {
        egui::Frame::new()
            .fill(SURFACE_RAISED)
            .corner_radius(egui::CornerRadius::same(7))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{rank:02}"))
                        .size(10.0)
                        .monospace()
                        .color(MUTED),
                );
            });
        ui.add_space(5.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(&player.steamid64)
                    .size(12.0)
                    .monospace()
                    .color(TEXT),
            );
            ui.label(RichText::new("SteamID64").size(9.0).color(DIM));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let played = unix_local_time(player.playtime);
            let link = ui.add(
                egui::Label::new(
                    RichText::new("Профиль ↗")
                        .size(10.0)
                        .color(LINK)
                        .underline(),
                )
                .sense(Sense::click()),
            );
            if link.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                    "https://opsearch.pro/profiles/{}",
                    player.steamid64
                )));
            }
            ui.add_space(10.0);
            ui.label(RichText::new(played).size(10.0).color(MUTED));
        });
    });
    ui.add_space(7.0);
    ui.separator();
    ui.add_space(5.0);
}

fn empty_state(ui: &mut egui::Ui, title: &str, detail: &str) {
    egui::Frame::new()
        .fill(SURFACE_RAISED)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(14, 16))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(title).size(13.0).strong().color(TEXT));
                ui.add_space(4.0);
                ui.label(RichText::new(detail).size(11.0).color(MUTED));
            });
        });
}

fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let (response, painter) = ui.allocate_painter(Vec2::splat(8.0), Sense::hover());
    painter.circle_filled(response.rect.center(), 3.0, color);
}

fn unix_local_time(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|dt| dt.format("%d.%m.%Y · %H:%M").to_string())
        .unwrap_or_else(|| "Дата недоступна".into())
}

fn local_time(time: SystemTime) -> String {
    let elapsed = time.elapsed().unwrap_or_default();
    if elapsed < Duration::from_secs(60) {
        "только что".into()
    } else if elapsed < Duration::from_secs(3600) {
        format!("{} мин. назад", elapsed.as_secs() / 60)
    } else {
        Local
            .timestamp_opt(
                time.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                0,
            )
            .single()
            .map(|dt| dt.format("%d.%m · %H:%M").to_string())
            .unwrap_or_else(|| "время неизвестно".into())
    }
}

fn humanize_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("pair")
        || lower.contains("code")
        || lower.contains("invalid")
        || lower.contains("http 401")
        || lower.contains("http 422")
    {
        "Не удалось подключить устройство. Проверьте код и попробуйте ещё раз.".into()
    } else if lower.contains("steam") || lower.contains("signed out") {
        "Steam не найден. Запустите Steam и войдите в нужный аккаунт.".into()
    } else if lower.contains("network")
        || lower.contains("connect")
        || lower.contains("timeout")
        || lower.contains("http")
    {
        "Не удалось связаться с OPSearch. Проверьте интернет и повторите попытку.".into()
    } else {
        "Что-то пошло не так. Проверьте Steam и подключение к интернету, затем попробуйте снова."
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_paired_screen_renders_without_network_or_steam() {
        let fake_owner = "synthetic-owner-id".to_owned();
        let mut app = CompanionApp {
            core: None,
            status: Status::Watching {
                steam_id64: fake_owner.clone(),
            },
            owner_id: Some(fake_owner.clone()),
            roster: Some(Roster {
                owner_steamid64: fake_owner,
                owner_playtime: 1_800_000_000,
                observed_at: SystemTime::now(),
                players: vec![Player {
                    steamid64: "synthetic-player-id".into(),
                    playtime: 1_800_000_000,
                    app_id: 730,
                }],
            }),
            code: String::new(),
            busy: false,
            error: None,
            notice: None,
            help_open: false,
            last_event: None,
            startup_error: None,
            updates: None,
            update: None,
        };
        let context = egui::Context::default();
        apply_theme(&context);
        let mut rendered = context.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.render_account(ui);
                app.render_monitoring(ui);
            });
        });

        assert!(app.paired());
        assert_eq!(app.roster.as_ref().unwrap().players.len(), 1);
        assert!(!rendered.shapes.is_empty());
        rendered.textures_delta.clear();
    }
}
