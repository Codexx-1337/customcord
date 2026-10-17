use egui::{Color32, Stroke, Visuals};

pub struct DiscordTheme;

impl DiscordTheme {
    pub const BG_SERVER_BAR: Color32 = Color32::from_rgb(30, 31, 34);       // #1e1f22
    pub const BG_SIDEBAR: Color32 = Color32::from_rgb(43, 45, 49);          // #2b2d31
    pub const BG_CHAT: Color32 = Color32::from_rgb(49, 51, 56);             // #313338
    pub const BG_INPUT: Color32 = Color32::from_rgb(56, 58, 64);            // #383a40
    pub const BG_ITEM_HOVER: Color32 = Color32::from_rgb(53, 55, 60);       // #35373c
    pub const BG_ITEM_ACTIVE: Color32 = Color32::from_rgb(64, 66, 73);      // #404249
    
    pub const BLURPLE: Color32 = Color32::from_rgb(88, 101, 242);           // #5865f2
    pub const BLURPLE_HOVER: Color32 = Color32::from_rgb(71, 82, 196);
    pub const GREEN: Color32 = Color32::from_rgb(35, 165, 90);              // #23a55a
    pub const RED: Color32 = Color32::from_rgb(242, 63, 67);                // #f23f43
    
    pub const TEXT_NORMAL: Color32 = Color32::from_rgb(219, 222, 225);      // #dbdee1
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(148, 155, 164);       // #949ba4
    pub const TEXT_BRIGHT: Color32 = Color32::from_rgb(242, 243, 245);     // #f2f3f5

    pub fn apply(ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        let mut visuals = Visuals::dark();

        visuals.override_text_color = Some(Self::TEXT_NORMAL);
        visuals.panel_fill = Self::BG_CHAT;
        visuals.window_fill = Self::BG_SIDEBAR;
        visuals.extreme_bg_color = Self::BG_INPUT;

        // Widgets styling
        visuals.widgets.noninteractive.bg_fill = Self::BG_INPUT;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Self::TEXT_NORMAL);
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(6.0);

        visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Self::TEXT_MUTED);
        visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);

        visuals.widgets.hovered.bg_fill = Self::BG_ITEM_HOVER;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Self::TEXT_BRIGHT);
        visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);

        visuals.widgets.active.bg_fill = Self::BG_ITEM_ACTIVE;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Self::TEXT_BRIGHT);
        visuals.widgets.active.rounding = egui::Rounding::same(6.0);

        visuals.selection.bg_fill = Self::BLURPLE;
        visuals.selection.stroke = Stroke::new(1.0_f32, Self::TEXT_BRIGHT);

        style.visuals = visuals;
        ctx.set_style(style);
    }
}
