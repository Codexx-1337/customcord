use egui::{Align2, Button, Color32, RichText, TextEdit, Ui, Vec2};
use crate::ui::theme::DiscordTheme;

pub struct LoginView {
    pub token: String,
    pub show_token: bool,
    pub save_token: bool,
    pub status_message: Option<String>,
    pub is_loading: bool,
}

impl LoginView {
    pub fn new(initial_token: String, save_token: bool) -> Self {
        Self {
            token: initial_token,
            show_token: false,
            save_token,
            status_message: None,
            is_loading: false,
        }
    }

    pub fn render(&mut self, ui: &mut Ui) -> Option<String> {
        let mut connect_requested = None;

        egui::Area::new(egui::Id::new("login_area"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                egui::Frame::none()
                    .fill(DiscordTheme::BG_SIDEBAR)
                    .rounding(10.0)
                    .inner_margin(28.0)
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(60, 63, 69)))
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);

                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("Lightcord")
                                    .size(28.0)
                                    .strong()
                                    .color(DiscordTheme::TEXT_BRIGHT),
                            );
                            ui.label(
                                RichText::new("Ультра-лёгкий нативный Discord клиент на Rust")
                                    .size(13.0)
                                    .color(DiscordTheme::TEXT_MUTED),
                            );
                        });

                        ui.add_space(20.0);

                        ui.label(
                            RichText::new("DISCORD USER TOKEN")
                                .size(11.0)
                                .strong()
                                .color(DiscordTheme::TEXT_MUTED),
                        );

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            let text_edit = TextEdit::singleline(&mut self.token)
                                .password(!self.show_token)
                                .hint_text("Вставьте ваш Discord токен...")
                                .desired_width(ui.available_width() - 40.0);
                            ui.add(text_edit);

                            let eye_icon = if self.show_token { "👁" } else { "🔒" };
                            if ui.button(eye_icon).clicked() {
                                self.show_token = !self.show_token;
                            }
                        });

                        ui.add_space(10.0);

                        ui.checkbox(&mut self.save_token, "Запомнить токен на этом ПК");

                        ui.add_space(15.0);

                        let button_text = if self.is_loading {
                            "Подключение..."
                        } else {
                            "Войти в Discord"
                        };

                        let btn = Button::new(
                            RichText::new(button_text)
                                .size(15.0)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(DiscordTheme::BLURPLE)
                        .min_size(Vec2::new(ui.available_width(), 40.0));

                        let btn_clicked = ui.add_enabled(!self.is_loading && !self.token.trim().is_empty(), btn).clicked();

                        if btn_clicked {
                            self.is_loading = true;
                            self.status_message = Some("Подключение к Discord...".to_string());
                            connect_requested = Some(self.token.trim().to_string());
                        }

                        if let Some(ref status) = self.status_message {
                            ui.add_space(10.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(status)
                                        .size(12.0)
                                        .color(DiscordTheme::TEXT_MUTED),
                                );
                            });
                        }

                        ui.add_space(20.0);
                        ui.separator();
                        ui.add_space(10.0);

                        // Instructions
                        ui.collapsing("ℹ️ Как узнать свой Discord токен?", |ui| {
                            ui.label(RichText::new("1. Откройте Discord в браузере или веб-клиент.").size(12.0));
                            ui.label(RichText::new("2. Нажмите F12 (Инструменты разработчика) -> вкладка Network (Сеть).").size(12.0));
                            ui.label(RichText::new("3. Отправьте сообщение или переключите канал.").size(12.0));
                            ui.label(RichText::new("4. Найдите любой запрос к /api/v9 или /api/v10.").size(12.0));
                            ui.label(RichText::new("5. В Headers -> Request Headers скопируйте значение 'authorization'.").size(12.0));
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("⚠️ Никогда и никому не передавайте свой токен!")
                                    .size(11.0)
                                    .color(DiscordTheme::RED),
                            );
                        });
                    });
            });

        connect_requested
    }
}
