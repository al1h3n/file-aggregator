//! Colours and egui styling.
//!
//! The colour scheme follows the background (dark text over light backgrounds and the
//! other way round) unless the user picks one. Text colours are Apple's label colours
//! (primary / secondary 60 % / tertiary 30 %), so hierarchy comes from contrast, not boxes.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, Visuals};

/// Every colour the interface draws with, for one scheme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    /// File names, titles.
    pub text: Color32,
    /// Summaries, hints.
    pub secondary: Color32,
    /// Folder paths, sizes, idle icons.
    pub tertiary: Color32,
    /// Hairlines between sections.
    pub separator: Color32,
    /// Hover highlight on rows and glass buttons.
    pub hover: Color32,
    /// Neutral fill for small chips (type badges, secondary buttons).
    pub fill: Color32,
    /// The one accent: primary action, focus, drop target.
    pub accent: Color32,
    pub danger: Color32,
    pub success: Color32,
    pub warning: Color32,
}

/// iOS 26 `systemBlue`; liquid-rust's tinted glass uses the same value.
pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0x88, 0xFF);

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

impl Palette {
    pub const LIGHT: Self = Self {
        dark: false,
        text: Color32::from_rgb(0x1D, 0x1D, 0x1F),
        secondary: rgba(0x3C, 0x3C, 0x43, 0x99),
        tertiary: rgba(0x3C, 0x3C, 0x43, 0x5C),
        separator: rgba(0x3C, 0x3C, 0x43, 0x2E),
        hover: rgba(0x00, 0x00, 0x00, 0x0F),
        fill: rgba(0x78, 0x78, 0x80, 0x24),
        accent: ACCENT,
        danger: Color32::from_rgb(0xE5, 0x2E, 0x33),
        success: Color32::from_rgb(0x28, 0xA7, 0x45),
        warning: Color32::from_rgb(0xE0, 0x7C, 0x00),
    };

    pub const DARK: Self = Self {
        dark: true,
        text: Color32::from_rgb(0xF5, 0xF5, 0xF7),
        secondary: rgba(0xEB, 0xEB, 0xF5, 0x99),
        tertiary: rgba(0xEB, 0xEB, 0xF5, 0x5C),
        separator: rgba(0xFF, 0xFF, 0xFF, 0x24),
        hover: rgba(0xFF, 0xFF, 0xFF, 0x14),
        fill: rgba(0x78, 0x78, 0x80, 0x3D),
        accent: Color32::from_rgb(0x0A, 0x91, 0xFF),
        danger: Color32::from_rgb(0xFF, 0x45, 0x48),
        success: Color32::from_rgb(0x30, 0xD1, 0x58),
        warning: Color32::from_rgb(0xFF, 0x9F, 0x0A),
    };

    pub fn new(dark: bool) -> Self {
        if dark { Self::DARK } else { Self::LIGHT }
    }
}

/// Type scale (points).
pub const BODY: f32 = 14.0;
pub const SMALL: f32 = 12.5;
pub const CAPTION: f32 = 11.5;

/// Applies the palette to egui's own widgets (text edit, tooltips, colour picker, scroll bars).
pub fn apply(ctx: &egui::Context, palette: &Palette) {
    ctx.set_theme(if palette.dark { egui::Theme::Dark } else { egui::Theme::Light });
    let mut visuals = if palette.dark { Visuals::dark() } else { Visuals::light() };

    // The glass is the surface: egui paints no panels of its own.
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.extreme_bg_color = Color32::TRANSPARENT;
    visuals.faint_bg_color = palette.hover;
    visuals.code_bg_color = palette.fill;
    visuals.weak_text_color = Some(palette.secondary);
    visuals.hyperlink_color = palette.accent;
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.text_cursor.stroke = Stroke::new(1.5, palette.accent);

    // Popups egui draws itself (tooltips, the colour picker) look like solid menus.
    let popup = if palette.dark { Color32::from_rgb(0x2C, 0x2C, 0x2E) } else { Color32::from_rgb(0xF9, 0xF9, 0xFB) };
    visuals.window_fill = popup;
    visuals.window_stroke = Stroke::new(1.0, palette.separator);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(12);
    let shadow = Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if palette.dark { 110 } else { 46 }) };
    visuals.popup_shadow = shadow;
    visuals.window_shadow = shadow;

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(8);
        widget.fg_stroke.color = palette.text;
    }
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.separator);
    visuals.widgets.inactive.weak_bg_fill = palette.fill;
    visuals.widgets.inactive.bg_fill = palette.fill;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.weak_bg_fill = palette.fill.gamma_multiply(1.6);
    visuals.widgets.hovered.bg_fill = palette.fill.gamma_multiply(1.6);
    visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    visuals.widgets.active.weak_bg_fill = palette.fill.gamma_multiply(2.2);
    visuals.widgets.active.bg_fill = palette.fill.gamma_multiply(2.2);
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals.widgets.noninteractive.fg_stroke.color = palette.text;
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(17.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(BODY, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(BODY, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(CAPTION, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size.y = 26.0;
        // Thin scroll bars that float over the content and widen on hover (like macOS).
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.interaction.tooltip_delay = 0.5;
    });
}
