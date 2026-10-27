use crate::tray::{self, Tray};
use chrono::{Local, TimeZone};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Layout, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, Vec2,
};
use opsearch_companion::{
    BACKGROUND_FLAG, CURRENT_VERSION, Command, CoreHandle, DEFAULT_SERVER, Event, Player,
    ProfileLoader, Roster, Settings, Status, SteamProfile, UpdateChecker, UpdateInfo, autostart,
    open_in_browser,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// The window is fixed: everything fits without scrolling.
pub const WINDOW_SIZE: [f32; 2] = [400.0, 510.0];
const TITLE_BAR: f32 = 46.0;
const PAD: f32 = 22.0;
const MAX_ROWS: usize = 10;
const ROW: f32 = 28.0;
const AVATAR: f32 = 22.0;

// The opsearch.pro palette, flattened to opaque colours.
const BG: Color32 = Color32::from_rgb(11, 13, 17);
const BG_TOP: Color32 = Color32::from_rgb(20, 22, 27);
const SURFACE: Color32 = Color32::from_rgb(23, 26, 32);
const SURFACE_HOVER: Color32 = Color32::from_rgb(33, 36, 43);
const BORDER: Color32 = Color32::from_rgb(38, 41, 48);
const TEXT: Color32 = Color32::from_rgb(238, 241, 245);
const MUTED: Color32 = Color32::from_rgb(143, 151, 163);
const FAINT: Color32 = Color32::from_rgb(98, 105, 116);
const PRIMARY: Color32 = Color32::from_rgb(244, 246, 248);
const ON_PRIMARY: Color32 = Color32::from_rgb(11, 13, 17);
const MINT: Color32 = Color32::from_rgb(201, 245, 215);
const AMBER: Color32 = Color32::from_rgb(243, 201, 139);
const RED: Color32 = Color32::from_rgb(241, 166, 166);

pub struct CompanionApp {
    logo: egui::TextureHandle,
    core: Option<CoreHandle>,
    status: Status,
    owner_id: Option<String>,
    roster: Option<Roster>,
    link_url: Option<String>,
    error: Option<String>,
    updates: Option<UpdateChecker>,
    update: Option<UpdateInfo>,
    profiles: Option<ProfileLoader>,
    /// Every SteamID asked for; `None` until Steam answers.
    cards: HashMap<String, Option<PlayerCard>>,
    window: Option<tray::Window>,
    tray: Option<Tray>,
    settings: Settings,
    autostart: bool,
    show_settings: bool,
    settings_error: Option<String>,
    /// Launched by autostart: get out of the way on the first frame.
    start_hidden: bool,
}

/// A player's Steam name and avatar, ready to draw.
struct PlayerCard {
    name: Option<String>,
    avatar: Option<egui::TextureHandle>,
}

/// What the middle of the window says, derived from the worker's status.
struct Headline {
    color: Color32,
    pulse: bool,
    title: &'static str,
    detail: String,
}

impl CompanionApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        let logo = cc.egui_ctx.load_texture(
            "opsearch-icon-v1",
            egui::ColorImage::from(window_icon().as_ref()),
            egui::TextureOptions::LINEAR,
        );
        let (core, error) = match CoreHandle::new() {
            Ok(core) => {
                let _ = core.send(Command::Start);
                (Some(core), None)
            }
            Err(error) => (None, Some(humanize_error(&error.to_string()))),
        };
        Self {
            logo,
            core,
            status: Status::Idle,
            owner_id: None,
            roster: None,
            link_url: None,
            error,
            updates: Some(UpdateChecker::spawn()),
            update: None,
            profiles: Some(ProfileLoader::spawn()),
            cards: HashMap::new(),
            window: None,
            tray: None,
            settings: Settings::load(),
            autostart: autostart::enabled(),
            show_settings: false,
            settings_error: None,
            start_hidden: std::env::args().any(|arg| arg == BACKGROUND_FLAG),
        }
        .with_window(tray::Window::new(cc))
    }

    fn with_window(mut self, window: tray::Window) -> Self {
        if self.settings.tray {
            self.tray = Tray::new(&window, &window_icon());
        }
        self.window = Some(window);
        self
    }

    /// Closing and minimising hide to the tray while it is on.
    fn put_away(&self, ctx: &egui::Context, close: bool) {
        match (&self.tray, &self.window) {
            (Some(_), Some(window)) => window.hide(),
            _ if close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            _ => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
        }
    }

    fn set_tray(&mut self, on: bool) {
        let previous = self.settings.clone();
        self.settings.tray = on;
        self.tray = match (&self.window, on) {
            (Some(window), true) => Tray::new(window, &window_icon()),
            _ => None,
        };
        if on && self.tray.is_none() {
            self.settings = previous;
            self.settings_error = Some("Не удалось добавить значок в трей".into());
        } else if self.settings.save().is_err() {
            self.settings_error = Some("Не удалось сохранить настройки".into());
        }
    }

    fn set_autostart(&mut self, on: bool) {
        match autostart::set(on) {
            Ok(()) => self.autostart = autostart::enabled(),
            Err(_) => self.settings_error = Some("Windows не дала изменить автозапуск".into()),
        }
    }

    /// Asks for the name and avatar of the owner and everyone in the lobby, once each.
    fn load_cards(&mut self, ctx: &egui::Context) {
        let Some(loader) = &self.profiles else { return };
        for profile in loader.poll() {
            let card = card_from(ctx, profile.clone());
            self.cards.insert(profile.steamid64, Some(card));
        }
        let wanted = self.owner_id.iter().chain(
            self.roster
                .iter()
                .flat_map(|roster| roster.players.iter().map(|p| &p.steamid64)),
        );
        for id in wanted {
            if !self.cards.contains_key(id) {
                self.cards.insert(id.clone(), None);
                loader.request(id);
            }
        }
    }

    fn card(&self, id: &str) -> Option<&PlayerCard> {
        self.cards.get(id).and_then(Option::as_ref)
    }

    fn receive_events(&mut self, ctx: &egui::Context) {
        let events = self.core.as_ref().map(CoreHandle::poll).unwrap_or_default();
        for event in events {
            match event {
                Event::Status(status) => {
                    match &status {
                        Status::NeedsPairing { steam_id64 } | Status::Watching { steam_id64 } => {
                            if self.owner_id.as_deref() != Some(steam_id64) {
                                self.roster = None;
                                self.error = None;
                            }
                            self.owner_id = Some(steam_id64.clone());
                        }
                        Status::SignedOut => {
                            self.owner_id = None;
                            self.roster = None;
                        }
                        _ => {}
                    }
                    // While the browser is open the worker keeps reporting the account; the
                    // waiting screen stays until the link resolves.
                    if self.link_url.is_none() || matches!(status, Status::Linking) {
                        self.status = status;
                    }
                }
                Event::OpenUrl(url) => {
                    open_in_browser(url.clone());
                    self.link_url = Some(url);
                    self.status = Status::Linking;
                }
                Event::PairComplete { steam_id64 } => {
                    self.link_url = None;
                    self.error = None;
                    self.owner_id = Some(steam_id64.clone());
                    self.status = Status::Watching { steam_id64 };
                    let _ = self.send(Command::Start);
                }
                Event::Roster(roster) => {
                    self.error = None;
                    self.owner_id = Some(roster.owner_steamid64.clone());
                    self.roster = Some(roster);
                }
                Event::Error(message) => {
                    if self.link_url.take().is_some() {
                        self.status = match &self.owner_id {
                            Some(id) => Status::NeedsPairing {
                                steam_id64: id.clone(),
                            },
                            None => Status::SignedOut,
                        };
                    }
                    self.error = Some(humanize_error(&message));
                }
            }
        }
        if let Some(update) = self.updates.as_ref().and_then(UpdateChecker::poll) {
            self.update = Some(update);
        }
        self.load_cards(ctx);
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.core
            .as_ref()
            .ok_or_else(|| "Core worker unavailable".to_owned())?
            .send(command)
            .map_err(|error| error.to_string())
    }

    fn headline(&self) -> Headline {
        let account = String::new();
        match &self.status {
            Status::Idle => Headline {
                color: FAINT,
                pulse: true,
                title: "Ищем Steam",
                detail: "Секунду…".into(),
            },
            Status::SignedOut => Headline {
                color: AMBER,
                pulse: false,
                title: "Steam не запущен",
                detail: "Откройте Steam и войдите в аккаунт".into(),
            },
            Status::NeedsPairing { .. } => Headline {
                color: TEXT,
                pulse: false,
                title: "Подключите компьютер",
                detail: account,
            },
            Status::Linking => Headline {
                color: MINT,
                pulse: true,
                title: "Подтвердите в браузере",
                detail: "Войдите тем же аккаунтом Steam".into(),
            },
            Status::Watching { .. } => {
                let stale = self.roster.as_ref().is_some_and(|r| {
                    r.observed_at.elapsed().unwrap_or_default() > Duration::from_secs(120)
                });
                Headline {
                    color: if stale { AMBER } else { MINT },
                    pulse: !stale,
                    title: "Подключено",
                    detail: account,
                }
            }
            Status::Stopped => Headline {
                color: FAINT,
                pulse: false,
                title: "На паузе",
                detail: account,
            },
        }
    }

    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let rect = Rect::from_min_size(
            ui.max_rect().min,
            Vec2::new(ui.max_rect().width(), TITLE_BAR),
        );
        let drag = ui.interact(
            rect,
            ui.scope_id().with("title-drag"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let painter = ui.painter();
        let logo_rect = Rect::from_center_size(
            Pos2::new(rect.left() + PAD + 11.0, rect.center().y),
            Vec2::splat(22.0),
        );
        painter.image(
            self.logo.id(),
            logo_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.text(
            Pos2::new(logo_rect.right() + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            "OPSearch",
            FontId::new(13.0, display_font()),
            TEXT,
        );

        let close = Rect::from_center_size(
            Pos2::new(rect.right() - 24.0, rect.center().y),
            Vec2::splat(30.0),
        );
        let minimize = close.translate(Vec2::new(-34.0, 0.0));
        let gear = minimize.translate(Vec2::new(-34.0, 0.0));
        if window_button(ui, close, "close", RED) {
            self.put_away(ui.ctx(), true);
        }
        if window_button(ui, minimize, "minimize", TEXT) {
            self.put_away(ui.ctx(), false);
        }
        if window_button(ui, gear, "settings", TEXT) {
            self.show_settings = !self.show_settings;
            self.settings_error = None;
        }
    }

    fn status_block(&mut self, ui: &mut egui::Ui) {
        let head = self.headline();
        ui.add_space(14.0);
        // One row: the orb, then what is happening and whose account it is.
        ui.horizontal(|ui| {
            status_orb(ui, head.color, head.pulse);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(
                    RichText::new(head.title)
                        .font(FontId::new(18.0, display_font()))
                        .color(TEXT),
                );
                if !head.detail.is_empty() {
                    ui.label(RichText::new(&head.detail).size(12.5).color(MUTED));
                }
                if matches!(
                    self.status,
                    Status::NeedsPairing { .. } | Status::Watching { .. } | Status::Stopped
                ) && let Some(id) = &self.owner_id
                {
                    owner_chip(ui, id, self.card(id));
                }
            });
        });
        ui.add_space(18.0);

        match &self.status {
            Status::NeedsPairing { .. } => {
                if primary_button(ui, "Подключить через Steam", true) {
                    self.error = None;
                    if let Err(error) = self.send(Command::Link {
                        server: DEFAULT_SERVER.into(),
                    }) {
                        self.error = Some(humanize_error(&error));
                    }
                }
            }
            Status::Linking => {
                ui.columns(2, |cols| {
                    if secondary_button(&mut cols[0], "Открыть снова")
                        && let Some(url) = &self.link_url
                    {
                        open_in_browser(url.clone());
                    }
                    if secondary_button(&mut cols[1], "Отмена") {
                        let _ = self.send(Command::CancelLink);
                        self.link_url = None;
                        self.status = match &self.owner_id {
                            Some(id) => Status::NeedsPairing {
                                steam_id64: id.clone(),
                            },
                            None => Status::SignedOut,
                        };
                    }
                });
            }
            _ => {}
        }

        if let Some(error) = &self.error {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(error).size(12.0).color(RED));
            });
        }
    }

    fn lobby(&self, ui: &mut egui::Ui, height: f32) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
        if !matches!(self.status, Status::Watching { .. } | Status::Stopped) {
            return;
        }
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        ui.horizontal(|ui| {
            ui.label(RichText::new("ЛОББИ").size(10.0).strong().color(FAINT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(roster) = &self.roster {
                    ui.label(
                        RichText::new(format!("сыграли · {}", clock(roster.owner_playtime)))
                            .size(11.0)
                            .color(FAINT),
                    );
                }
            });
        });
        ui.add_space(6.0);
        let Some(roster) = &self.roster else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Появится после матча")
                        .size(13.0)
                        .color(MUTED),
                );
                ui.label(
                    RichText::new("Оставьте Steam открытым")
                        .size(11.0)
                        .color(FAINT),
                );
            });
            return;
        };
        let others: Vec<&Player> = roster
            .players
            .iter()
            .filter(|p| p.steamid64 != roster.owner_steamid64)
            .collect();
        if others.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Пока никого").size(13.0).color(MUTED));
            });
            return;
        }
        // As many rows as the window holds; the rest are counted, never scrolled.
        ui.spacing_mut().item_spacing.y = 0.0;
        let fit = ((ui.available_height() / ROW).floor() as usize).min(MAX_ROWS);
        let shown = if others.len() > fit {
            fit.saturating_sub(1)
        } else {
            others.len()
        };
        for player in others.iter().take(shown) {
            player_row(&mut ui, player, self.card(&player.steamid64));
        }
        if others.len() > shown
            && text_link(
                &mut ui,
                &format!("и ещё {} — открыть на сайте", others.len() - shown),
                MUTED,
            )
        {
            open_in_browser(DEFAULT_SERVER.to_owned());
        }
    }

    fn settings_view(&mut self, ui: &mut egui::Ui, height: f32) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Настройки")
                    .font(FontId::new(20.0, display_font()))
                    .color(TEXT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if text_link(ui, "Готово", MINT) {
                    self.show_settings = false;
                }
            });
        });
        ui.add_space(18.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        let autostart_supported = autostart::supported();
        if toggle_row(
            &mut ui,
            "Запускать вместе с Windows",
            if autostart_supported {
                "Откроется в фоне при входе в систему"
            } else {
                "Доступно только в Windows"
            },
            self.autostart,
            autostart_supported,
        ) {
            self.settings_error = None;
            self.set_autostart(!self.autostart);
        }
        if toggle_row(
            &mut ui,
            "Сворачивать в трей",
            "Закрыть и свернуть прячут окно к часам",
            self.settings.tray,
            true,
        ) {
            self.settings_error = None;
            self.set_tray(!self.settings.tray);
        }
        if let Some(error) = &self.settings_error {
            ui.add_space(6.0);
            ui.label(RichText::new(error).size(12.0).color(RED));
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("v{CURRENT_VERSION}"))
                    .size(11.0)
                    .color(FAINT),
            );
            if let Some(update) = &self.update
                && text_link(ui, &format!("Обновить до {}", update.version), MINT)
            {
                open_in_browser(update.url.clone());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let toggle = match self.status {
                    Status::Watching { .. } => Some(("Пауза", MUTED, Command::Stop)),
                    Status::Stopped => Some(("Продолжить", MINT, Command::Start)),
                    _ => None,
                };
                if let Some((label, color, command)) = toggle
                    && text_link(ui, label, color)
                {
                    let _ = self.send(command);
                }
                if matches!(self.status, Status::Watching { .. } | Status::Stopped)
                    && let Some(id) = self.owner_id.clone()
                    && text_link(ui, "Профиль", MUTED)
                {
                    open_in_browser(format!("{DEFAULT_SERVER}/profiles/{id}"));
                }
                if text_link(ui, "Сайт", MUTED) {
                    open_in_browser(format!("{DEFAULT_SERVER}/companion"));
                }
            });
        });
    }
}

impl eframe::App for CompanionApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(BG).to_array()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive_events(&ctx);
        if std::mem::take(&mut self.start_hidden) {
            self.put_away(&ctx, false);
        }
        // Alt+F4 and the taskbar's close hide to the tray too, unless the tray menu said quit.
        if ctx.input(|i| i.viewport().close_requested())
            && self.tray.is_some()
            && !tray::QUITTING.load(std::sync::atomic::Ordering::SeqCst)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.put_away(&ctx, false);
        }
        self.draw(ui);
    }
}

impl CompanionApp {
    fn draw(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG))
            .show(ui, |ui| {
                let full = ui.max_rect();
                paint_backdrop(ui, full);
                self.title_bar(ui);

                let body = Rect::from_min_max(
                    Pos2::new(full.left() + PAD, full.top() + TITLE_BAR),
                    Pos2::new(full.right() - PAD, full.bottom() - 14.0),
                );
                let mut body_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(body)
                        .layout(Layout::top_down(Align::Min)),
                );
                if self.show_settings {
                    let height = body_ui.available_height() - 30.0;
                    self.settings_view(&mut body_ui, height.max(0.0));
                } else {
                    self.status_block(&mut body_ui);
                    body_ui.add_space(10.0);
                    let lobby_height = body_ui.available_height() - 30.0;
                    self.lobby(&mut body_ui, lobby_height.max(0.0));
                }
                self.footer(&mut body_ui);
            });
    }
}

fn display_font() -> FontFamily {
    FontFamily::Name(Arc::from("unbounded"))
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
    fonts
        .families
        .insert(display_font(), vec!["unbounded".to_owned()]);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = BG;
    style.visuals.selection.bg_fill = Color32::from_rgb(64, 92, 76);
    style.visuals.hyperlink_color = MINT;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    ctx.set_style_of(egui::Theme::Dark, style);
    // Only the dark style is customised; a light Windows theme must not switch to egui's light one.
    ctx.set_theme(egui::ThemePreference::Dark);
}

pub fn window_icon() -> Arc<egui::IconData> {
    Arc::new(
        eframe::icon_data::from_png_bytes(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/opsearch-icon-v1.png"
        )))
        .expect("embedded OPSearch icon decodes"),
    )
}

/// A soft light from the top. Windows draws the frameless window's own edge.
fn paint_backdrop(ui: &egui::Ui, rect: Rect) {
    let painter = ui.painter();
    let mut mesh = egui::Mesh::default();
    let mid = Pos2::new(rect.right(), rect.top() + 220.0);
    mesh.colored_vertex(rect.left_top(), BG_TOP);
    mesh.colored_vertex(rect.right_top(), BG_TOP);
    mesh.colored_vertex(Pos2::new(rect.left(), mid.y), BG);
    mesh.colored_vertex(mid, BG);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    painter.add(mesh);
}

fn window_button(ui: &mut egui::Ui, rect: Rect, kind: &str, hover_color: Color32) -> bool {
    let response = ui
        .interact(rect, ui.scope_id().with(kind), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(8), SURFACE_HOVER);
    }
    let color = if response.hovered() {
        hover_color
    } else {
        MUTED
    };
    let stroke = Stroke::new(1.4, color);
    let c = rect.center();
    if kind == "settings" {
        painter.circle_stroke(c, 3.0, stroke);
        for i in 0..6 {
            let angle = i as f32 * std::f32::consts::TAU / 6.0;
            let dir = Vec2::angled(angle);
            painter.line_segment([c + dir * 5.0, c + dir * 7.0], Stroke::new(2.0, color));
        }
        painter.circle_stroke(c, 5.5, Stroke::new(1.2, color));
    } else if kind == "close" {
        let d = 5.0;
        painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
        painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
    } else {
        painter.line_segment([c + Vec2::new(-5.0, 0.0), c + Vec2::new(5.0, 0.0)], stroke);
    }
    response.clicked()
}

fn status_orb(ui: &mut egui::Ui, color: Color32, pulse: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    if pulse {
        let t = (ui.input(|i| i.time) % 2.0) as f32 / 2.0;
        let radius = 12.0 + t * 14.0;
        painter.circle_filled(c, radius, color.gamma_multiply(0.35 * (1.0 - t)));
    }
    painter.circle_filled(c, 21.0, color.gamma_multiply(0.08));
    painter.circle_stroke(c, 21.0, Stroke::new(1.0, color.gamma_multiply(0.35)));
    painter.circle_filled(c, 7.0, color);
}

fn primary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
    let response = if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    };
    let fill = if !enabled {
        MUTED
    } else if response.is_pointer_button_down_on() {
        Color32::from_rgb(220, 224, 230)
    } else if response.hovered() {
        Color32::WHITE
    } else {
        PRIMARY
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(13), fill);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(14.0),
        ON_PRIMARY,
    );
    enabled && response.clicked()
}

fn secondary_button(ui: &mut egui::Ui, label: &str) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(12),
        if response.hovered() {
            SURFACE_HOVER
        } else {
            SURFACE
        },
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(12),
        Stroke::new(1.0, BORDER),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.0),
        TEXT,
    );
    response.clicked()
}

/// A setting: the whole row toggles it.
fn toggle_row(ui: &mut egui::Ui, title: &str, detail: &str, on: bool, enabled: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 60.0), Sense::click());
    let response = if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    };
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(12),
        if enabled && response.hovered() {
            SURFACE_HOVER
        } else {
            SURFACE
        },
    );
    painter.text(
        Pos2::new(rect.left() + 14.0, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.5),
        if enabled { TEXT } else { MUTED },
    );
    painter.text(
        Pos2::new(rect.left() + 14.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        detail,
        FontId::proportional(11.5),
        FAINT,
    );
    let track = Rect::from_center_size(
        Pos2::new(rect.right() - 14.0 - 18.0, rect.center().y),
        Vec2::new(36.0, 20.0),
    );
    let t = ui.ctx().animate_bool(response.id, on);
    let fill = if on { MINT } else { BORDER };
    painter.rect_filled(track, CornerRadius::same(10), fill);
    let knob_x = egui::lerp(track.left() + 10.0..=track.right() - 10.0, t);
    painter.circle_filled(
        Pos2::new(knob_x, track.center().y),
        7.0,
        if on { ON_PRIMARY } else { MUTED },
    );
    enabled && response.clicked()
}

fn text_link(ui: &mut egui::Ui, label: &str, color: Color32) -> bool {
    let response = ui
        .add(egui::Label::new(RichText::new(label).size(11.0).color(color)).sense(Sense::click()))
        .on_hover_cursor(CursorIcon::PointingHand);
    if response.hovered() {
        let r = response.rect;
        ui.painter().line_segment(
            [
                Pos2::new(r.left(), r.bottom()),
                Pos2::new(r.right(), r.bottom()),
            ],
            Stroke::new(1.0, color),
        );
    }
    response.clicked()
}

/// One lobby player: the whole row opens their OPSearch profile.
fn player_row(ui: &mut egui::Ui, player: &Player, card: Option<&PlayerCard>) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), SURFACE_HOVER);
    }
    let avatar = Rect::from_center_size(
        Pos2::new(rect.left() + 8.0 + AVATAR / 2.0, rect.center().y),
        Vec2::splat(AVATAR),
    );
    paint_avatar(ui, avatar, card);
    let time = clock(player.playtime);
    let time_width = 44.0;
    let name_rect = Rect::from_min_max(
        Pos2::new(avatar.right() + 10.0, rect.top()),
        Pos2::new(rect.right() - 10.0 - time_width, rect.bottom()),
    );
    let (text, font) = match card.and_then(|c| c.name.as_deref()) {
        Some(name) => (name.to_owned(), FontId::proportional(13.0)),
        None => (player.steamid64.clone(), FontId::monospace(12.0)),
    };
    let color = if response.hovered() {
        TEXT
    } else {
        Color32::from_rgb(205, 210, 218)
    };
    let galley = elide(ui, &text, name_rect.width(), font, color);
    ui.painter().galley(
        Pos2::new(name_rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    ui.painter().text(
        Pos2::new(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        time,
        FontId::proportional(11.0),
        FAINT,
    );
    if response.clicked() {
        open_in_browser(format!("{DEFAULT_SERVER}/profiles/{}", player.steamid64));
    }
}

/// The signed-in account under the status: avatar and name, opening the OPSearch profile.
fn owner_chip(ui: &mut egui::Ui, id: &str, card: Option<&PlayerCard>) {
    let name = card
        .and_then(|c| c.name.clone())
        .unwrap_or_else(|| short_id(id));
    let font = FontId::proportional(13.0);
    let galley = ui.painter().layout(name, font, MUTED, 220.0);
    let size = Vec2::new(6.0 + AVATAR + 8.0 + galley.size().x + 12.0, AVATAR + 8.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(255),
        if response.hovered() {
            SURFACE_HOVER
        } else {
            SURFACE
        },
    );
    let avatar = Rect::from_center_size(
        Pos2::new(rect.left() + 4.0 + AVATAR / 2.0, rect.center().y),
        Vec2::splat(AVATAR),
    );
    paint_avatar(ui, avatar, card);
    let color = if response.hovered() { TEXT } else { MUTED };
    ui.painter().galley(
        Pos2::new(
            avatar.right() + 8.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    if response.clicked() {
        open_in_browser(format!("{DEFAULT_SERVER}/profiles/{id}"));
    }
}

fn paint_avatar(ui: &egui::Ui, rect: Rect, card: Option<&PlayerCard>) {
    match card.and_then(|c| c.avatar.as_ref()) {
        Some(texture) => {
            egui::Image::new(egui::load::SizedTexture::new(texture.id(), rect.size()))
                .corner_radius(CornerRadius::same(6))
                .paint_at(ui, rect);
        }
        None => {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(40, 44, 52));
        }
    }
}

/// Cuts a single line of text to fit `width`, ending it with an ellipsis.
fn elide(ui: &egui::Ui, text: &str, width: f32, font: FontId, color: Color32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.painter().layout_job(job)
}

fn card_from(ctx: &egui::Context, profile: SteamProfile) -> PlayerCard {
    let side = SteamProfile::AVATAR_SIZE as usize;
    let avatar = profile.avatar.map(|pixels| {
        ctx.load_texture(
            format!("avatar-{}", profile.steamid64),
            egui::ColorImage::from_rgba_unmultiplied([side, side], &pixels),
            egui::TextureOptions::LINEAR,
        )
    });
    PlayerCard {
        name: profile.name,
        avatar,
    }
}

fn short_id(id: &str) -> String {
    if id.len() > 8 {
        format!("…{}", &id[id.len() - 6..])
    } else {
        id.to_owned()
    }
}

/// When Steam last saw the owner playing with someone: the time today, or the date before that.
fn clock(unix: i64) -> String {
    let Some(time) = Local.timestamp_opt(unix, 0).single() else {
        return String::new();
    };
    if time.date_naive() == Local::now().date_naive() {
        time.format("%H:%M").to_string()
    } else {
        time.format("%d.%m").to_string()
    }
}

fn humanize_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("not approved in time") {
        "Не дождались подтверждения. Попробуйте ещё раз.".into()
    } else if lower.contains("steam") || lower.contains("signed out") {
        "Steam не найден. Запустите Steam и войдите в аккаунт.".into()
    } else if lower.contains("network")
        || lower.contains("connect")
        || lower.contains("timeout")
        || lower.contains("http")
    {
        "Нет связи с OPSearch. Проверьте интернет.".into()
    } else {
        "Что-то пошло не так. Попробуйте ещё раз.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn app(status: Status, roster: Option<Roster>) -> CompanionApp {
        let context = egui::Context::default();
        let logo = context.load_texture(
            "test-logo",
            egui::ColorImage::filled([1, 1], Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        CompanionApp {
            logo,
            core: None,
            status,
            owner_id: Some("76561198000000001".into()),
            roster,
            link_url: None,
            error: None,
            updates: None,
            update: None,
            profiles: None,
            cards: HashMap::new(),
            window: None,
            tray: None,
            settings: Settings::default(),
            autostart: false,
            show_settings: false,
            settings_error: None,
            start_hidden: false,
        }
    }

    #[test]
    fn every_state_renders_inside_the_fixed_window() {
        let roster = Roster {
            owner_steamid64: "76561198000000001".into(),
            owner_playtime: 1_800_000_000,
            observed_at: SystemTime::now(),
            players: (0..14)
                .map(|i| Player {
                    steamid64: format!("765611980000001{i:02}"),
                    playtime: 1_800_000_000,
                    app_id: 730,
                })
                .collect(),
        };
        let owner = "76561198000000001".to_owned();
        for (status, roster) in [
            (Status::Idle, None),
            (Status::SignedOut, None),
            (
                Status::NeedsPairing {
                    steam_id64: owner.clone(),
                },
                None,
            ),
            (Status::Linking, None),
            (
                Status::Watching {
                    steam_id64: owner.clone(),
                },
                Some(roster.clone()),
            ),
            (Status::Stopped, None),
        ] {
            let mut app = app(status, roster);
            let context = egui::Context::default();
            apply_theme(&context);
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::from(WINDOW_SIZE))),
                ..Default::default()
            };
            let mut output = context.run_ui(input, |ui| app.draw(ui));
            assert!(!output.shapes.is_empty());
            output.textures_delta.clear();
        }
    }
}
