use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Sense, Ui, Vec2};
use crate::models::Guild;
use crate::ui::theme::DiscordTheme;

pub struct ServerBar;

impl ServerBar {
    pub const WIDTH: f32 = 72.0;

    pub fn render(
        ui: &mut Ui,
        guilds: &[Guild],
        selected_guild_id: &Option<String>,
        on_select_home: impl FnOnce(),
        mut on_select_guild: impl FnMut(&str),
    ) {
        egui::Frame::none()
            .fill(DiscordTheme::BG_SERVER_BAR)
            .inner_margin(egui::Margin::symmetric(12.0, 12.0))
            .show(ui, |ui| {
                ui.set_width(Self::WIDTH);
                ui.set_height(ui.available_height());

                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    // Home (DMs) button
                    let is_home_selected = selected_guild_id.is_none();
                    let (rect, response) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::click());

                    let hovered = response.hovered();
                    let bg_color = if is_home_selected {
                        DiscordTheme::BLURPLE
                    } else if hovered {
                        DiscordTheme::BLURPLE_HOVER
                    } else {
                        DiscordTheme::BG_SIDEBAR
                    };

                    let rounding = if is_home_selected || hovered { 16.0 } else { 24.0 };
                    ui.painter().rect_filled(rect, rounding, bg_color);

                    // Indicator bar on the left
                    if is_home_selected {
                        let indicator = Rect::from_min_size(
                            Pos2::new(rect.min.x - 12.0, rect.min.y + 4.0),
                            Vec2::new(4.0, 40.0),
                        );
                        ui.painter().rect_filled(indicator, 2.0, Color32::WHITE);
                    } else if hovered {
                        let indicator = Rect::from_min_size(
                            Pos2::new(rect.min.x - 12.0, rect.min.y + 14.0),
                            Vec2::new(4.0, 20.0),
                        );
                        ui.painter().rect_filled(indicator, 2.0, Color32::WHITE);
                    }

                    // Icon
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "💬",
                        egui::FontId::proportional(22.0),
                        Color32::WHITE,
                    );

                    if response.on_hover_text("Личные сообщения").clicked() {
                        on_select_home();
                    }

                    ui.add_space(8.0);
                    // Separator
                    let (sep_rect, _) = ui.allocate_exact_size(Vec2::new(32.0, 2.0), Sense::hover());
                    ui.painter().rect_filled(sep_rect, 1.0, Color32::from_rgb(53, 55, 60));
                    ui.add_space(8.0);

                    // Guilds list
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| {
                            for guild in guilds {
                                let is_selected = selected_guild_id
                                    .as_ref()
                                    .map(|id| id == &guild.id)
                                    .unwrap_or(false);

                                let (rect, response) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::click());
                                let hovered = response.hovered();

                                let bg_color = if is_selected {
                                    DiscordTheme::BLURPLE
                                } else if hovered {
                                    DiscordTheme::BLURPLE_HOVER
                                } else {
                                    DiscordTheme::BG_SIDEBAR
                                };

                                let rounding = if is_selected || hovered { 16.0 } else { 24.0 };
                                ui.painter().rect_filled(rect, rounding, bg_color);

                                // Pill indicator on left
                                if is_selected {
                                    let indicator = Rect::from_min_size(
                                        Pos2::new(rect.min.x - 12.0, rect.min.y + 4.0),
                                        Vec2::new(4.0, 40.0),
                                    );
                                    ui.painter().rect_filled(indicator, 2.0, Color32::WHITE);
                                } else if hovered {
                                    let indicator = Rect::from_min_size(
                                        Pos2::new(rect.min.x - 12.0, rect.min.y + 14.0),
                                        Vec2::new(4.0, 20.0),
                                    );
                                    ui.painter().rect_filled(indicator, 2.0, Color32::WHITE);
                                }

                                // Guild initials
                                let initials = guild.initials();
                                ui.painter().text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    &initials,
                                    egui::FontId::proportional(14.0),
                                    Color32::WHITE,
                                );

                                if response.on_hover_text(&guild.name).clicked() {
                                    on_select_guild(&guild.id);
                                }

                                ui.add_space(8.0);
                            }
                        });
                });
            });
    }
}
