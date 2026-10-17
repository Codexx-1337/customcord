use egui::{Align, Align2, Color32, Layout, RichText, Sense, Ui, Vec2};
use crate::models::{Channel, ChannelType, Guild, User};
use crate::ui::theme::DiscordTheme;

pub struct ChannelSidebar;

impl ChannelSidebar {
    pub const WIDTH: f32 = 240.0;

    pub fn render(
        ui: &mut Ui,
        current_user: Option<&User>,
        selected_guild: Option<&Guild>,
        channels: &[Channel],
        private_channels: &[Channel],
        selected_channel_id: Option<&str>,
        mut on_select_channel: impl FnMut(&str),
        on_logout: impl FnOnce(),
    ) {
        egui::Frame::none()
            .fill(DiscordTheme::BG_SIDEBAR)
            .show(ui, |ui| {
                ui.set_width(Self::WIDTH);
                ui.set_height(ui.available_height());

                ui.vertical(|ui| {
                    // Top header: Server name or DM title
                    egui::Frame::none()
                        .fill(DiscordTheme::BG_SIDEBAR)
                        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                        .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(30, 31, 34)))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let title = match selected_guild {
                                Some(g) => &g.name,
                                None => "Личные сообщения",
                            };
                            ui.label(
                                RichText::new(title)
                                    .size(15.0)
                                    .strong()
                                    .color(DiscordTheme::TEXT_BRIGHT),
                            );
                        });

                    // Middle scroll area for channels or DMs
                    let scroll_height = ui.available_height() - 56.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), scroll_height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    ui.add_space(8.0);
                                    if selected_guild.is_some() {
                                        Self::render_guild_channels(
                                            ui,
                                            channels,
                                            selected_channel_id,
                                            &mut on_select_channel,
                                        );
                                    } else {
                                        Self::render_direct_messages(
                                            ui,
                                            current_user,
                                            private_channels,
                                            selected_channel_id,
                                            &mut on_select_channel,
                                        );
                                    }
                                });
                        },
                    );

                    // Bottom: User profile & status bar
                    Self::render_user_footer(ui, current_user, on_logout);
                });
            });
    }

    fn render_guild_channels(
        ui: &mut Ui,
        channels: &[Channel],
        selected_channel_id: Option<&str>,
        on_select_channel: &mut impl FnMut(&str),
    ) {
        // Group channels by category if possible, or filter text channels
        let text_channels: Vec<&Channel> = channels
            .iter()
            .filter(|c| c.channel_type == ChannelType::GuildText || c.channel_type == ChannelType::GuildAnnouncement)
            .collect();

        let voice_channels: Vec<&Channel> = channels
            .iter()
            .filter(|c| c.channel_type == ChannelType::GuildVoice || c.channel_type == ChannelType::GuildStageVoice)
            .collect();

        if !text_channels.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(
                    RichText::new("ТЕКСТОВЫЕ КАНАЛЫ")
                        .size(11.0)
                        .strong()
                        .color(DiscordTheme::TEXT_MUTED),
                );
            });
            ui.add_space(4.0);

            for ch in text_channels {
                let is_selected = selected_channel_id == Some(&ch.id);
                let channel_name = ch.name.as_deref().unwrap_or("channel");

                let item_response = ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width() - 8.0, 32.0),
                        Sense::click(),
                    );

                    let hovered = resp.hovered();
                    let bg = if is_selected {
                        DiscordTheme::BG_ITEM_ACTIVE
                    } else if hovered {
                        DiscordTheme::BG_ITEM_HOVER
                    } else {
                        Color32::TRANSPARENT
                    };

                    ui.painter().rect_filled(rect, 4.0, bg);

                    let text_color = if is_selected {
                        DiscordTheme::TEXT_BRIGHT
                    } else if hovered {
                        DiscordTheme::TEXT_NORMAL
                    } else {
                        DiscordTheme::TEXT_MUTED
                    };

                    // # icon
                    ui.painter().text(
                        egui::Pos2::new(rect.min.x + 10.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        "#",
                        egui::FontId::proportional(16.0),
                        DiscordTheme::TEXT_MUTED,
                    );

                    // Name
                    ui.painter().text(
                        egui::Pos2::new(rect.min.x + 28.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        channel_name,
                        egui::FontId::proportional(14.0),
                        text_color,
                    );

                    resp
                });

                if item_response.inner.clicked() {
                    on_select_channel(&ch.id);
                }
            }
        }

        if !voice_channels.is_empty() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(
                    RichText::new("ГОЛОСОВЫЕ КАНАЛЫ")
                        .size(11.0)
                        .strong()
                        .color(DiscordTheme::TEXT_MUTED),
                );
            });
            ui.add_space(4.0);

            for ch in voice_channels {
                let channel_name = ch.name.as_deref().unwrap_or("voice");
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width() - 8.0, 30.0),
                        Sense::hover(),
                    );
                    ui.painter().text(
                        egui::Pos2::new(rect.min.x + 10.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        "🔊",
                        egui::FontId::proportional(13.0),
                        DiscordTheme::TEXT_MUTED,
                    );
                    ui.painter().text(
                        egui::Pos2::new(rect.min.x + 28.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        channel_name,
                        egui::FontId::proportional(13.0),
                        DiscordTheme::TEXT_MUTED,
                    );
                });
            }
        }
    }

    fn render_direct_messages(
        ui: &mut Ui,
        current_user: Option<&User>,
        private_channels: &[Channel],
        selected_channel_id: Option<&str>,
        on_select_channel: &mut impl FnMut(&str),
    ) {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.label(
                RichText::new("ПРЯМЫЕ СООБЩЕНИЯ")
                    .size(11.0)
                    .strong()
                    .color(DiscordTheme::TEXT_MUTED),
            );
        });
        ui.add_space(4.0);

        let my_id = current_user.map(|u| u.id.as_str()).unwrap_or("");

        for ch in private_channels {
            let is_selected = selected_channel_id == Some(&ch.id);
            let display_name = ch.display_name(my_id);

            let resp = ui.horizontal(|ui| {
                ui.add_space(8.0);
                let (rect, resp) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width() - 8.0, 40.0),
                    Sense::click(),
                );

                let hovered = resp.hovered();
                let bg = if is_selected {
                    DiscordTheme::BG_ITEM_ACTIVE
                } else if hovered {
                    DiscordTheme::BG_ITEM_HOVER
                } else {
                    Color32::TRANSPARENT
                };

                ui.painter().rect_filled(rect, 6.0, bg);

                // Avatar placeholder (circle)
                let avatar_pos = egui::Pos2::new(rect.min.x + 20.0, rect.center().y);
                ui.painter().circle_filled(avatar_pos, 14.0, DiscordTheme::BLURPLE);

                let first_char = display_name.chars().next().unwrap_or('?').to_uppercase().to_string();
                ui.painter().text(
                    avatar_pos,
                    Align2::CENTER_CENTER,
                    &first_char,
                    egui::FontId::proportional(12.0),
                    Color32::WHITE,
                );

                // Online indicator dot
                let dot_pos = egui::Pos2::new(avatar_pos.x + 9.0, avatar_pos.y + 9.0);
                ui.painter().circle_filled(dot_pos, 4.0, DiscordTheme::GREEN);

                // Name
                let text_color = if is_selected {
                    DiscordTheme::TEXT_BRIGHT
                } else if hovered {
                    DiscordTheme::TEXT_NORMAL
                } else {
                    DiscordTheme::TEXT_MUTED
                };

                ui.painter().text(
                    egui::Pos2::new(rect.min.x + 42.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &display_name,
                    egui::FontId::proportional(13.0),
                    text_color,
                );

                resp
            });

            if resp.inner.clicked() {
                on_select_channel(&ch.id);
            }
        }
    }

    fn render_user_footer(ui: &mut Ui, current_user: Option<&User>, on_logout: impl FnOnce()) {
        egui::Frame::none()
            .fill(DiscordTheme::BG_SERVER_BAR)
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(48.0);

                ui.horizontal(|ui| {
                    if let Some(user) = current_user {
                        // User Avatar
                        let (avatar_rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                        ui.painter().circle_filled(avatar_rect.center(), 16.0, DiscordTheme::BLURPLE);
                        
                        let initial = user.username.chars().next().unwrap_or('?').to_uppercase().to_string();
                        ui.painter().text(
                            avatar_rect.center(),
                            Align2::CENTER_CENTER,
                            &initial,
                            egui::FontId::proportional(14.0),
                            Color32::WHITE,
                        );

                        // Online dot
                        let dot_pos = egui::Pos2::new(avatar_rect.max.x - 3.0, avatar_rect.max.y - 3.0);
                        ui.painter().circle_filled(dot_pos, 4.0, DiscordTheme::GREEN);

                        ui.add_space(4.0);

                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(user.display_name())
                                    .size(13.0)
                                    .strong()
                                    .color(DiscordTheme::TEXT_BRIGHT),
                            );
                            
                            // Live RAM monitor
                            let ram_mb = get_process_ram_mb();
                            ui.label(
                                RichText::new(format!("RAM: {:.1} MB", ram_mb))
                                    .size(10.0)
                                    .color(DiscordTheme::GREEN),
                            );
                        });
                    } else {
                        ui.label(RichText::new("Не подключен").size(12.0).color(DiscordTheme::TEXT_MUTED));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("🚪").on_hover_text("Выйти из аккаунта").clicked() {
                            on_logout();
                        }
                    });
                });
            });
    }
}

// Memory tracking via Windows API
#[cfg(target_os = "windows")]
fn get_process_ram_mb() -> f64 {
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    unsafe {
        let mut counters = std::mem::zeroed::<ProcessMemoryCounters>();
        counters.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
        let proc = GetCurrentProcess();
        if K32GetProcessMemoryInfo(proc, &mut counters, counters.cb) != 0 {
            counters.working_set_size as f64 / (1024.0 * 1024.0)
        } else {
            0.0
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn get_process_ram_mb() -> f64 {
    0.0
}
