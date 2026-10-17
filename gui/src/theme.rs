//! Colors, spacing and the window icon.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle};

pub const BG: Color32 = Color32::from_rgb(0x11, 0x13, 0x18);
pub const PANEL: Color32 = Color32::from_rgb(0x16, 0x19, 0x20);
pub const CARD: Color32 = Color32::from_rgb(0x1d, 0x21, 0x2a);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x24, 0x29, 0x34);
pub const BORDER: Color32 = Color32::from_rgb(0x2b, 0x31, 0x3c);
pub const TEXT: Color32 = Color32::from_rgb(0xe4, 0xe7, 0xee);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0x8f, 0x99, 0xa8);
pub const ACCENT: Color32 = Color32::from_rgb(0x20, 0xb9, 0xa6);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x59, 0xd8, 0xc6);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x15, 0x39, 0x37);
pub const SUCCESS: Color32 = Color32::from_rgb(0x3c, 0xd2, 0x8a);
pub const WARNING: Color32 = Color32::from_rgb(0xf3, 0xb0, 0x45);
pub const ERROR: Color32 = Color32::from_rgb(0xff, 0x66, 0x66);
pub const LOG_BG: Color32 = Color32::from_rgb(0x0c, 0x0e, 0x12);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(21.0)),
            (TextStyle::Body, FontId::proportional(14.5)),
            (TextStyle::Button, FontId::proportional(14.5)),
            (TextStyle::Small, FontId::proportional(12.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(8.0, 7.0);
        s.button_padding = egui::vec2(12.0, 5.0);
        s.interact_size.y = 28.0;
        s.combo_width = 200.0;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.override_text_color = Some(TEXT);
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.faint_bg_color = CARD;
        v.extreme_bg_color = LOG_BG;
        v.hyperlink_color = ACCENT_HOVER;
        v.selection.bg_fill = ACCENT_SOFT;
        v.selection.stroke = Stroke::new(1.0, ACCENT_HOVER);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_stroke = Stroke::new(1.0, BORDER);

        let w = &mut v.widgets;
        for (state, fill) in [
            (&mut w.noninteractive, CARD),
            (&mut w.inactive, CARD_HOVER),
            (&mut w.hovered, Color32::from_rgb(0x2c, 0x33, 0x40)),
            (&mut w.active, ACCENT_SOFT),
            (&mut w.open, CARD_HOVER),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.corner_radius = CornerRadius::same(6);
        }
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
        w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.active.bg_stroke = Stroke::new(1.0, ACCENT_HOVER);
        w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    });
}

/// Window icon, the same artwork as the exe icon (see build.rs).
pub fn app_icon() -> egui::IconData {
    const N: u32 = 64;
    egui::IconData { rgba: crate::icon::rgba(N), width: N, height: N }
}
