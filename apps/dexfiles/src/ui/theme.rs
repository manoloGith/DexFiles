//! The look: light and dark palettes in the spirit of a modern desktop file manager (soft
//! sidebar, rounded selections, one accent colour), and the fonts.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(0x0a, 0x7a, 0xff);

#[derive(Clone, Copy)]
pub struct Palette {
    pub sidebar: Color32,
    pub content: Color32,
    pub toolbar: Color32,
    pub selection: Color32,
    pub selection_inactive: Color32,
    pub hover: Color32,
    pub separator: Color32,
    pub weak: Color32,
    pub stripe: Color32,
    pub drop: Color32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            sidebar: Color32::from_rgb(0x26, 0x27, 0x2b),
            content: Color32::from_rgb(0x1c, 0x1c, 0x1f),
            toolbar: Color32::from_rgb(0x2b, 0x2c, 0x30),
            selection: Color32::from_rgb(0x0a, 0x5c, 0xc8),
            selection_inactive: Color32::from_rgb(0x46, 0x48, 0x4e),
            hover: Color32::from_rgba_unmultiplied(255, 255, 255, 14),
            separator: Color32::from_rgb(0x3a, 0x3b, 0x40),
            weak: Color32::from_rgb(0x9a, 0x9c, 0xa3),
            stripe: Color32::from_rgba_unmultiplied(255, 255, 255, 6),
            drop: Color32::from_rgba_unmultiplied(0x0a, 0x7a, 0xff, 60),
        }
    } else {
        Palette {
            sidebar: Color32::from_rgb(0xf0, 0xf0, 0xf3),
            content: Color32::from_rgb(0xff, 0xff, 0xff),
            toolbar: Color32::from_rgb(0xf7, 0xf7, 0xf9),
            selection: Color32::from_rgb(0x0a, 0x7a, 0xff),
            selection_inactive: Color32::from_rgb(0xdc, 0xdc, 0xe1),
            hover: Color32::from_rgba_unmultiplied(0, 0, 0, 10),
            separator: Color32::from_rgb(0xe0, 0xe0, 0xe5),
            weak: Color32::from_rgb(0x6e, 0x6e, 0x75),
            stripe: Color32::from_rgba_unmultiplied(0, 0, 0, 7),
            drop: Color32::from_rgba_unmultiplied(0x0a, 0x7a, 0xff, 40),
        }
    }
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = palette(dark);
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.panel_fill = p.content;
    v.window_fill = if dark {
        Color32::from_rgb(0x2a, 0x2b, 0x2f)
    } else {
        Color32::WHITE
    };
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, p.separator);
    v.extreme_bg_color = if dark {
        Color32::from_rgb(0x16, 0x16, 0x18)
    } else {
        Color32::from_rgb(0xff, 0xff, 0xff)
    };
    v.faint_bg_color = p.stripe;
    v.text_edit_bg_color = Some(if dark {
        Color32::from_rgb(0x1f, 0x20, 0x23)
    } else {
        Color32::from_rgb(0xf2, 0xf2, 0xf5)
    });
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
        &mut v.widgets.noninteractive,
    ] {
        w.corner_radius = CornerRadius::same(6);
    }
    v.widgets.inactive.weak_bg_fill = if dark {
        Color32::from_rgb(0x3a, 0x3b, 0x40)
    } else {
        Color32::from_rgb(0xe9, 0xe9, 0xed)
    };
    v.widgets.inactive.bg_fill = v.widgets.inactive.weak_bg_fill;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = if dark {
        Color32::from_rgb(0x48, 0x49, 0x4f)
    } else {
        Color32::from_rgb(0xdf, 0xdf, 0xe4)
    };
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.menu_margin = egui::Margin::same(6);
        s.interaction.selectable_labels = false;
        s.text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
        s.text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(19.0));
    });
}

/// On Android: the system's Roboto as the main face, and its CJK and other fallbacks so any
/// file name shows (egui's own fonts cover Latin, Greek and Cyrillic only).
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut added = Vec::new();
    let candidates: &[(&str, &str, bool)] = &[
        ("roboto", "/system/fonts/Roboto-Regular.ttf", true),
        ("noto-cjk", "/system/fonts/NotoSansCJK-Regular.ttc", false),
        (
            "droid-fallback",
            "/system/fonts/DroidSansFallback.ttf",
            false,
        ),
        (
            "noto-arabic",
            "/system/fonts/NotoNaskhArabic-Regular.ttf",
            false,
        ),
        (
            "noto-hebrew",
            "/system/fonts/NotoSansHebrew-Regular.ttf",
            false,
        ),
        (
            "noto-devanagari",
            "/system/fonts/NotoSansDevanagari-Regular.otf",
            false,
        ),
        ("noto-thai", "/system/fonts/NotoSansThai-Regular.ttf", false),
        (
            "noto-cjk-linux",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            false,
        ),
    ];
    for (name, path, primary) in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        fonts
            .font_data
            .insert((*name).to_owned(), egui::FontData::from_owned(bytes).into());
        added.push((*name, *primary));
    }
    if added.is_empty() {
        return;
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family.clone()).or_default();
        for (name, primary) in &added {
            if *primary && family == egui::FontFamily::Proportional {
                list.insert(0, (*name).to_owned());
            } else {
                list.push((*name).to_owned());
            }
        }
    }
    ctx.set_fonts(fonts);
}
