use chrono::{Local, TimeZone};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Layout, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, Vec2,
};
use opsearch_companion::{
    CURRENT_VERSION, Command, CoreHandle, DEFAULT_SERVER, Event, Player, Roster, Status,
    UpdateChecker, UpdateInfo,
};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// The window is fixed: everything fits without scrolling.
pub const WINDOW_SIZE: [f32; 2] = [400.0, 600.0];
const TITLE_BAR: f32 = 46.0;
const PAD: f32 = 22.0;
const MAX_ROWS: usize = 10;
const ROW: f32 = 30.0;

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
        }
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
                    ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
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
        let account = self
            .owner_id
            .as_deref()
            .map(|id| format!("Steam · {}", short_id(id)))
            .unwrap_or_default();
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

    fn title_bar(&self, ui: &mut egui::Ui) {
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
        if window_button(ui, close, "close", RED) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if window_button(ui, minimize, "minimize", TEXT) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    fn status_block(&mut self, ui: &mut egui::Ui) {
        let head = self.headline();
        ui.add_space(26.0);
        ui.vertical_centered(|ui| {
            status_orb(ui, head.color, head.pulse);
            ui.add_space(16.0);
            ui.label(
                RichText::new(head.title)
                    .font(FontId::new(20.0, display_font()))
                    .color(TEXT),
            );
            ui.add_space(4.0);
            ui.label(RichText::new(&head.detail).size(13.0).color(MUTED));
        });
        ui.add_space(20.0);

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
                        cols[0].ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
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
                        RichText::new(relative_time(roster.observed_at))
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
            player_row(&mut ui, player);
        }
        if others.len() > shown
            && text_link(
                &mut ui,
                &format!("и ещё {} — открыть на сайте", others.len() - shown),
                MUTED,
            )
        {
            ui.ctx()
                .open_url(egui::OpenUrl::new_tab(DEFAULT_SERVER.to_owned()));
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
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(update.url.clone()));
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
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                        "{DEFAULT_SERVER}/profiles/{id}"
                    )));
                }
                if text_link(ui, "Сайт", MUTED) {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                        "{DEFAULT_SERVER}/companion"
                    )));
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
        self.receive_events(ui.ctx());
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
                self.status_block(&mut body_ui);
                body_ui.add_space(18.0);
                let lobby_height = body_ui.available_height() - 30.0;
                self.lobby(&mut body_ui, lobby_height.max(0.0));
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

/// A soft light from the top and a hairline border, so the frameless window has an edge.
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
    painter.rect_stroke(
        rect.shrink(0.5),
        CornerRadius::ZERO,
        Stroke::new(1.0, BORDER),
        StrokeKind::Inside,
    );
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
    if kind == "close" {
        let d = 5.0;
        painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
        painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
    } else {
        painter.line_segment([c + Vec2::new(-5.0, 0.0), c + Vec2::new(5.0, 0.0)], stroke);
    }
    response.clicked()
}

fn status_orb(ui: &mut egui::Ui, color: Color32, pulse: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    if pulse {
        let t = (ui.input(|i| i.time) % 2.0) as f32 / 2.0;
        let radius = 14.0 + t * 18.0;
        painter.circle_filled(c, radius, color.gamma_multiply(0.35 * (1.0 - t)));
    }
    painter.circle_filled(c, 26.0, color.gamma_multiply(0.08));
    painter.circle_stroke(c, 26.0, Stroke::new(1.0, color.gamma_multiply(0.35)));
    painter.circle_filled(c, 8.0, color);
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
fn player_row(ui: &mut egui::Ui, player: &Player) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let response = response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Открыть профиль на OPSearch");
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(8), SURFACE_HOVER);
    }
    painter.text(
        Pos2::new(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        &player.steamid64,
        FontId::monospace(12.0),
        if response.hovered() { TEXT } else { MUTED },
    );
    painter.text(
        Pos2::new(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        if response.hovered() { "↗" } else { "" },
        FontId::proportional(12.0),
        MINT,
    );
    if response.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
            "{DEFAULT_SERVER}/profiles/{}",
            player.steamid64
        )));
    }
}

fn short_id(id: &str) -> String {
    if id.len() > 8 {
        format!("…{}", &id[id.len() - 6..])
    } else {
        id.to_owned()
    }
}

fn relative_time(time: SystemTime) -> String {
    let elapsed = time.elapsed().unwrap_or_default();
    if elapsed < Duration::from_secs(60) {
        "только что".into()
    } else if elapsed < Duration::from_secs(3600) {
        format!("{} мин назад", elapsed.as_secs() / 60)
    } else {
        Local
            .timestamp_opt(
                time.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                0,
            )
            .single()
            .map(|dt| dt.format("%d.%m %H:%M").to_string())
            .unwrap_or_default()
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
