//! Visual design system: palette, fonts, reusable widgets and vector icons.
//! Everything is painted with egui primitives (no images, no icon fonts).

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, Pos2, Rect, Response,
    RichText, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui, Vec2, pos2, vec2,
};
use std::sync::Arc;

// ---------------------------------------------------------------- palette --
pub const BG: Color32 = Color32::from_rgb(0x0E, 0x11, 0x16);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x12, 0x16, 0x1C);
pub const SURFACE: Color32 = Color32::from_rgb(0x15, 0x1A, 0x21);
pub const RAISED: Color32 = Color32::from_rgb(0x1E, 0x26, 0x33);
pub const SELECTED: Color32 = Color32::from_rgb(0x1F, 0x27, 0x42);
pub const BORDER: Color32 = Color32::from_rgb(0x22, 0x2A, 0x35);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x2E, 0x37, 0x44);
pub const BORDER_ACCENT: Color32 = Color32::from_rgb(0x34, 0x40, 0x6A);
pub const TRACK: Color32 = Color32::from_rgb(0x25, 0x2D, 0x39);
pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xEC, 0xF1);
pub const TEXT_SOFT: Color32 = Color32::from_rgb(0xC9, 0xD0, 0xDA);
pub const NAV_TEXT: Color32 = Color32::from_rgb(0xB4, 0xBD, 0xC9);
pub const MUTED: Color32 = Color32::from_rgb(0x9A, 0xA4, 0xB2);
pub const FAINT: Color32 = Color32::from_rgb(0x8F, 0x99, 0xA8);
pub const ACCENT: Color32 = Color32::from_rgb(0x7C, 0x9C, 0xFF);
pub const ON_ACCENT: Color32 = BG;

pub const CRIT: Color32 = Color32::from_rgb(0xF0, 0x71, 0x67);
pub const CRIT_TEXT: Color32 = Color32::from_rgb(0xF7, 0x97, 0x8E);
pub const CRIT_BG: Color32 = Color32::from_rgb(0x3A, 0x1F, 0x22);
pub const WARN: Color32 = Color32::from_rgb(0xF4, 0xB8, 0x60);
pub const WARN_TEXT: Color32 = Color32::from_rgb(0xF4, 0xC0, 0x77);
pub const WARN_BG: Color32 = Color32::from_rgb(0x3A, 0x2E, 0x1A);
pub const INFO_TEXT: Color32 = Color32::from_rgb(0xA9, 0xBD, 0xFF);
pub const INFO_BG: Color32 = SELECTED;
pub const GOOD: Color32 = Color32::from_rgb(0x6E, 0xDD, 0xB0);
pub const GOOD_BG: Color32 = Color32::from_rgb(0x17, 0x33, 0x2A);
pub const BANNER_BG: Color32 = Color32::from_rgb(0x21, 0x1C, 0x14);
pub const BANNER_BORDER: Color32 = Color32::from_rgb(0x4A, 0x3B, 0x22);
pub const BANNER_TEXT: Color32 = Color32::from_rgb(0xB9, 0xAE, 0x9A);

pub const SEMIBOLD: &str = "semibold";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Load the OS UI font (Segoe UI on Windows) when present; otherwise keep
/// egui's built-in fonts. Nothing is bundled, so the binary stays small.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates: &[(&str, &[&str])] = &[
        ("ui", &["C:/Windows/Fonts/segoeui.ttf", "/System/Library/Fonts/SFNS.ttf", "/usr/share/fonts/truetype/inter/Inter-Regular.ttf"]),
        ("ui-semibold", &["C:/Windows/Fonts/seguisb.ttf", "/usr/share/fonts/truetype/inter/Inter-SemiBold.ttf"]),
    ];
    let mut loaded = Vec::new();
    for (name, paths) in candidates {
        if let Some(bytes) = paths.iter().find_map(|p| std::fs::read(p).ok()) {
            fonts.font_data.insert((*name).into(), Arc::new(egui::FontData::from_owned(bytes)));
            loaded.push(*name);
        }
    }
    let mut prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if loaded.contains(&"ui") {
        prop.insert(0, "ui".into());
    }
    let mut semi = prop.clone();
    if loaded.contains(&"ui-semibold") {
        semi.insert(0, "ui-semibold".into());
    }
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), semi);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, regular(12.0)),
            (TextStyle::Body, regular(14.5)),
            (TextStyle::Button, regular(14.5)),
            (TextStyle::Heading, semibold(28.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        style.spacing.item_spacing = vec2(10.0, 8.0);
        style.spacing.button_padding = vec2(16.0, 8.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.scroll.bar_width = 8.0;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.extreme_bg_color = BG;
        v.faint_bg_color = SURFACE;
        v.code_bg_color = RAISED;
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = INFO_TEXT;
        v.selection.bg_fill = Color32::from_rgb(0x34, 0x40, 0x6A);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
        v.window_corner_radius = CornerRadius::same(14);
        v.window_stroke = Stroke::new(1.0, BORDER);
        let r = CornerRadius::same(10);
        for (w, fill, stroke) in [
            (&mut v.widgets.noninteractive, SURFACE, BORDER),
            (&mut v.widgets.inactive, Color32::TRANSPARENT, BORDER_STRONG),
            (&mut v.widgets.hovered, RAISED, Color32::from_rgb(0x3A, 0x45, 0x55)),
            (&mut v.widgets.active, SELECTED, ACCENT),
            (&mut v.widgets.open, RAISED, BORDER_STRONG),
        ] {
            w.corner_radius = r;
            w.weak_bg_fill = fill;
            w.bg_fill = if fill == Color32::TRANSPARENT { BG } else { fill };
            w.bg_stroke = Stroke::new(1.0, stroke);
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    });
}

// ---------------------------------------------------------------- frames --

pub fn card() -> Frame {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(16)
        .inner_margin(Margin::same(20))
}

pub fn card_accent() -> Frame {
    card().stroke(Stroke::new(1.0, BORDER_ACCENT))
}

pub fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(semibold(28.0)).color(TEXT));
}

pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(semibold(17.0)).color(TEXT));
}

pub fn eyebrow(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).font(regular(11.5)).color(FAINT).extra_letter_spacing(0.8));
}

pub fn muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).color(MUTED))
}

// --------------------------------------------------------------- buttons --

pub fn primary(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).font(semibold(14.5)).color(ON_ACCENT))
            .fill(ACCENT)
            .stroke(Stroke::NONE)
            .corner_radius(10)
            .min_size(vec2(0.0, 44.0)),
    )
}

pub fn primary_enabled(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    ui.add_enabled_ui(enabled, |ui| primary(ui, text)).inner
}

pub fn secondary(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(TEXT))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, BORDER_STRONG))
            .corner_radius(10)
            .min_size(vec2(0.0, 44.0)),
    )
}

pub fn secondary_enabled(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    ui.add_enabled_ui(enabled, |ui| secondary(ui, text)).inner
}

pub fn warn_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).font(semibold(14.5)).color(Color32::from_rgb(0x1A, 0x14, 0x08)))
            .fill(WARN)
            .stroke(Stroke::NONE)
            .corner_radius(10)
            .min_size(vec2(0.0, 44.0)),
    )
}

/// Icon-only button with a hover background.
pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8, RAISED);
    }
    let c = if resp.hovered() { TEXT } else { FAINT };
    paint_icon(ui.painter(), icon, rect.center(), 16.0, c);
    resp.on_hover_text(tooltip)
}

/// Pill-style choice used for questionnaire answers.
pub fn choice(ui: &mut Ui, selected: bool, text: &str) -> Response {
    let (fill, stroke, color) = if selected {
        (SELECTED, Stroke::new(1.0, ACCENT), Color32::WHITE)
    } else {
        (Color32::TRANSPARENT, Stroke::new(1.0, BORDER_STRONG), NAV_TEXT)
    };
    ui.add(
        egui::Button::new(RichText::new(text).color(color))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(10)
            .min_size(vec2(0.0, 40.0)),
    )
}

// ------------------------------------------------------------ indicators --

pub fn tag(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) {
    Frame::new().fill(bg).corner_radius(6).inner_margin(Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.label(RichText::new(text).font(semibold(12.0)).color(fg));
    });
}

pub fn chip(ui: &mut Ui, strong: &str, text: &str) {
    Frame::new().fill(RAISED).corner_radius(14).inner_margin(Margin::symmetric(10, 4)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if !strong.is_empty() {
                ui.label(RichText::new(strong).font(semibold(13.0)));
            }
            ui.label(RichText::new(text).font(regular(13.0)));
        });
    });
}

/// Horizontal progress bar with a rounded track.
pub fn bar(ui: &mut Ui, width: f32, height: f32, fraction: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let r = (height / 2.0) as u8;
    ui.painter().rect_filled(rect, r, TRACK);
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.0 {
        let mut fill = rect;
        fill.set_width((rect.width() * f).max(height));
        ui.painter().rect_filled(fill, r, color);
    }
    resp
}

/// Stacked allocation bar. Segments are (fraction 0..1, color).
pub fn stacked_bar(ui: &mut Ui, width: f32, height: f32, segments: &[(f32, Color32)]) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let r = (height / 2.0) as u8;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, r, TRACK);
    let mut x = rect.left();
    let gap = 2.0;
    let n = segments.iter().filter(|s| s.0 > 0.0).count();
    let mut i = 0;
    for (f, c) in segments {
        if *f <= 0.0 {
            continue;
        }
        i += 1;
        let w = rect.width() * f.clamp(0.0, 1.0);
        let seg_w = if i < n { (w - gap).max(1.0) } else { w };
        let seg = Rect::from_min_size(pos2(x, rect.top()), vec2(seg_w, height));
        let cr = CornerRadius {
            nw: if i == 1 { r } else { 0 },
            sw: if i == 1 { r } else { 0 },
            ne: if i == n && x + w >= rect.right() - 0.5 { r } else { 0 },
            se: if i == n && x + w >= rect.right() - 0.5 { r } else { 0 },
        };
        painter.rect_filled(seg, cr, *c);
        x += w;
    }
    resp
}

/// Circular score gauge (0..=100).
pub fn score_ring(ui: &mut Ui, size: f32, score: u8, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let radius = size / 2.0 - 6.0;
    let width = 9.0;
    p.circle_stroke(c, radius, Stroke::new(width, TRACK));
    let frac = score as f32 / 100.0;
    if frac > 0.0 {
        let steps = (64.0 * frac).ceil().max(2.0) as usize;
        let start = -std::f32::consts::FRAC_PI_2;
        let pts: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let a = start + std::f32::consts::TAU * frac * i as f32 / steps as f32;
                c + vec2(a.cos(), a.sin()) * radius
            })
            .collect();
        // Round caps.
        p.circle_filled(pts[0], width / 2.0, color);
        p.circle_filled(*pts.last().unwrap(), width / 2.0, color);
        p.add(Shape::line(pts, Stroke::new(width, color)));
    }
    p.text(c, Align2::CENTER_CENTER, score.to_string(), semibold(size * 0.3), TEXT);
}

pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

pub fn divider(ui: &mut Ui) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, BORDER);
}

// ----------------------------------------------------------------- icons --

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Grid,
    User,
    Chart,
    Chat,
    Gear,
    Sparkle,
    Close,
    Arrow,
    Check,
    Alert,
    Logo,
}

/// Paints a stroke icon centred at `c`, `s` px wide (24-unit design grid).
pub fn paint_icon(p: &egui::Painter, icon: Icon, c: Pos2, s: f32, color: Color32) {
    let k = s / 24.0;
    let at = |x: f32, y: f32| pos2(c.x + (x - 12.0) * k, c.y + (y - 12.0) * k);
    let st = Stroke::new(1.8 * k.max(0.75), color);
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), st));
    };
    let rrect = |x: f32, y: f32, w: f32, h: f32| {
        p.rect_stroke(Rect::from_min_max(at(x, y), at(x + w, y + h)), (1.5 * k) as u8, st, StrokeKind::Middle);
    };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| {
        let n = 20;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                at(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        p.add(Shape::line(pts, st));
    };
    match icon {
        Icon::Grid => {
            rrect(3.0, 3.0, 7.0, 7.0);
            rrect(14.0, 3.0, 7.0, 7.0);
            rrect(3.0, 14.0, 7.0, 7.0);
            rrect(14.0, 14.0, 7.0, 7.0);
        }
        Icon::User => {
            p.circle_stroke(at(12.0, 8.0), 4.0 * k, st);
            arc(12.0, 22.0, 8.0, 200.0, 340.0);
        }
        Icon::Chart => {
            line(&[(4.0, 20.0), (4.0, 10.0)]);
            line(&[(10.0, 20.0), (10.0, 4.0)]);
            line(&[(16.0, 20.0), (16.0, 13.0)]);
            line(&[(2.0, 20.0), (22.0, 20.0)]);
        }
        Icon::Chat => {
            arc(12.0, 11.5, 8.5, 135.0, 495.0 - 45.0);
            line(&[(6.0, 17.5), (4.0, 21.0), (8.5, 19.5)]);
        }
        Icon::Gear => {
            p.circle_stroke(at(12.0, 12.0), 3.0 * k, st);
            p.circle_stroke(at(12.0, 12.0), 7.5 * k, st);
            for i in 0..8 {
                let a = (i as f32 * 45.0f32).to_radians();
                line(&[(12.0 + 7.5 * a.cos(), 12.0 + 7.5 * a.sin()), (12.0 + 10.0 * a.cos(), 12.0 + 10.0 * a.sin())]);
            }
        }
        Icon::Sparkle => {
            line(&[(12.0, 3.0), (13.8, 7.7), (18.5, 9.5), (13.8, 11.3), (12.0, 16.0), (10.2, 11.3), (5.5, 9.5), (10.2, 7.7), (12.0, 3.0)]);
            line(&[(19.0, 15.0), (19.8, 17.2), (22.0, 18.0), (19.8, 18.8), (19.0, 21.0), (18.2, 18.8), (16.0, 18.0), (18.2, 17.2), (19.0, 15.0)]);
        }
        Icon::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Icon::Arrow => {
            line(&[(5.0, 12.0), (19.0, 12.0)]);
            line(&[(13.0, 6.0), (19.0, 12.0), (13.0, 18.0)]);
        }
        Icon::Check => line(&[(5.0, 12.0), (9.0, 16.0), (19.0, 6.0)]),
        Icon::Alert => {
            p.circle_stroke(at(12.0, 12.0), 9.0 * k, st);
            line(&[(12.0, 8.0), (12.0, 13.0)]);
            p.circle_filled(at(12.0, 16.5), 1.1 * k, color);
        }
        Icon::Logo => {
            // Pie with a lifted slice.
            arc(12.0, 12.0, 9.0, -90.0, 180.0);
            line(&[(3.0, 12.0), (12.0, 12.0), (12.0, 3.0)]);
            line(&[(15.0, 3.5), (15.0, 9.0), (20.5, 9.0)]);
            arc(15.0, 9.0, 5.5, -90.0, 0.0);
        }
    }
}

pub fn icon(ui: &mut Ui, icon: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_icon(ui.painter(), icon, rect.center(), size, color);
}

/// Sidebar navigation row (44 px tall).
pub fn nav_item(ui: &mut Ui, selected: bool, ic: Icon, label: &str) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 44.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 10, RAISED);
    } else if resp.hovered() {
        p.rect_filled(rect, 10, Color32::from_rgb(0x18, 0x1E, 0x27));
    }
    let (ic_color, txt_color, font) = if selected {
        (ACCENT, Color32::WHITE, semibold(14.5))
    } else {
        (NAV_TEXT, NAV_TEXT, regular(14.5))
    };
    paint_icon(p, ic, pos2(rect.left() + 22.0, rect.center().y), 18.0, ic_color);
    p.text(pos2(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, label, font, txt_color);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// One "label  [bar]  value" row, painted so all parts share a baseline.
pub fn bar_row(ui: &mut Ui, label: &str, fraction: f32, value: &str, color: Color32) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
    let p = ui.painter();
    let cy = rect.center().y;
    p.text(pos2(rect.left(), cy), Align2::LEFT_CENTER, label, semibold(14.0), TEXT);
    p.text(pos2(rect.right(), cy), Align2::RIGHT_CENTER, value, regular(14.0), TEXT);
    let track = Rect::from_min_max(pos2(rect.left() + 64.0, cy - 4.0), pos2(rect.right() - 60.0, cy + 4.0));
    p.rect_filled(track, 4, TRACK);
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.0 {
        let mut fill = track;
        fill.set_width((track.width() * f).max(8.0));
        p.rect_filled(fill, 4, color);
    }
}

/// Legend row: dot, label, right-aligned value, painted on one baseline.
pub fn legend_row(ui: &mut Ui, width: f32, color: Color32, label: &str, value: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let p = ui.painter();
    let cy = rect.center().y;
    p.circle_filled(pos2(rect.left() + 4.0, cy), 4.0, color);
    p.text(pos2(rect.left() + 16.0, cy), Align2::LEFT_CENTER, label, regular(13.5), TEXT);
    p.text(pos2(rect.right(), cy), Align2::RIGHT_CENTER, value, semibold(13.5), TEXT);
}

/// Thin wrapper so callers can lay out a fixed-width column.
pub fn column<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(width);
        add(ui)
    })
    .inner
}
