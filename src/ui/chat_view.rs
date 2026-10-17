use chrono::DateTime;
use egui::{Align, Align2, Button, Color32, Key, Layout, RichText, Sense, TextEdit, Ui, Vec2};
use crate::models::{Channel, Message};
use crate::ui::theme::DiscordTheme;

#[allow(dead_code)]
pub struct ChatView {
    pub input_text: String,
    pub should_scroll_to_bottom: bool,
}

impl ChatView {
    pub fn new() -> Self {
        Self {
            input_text: String::new(),
            should_scroll_to_bottom: true,
        }
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        current_channel: Option<&Channel>,
        current_user_id: &str,
        messages: &[Message],
        mut on_send_message: impl FnMut(String),
    ) {
        egui::Frame::none()
            .fill(DiscordTheme::BG_CHAT)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());

                ui.vertical(|ui| {
                    // Header
                    Self::render_header(ui, current_channel, current_user_id);

                    // Messages list
                    let input_area_height = 64.0;
                    let list_height = ui.available_height() - input_area_height;

                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), list_height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            let scroll = egui::ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .stick_to_bottom(true);

                            scroll.show(ui, |ui| {
                                ui.add_space(8.0);
                                if messages.is_empty() {
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(40.0);
                                        ui.label(
                                            RichText::new("Нет сообщений в этом чате")
                                                .size(14.0)
                                                .color(DiscordTheme::TEXT_MUTED),
                                        );
                                    });
                                } else {
                                    for msg in messages {
                                        Self::render_message(ui, msg);
                                    }
                                }
                                ui.add_space(8.0);
                            });
                        },
                    );

                    // Input bar
                    self.render_input_bar(ui, current_channel, current_user_id, &mut on_send_message);
                });
            });
    }

    fn render_header(ui: &mut Ui, current_channel: Option<&Channel>, current_user_id: &str) {
        egui::Frame::none()
            .fill(DiscordTheme::BG_CHAT)
            .inner_margin(egui::Margin::symmetric(16.0, 14.0))
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(40, 42, 46)))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if let Some(ch) = current_channel {
                        let name = ch.display_name(current_user_id);
                        ui.label(
                            RichText::new("#")
                                .size(18.0)
                                .color(DiscordTheme::TEXT_MUTED),
                        );
                        ui.label(
                            RichText::new(name)
                                .size(16.0)
                                .strong()
                                .color(DiscordTheme::TEXT_BRIGHT),
                        );

                        if let Some(ref topic) = ch.topic {
                            if !topic.is_empty() {
                                ui.separator();
                                ui.label(
                                    RichText::new(topic)
                                        .size(12.0)
                                        .color(DiscordTheme::TEXT_MUTED),
                                );
                            }
                        }
                    } else {
                        ui.label(
                            RichText::new("Выберите канал для общения")
                                .size(15.0)
                                .color(DiscordTheme::TEXT_MUTED),
                        );
                    }
                });
            });
    }

    fn render_message(ui: &mut Ui, msg: &Message) {
        ui.horizontal_top(|ui| {
            ui.add_space(16.0);

            // User Avatar circle
            let (avatar_rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
            ui.painter().circle_filled(avatar_rect.center(), 18.0, DiscordTheme::BLURPLE);

            let initial = msg
                .author
                .display_name()
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();

            ui.painter().text(
                avatar_rect.center(),
                Align2::CENTER_CENTER,
                &initial,
                egui::FontId::proportional(15.0),
                Color32::WHITE,
            );

            ui.add_space(8.0);

            ui.vertical(|ui| {
                // Name & Timestamp
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(msg.author.display_name())
                            .size(14.0)
                            .strong()
                            .color(DiscordTheme::TEXT_BRIGHT),
                    );

                    let formatted_time = format_timestamp(&msg.timestamp);
                    ui.label(
                        RichText::new(formatted_time)
                            .size(11.0)
                            .color(DiscordTheme::TEXT_MUTED),
                    );
                });

                // Content
                if !msg.content.is_empty() {
                    ui.label(
                        RichText::new(&msg.content)
                            .size(14.0)
                            .color(DiscordTheme::TEXT_NORMAL),
                    );
                }

                // Attachments
                for att in &msg.attachments {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("📎")
                                .size(12.0)
                                .color(DiscordTheme::TEXT_MUTED),
                        );
                        let is_image = att.content_type.as_deref().map(|ct| ct.starts_with("image/")).unwrap_or(false);
                        let label_text = if is_image {
                            format!("🖼 {}", att.filename)
                        } else {
                            format!("📄 {} ({:.1} KB)", att.filename, att.size as f64 / 1024.0)
                        };
                        ui.hyperlink_to(
                            RichText::new(label_text).size(12.0).color(DiscordTheme::BLURPLE),
                            &att.url,
                        );
                    });
                }

                ui.add_space(6.0);
            });
        });
    }

    fn render_input_bar(
        &mut self,
        ui: &mut Ui,
        current_channel: Option<&Channel>,
        current_user_id: &str,
        on_send_message: &mut impl FnMut(String),
    ) {
        egui::Frame::none()
            .fill(DiscordTheme::BG_CHAT)
            .inner_margin(egui::Margin::symmetric(16.0, 10.0))
            .show(ui, |ui| {
                let hint = if let Some(ch) = current_channel {
                    format!("Написать в #{}...", ch.display_name(current_user_id))
                } else {
                    "Выберите канал...".to_string()
                };

                let enabled = current_channel.is_some();

                ui.horizontal(|ui| {
                    let text_edit = TextEdit::singleline(&mut self.input_text)
                        .hint_text(hint)
                        .desired_width(ui.available_width() - 80.0);

                    let edit_response = ui.add_enabled(enabled, text_edit);

                    let enter_pressed = edit_response.lost_focus()
                        && ui.input(|i| i.key_pressed(Key::Enter));

                    let send_btn = Button::new(
                        RichText::new("Отправить")
                            .size(13.0)
                            .color(Color32::WHITE),
                    )
                    .fill(DiscordTheme::BLURPLE);

                    let send_clicked = ui.add_enabled(enabled && !self.input_text.trim().is_empty(), send_btn).clicked();

                    if (enter_pressed || send_clicked) && !self.input_text.trim().is_empty() {
                        let to_send = self.input_text.trim().to_string();
                        self.input_text.clear();
                        on_send_message(to_send);
                        edit_response.request_focus();
                    }
                });
            });
    }
}

fn format_timestamp(ts: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        let local = dt.with_timezone(&chrono::Local);
        local.format("%H:%M").to_string()
    } else if ts.len() >= 16 {
        ts[11..16].to_string()
    } else {
        ts.to_string()
    }
}
