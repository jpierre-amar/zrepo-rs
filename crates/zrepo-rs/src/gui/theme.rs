//! Couleurs, typographie et petits éléments dessinés à la main (icônes,
//! pastilles, badges, boutons).

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    TextStyle, Theme, Ui, Vec2, pos2, vec2,
};
use zrepo_core::entry::Kind;

/// Indigo de l'icône (packaging/zrepo-rs.svg).
pub const ACCENT: Color32 = Color32::from_rgb(0x6D, 0x5D, 0xFC);

#[derive(Clone, Copy)]
pub struct Pal {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub border: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub hit: Color32,
    pub hover: Color32,
    pub warn: Color32,
    pub error: Color32,
    pub ok: Color32,
}

impl Pal {
    pub fn of(ui: &Ui) -> Pal {
        Self::for_dark(ui.visuals().dark_mode)
    }

    pub fn for_dark(dark: bool) -> Pal {
        if dark {
            Pal {
                dark,
                bg: Color32::from_rgb(0x16, 0x16, 0x1C),
                surface: Color32::from_rgb(0x1F, 0x1F, 0x27),
                border: Color32::from_rgb(0x2C, 0x2C, 0x37),
                text: Color32::from_rgb(0xEE, 0xEE, 0xF4),
                weak: Color32::from_rgb(0xA0, 0xA0, 0xB2),
                faint: Color32::from_rgb(0x6E, 0x6E, 0x80),
                accent: ACCENT,
                accent_soft: Color32::from_rgba_unmultiplied(0x6D, 0x5D, 0xFC, 46),
                hit: Color32::from_rgb(0xA8, 0x9E, 0xFF),
                hover: Color32::from_rgba_unmultiplied(255, 255, 255, 9),
                warn: Color32::from_rgb(0xF5, 0xB0, 0x41),
                error: Color32::from_rgb(0xF2, 0x6D, 0x7D),
                ok: Color32::from_rgb(0x3D, 0xD6, 0x8C),
            }
        } else {
            Pal {
                dark,
                bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                surface: Color32::from_rgb(0xF5, 0xF5, 0xF9),
                border: Color32::from_rgb(0xE3, 0xE3, 0xEB),
                text: Color32::from_rgb(0x1A, 0x1A, 0x24),
                weak: Color32::from_rgb(0x5F, 0x5F, 0x72),
                faint: Color32::from_rgb(0x93, 0x93, 0xA5),
                accent: Color32::from_rgb(0x5B, 0x4B, 0xEA),
                accent_soft: Color32::from_rgba_unmultiplied(0x6D, 0x5D, 0xFC, 30),
                hit: Color32::from_rgb(0x4B, 0x3A, 0xE0),
                hover: Color32::from_rgba_unmultiplied(0, 0, 30, 9),
                warn: Color32::from_rgb(0xB4, 0x6A, 0x00),
                error: Color32::from_rgb(0xD0, 0x2F, 0x4A),
                ok: Color32::from_rgb(0x12, 0x9B, 0x5A),
            }
        }
    }

    pub fn kind(&self, kind: Kind) -> Color32 {
        match kind {
            Kind::Local => Color32::from_rgb(0x14, 0xA8, 0xBE),
            Kind::Remote => Color32::from_rgb(0xE8, 0x8F, 0x1E),
            Kind::Github if self.dark => Color32::from_rgb(0x3A, 0x3A, 0x48),
            Kind::Github => Color32::from_rgb(0x2B, 0x2B, 0x36),
        }
    }
}

/// Styles des deux thèmes ; egui choisit selon le système.
pub fn install(ctx: &egui::Context) {
    for theme in [Theme::Dark, Theme::Light] {
        let pal = Pal::for_dark(theme == Theme::Dark);
        ctx.style_mut_of(theme, |style| {
            use egui::FontFamily::{Monospace, Proportional};
            style.text_styles = [
                (TextStyle::Heading, FontId::new(22.0, Proportional)),
                (TextStyle::Body, FontId::new(14.5, Proportional)),
                (TextStyle::Button, FontId::new(14.0, Proportional)),
                (TextStyle::Small, FontId::new(12.0, Proportional)),
                (TextStyle::Monospace, FontId::new(13.0, Monospace)),
            ]
            .into();
            style.spacing.item_spacing = vec2(8.0, 8.0);
            style.spacing.button_padding = vec2(14.0, 7.0);
            style.spacing.interact_size.y = 32.0;
            style.spacing.text_edit_width = 320.0;

            let v = &mut style.visuals;
            v.panel_fill = pal.bg;
            v.window_fill = pal.bg;
            v.extreme_bg_color = pal.surface;
            v.faint_bg_color = pal.surface;
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.hovered.fg_stroke = Stroke::new(1.5, pal.text);
            v.widgets.active.fg_stroke = Stroke::new(2.0, pal.text);
            v.hyperlink_color = pal.hit;
            v.selection.bg_fill = pal.accent_soft;
            v.selection.stroke = Stroke::new(1.5, pal.accent);
            v.text_cursor.stroke = Stroke::new(2.0, pal.accent);
            v.window_corner_radius = CornerRadius::same(14);
            v.window_stroke = Stroke::new(1.0, pal.border);
            let r = CornerRadius::same(8);
            for w in [
                &mut v.widgets.noninteractive,
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.corner_radius = r;
            }
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.border);
            v.widgets.inactive.bg_fill = pal.surface;
            v.widgets.inactive.weak_bg_fill = pal.surface;
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, pal.border);
            v.widgets.hovered.bg_fill = pal.surface;
            v.widgets.hovered.weak_bg_fill = pal.surface;
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.accent);
            v.widgets.active.bg_stroke = Stroke::new(1.5, pal.accent);
        });
    }
}

/// Pastille arrondie de l'icône d'un type de projet, avec son dessin.
pub fn kind_icon(painter: &egui::Painter, rect: Rect, kind: Kind, pal: &Pal) {
    painter.rect_filled(
        rect,
        CornerRadius::same((rect.width() * 0.28) as u8),
        pal.kind(kind),
    );
    let s = rect.width() / 30.0;
    let c = rect.center();
    let fg = Color32::WHITE;
    let st = Stroke::new(1.6 * s, fg);
    match kind {
        Kind::Local => {
            // Dossier : onglet + corps.
            let body = Rect::from_center_size(c + vec2(0.0, 1.5 * s), vec2(16.0 * s, 11.0 * s));
            let tab = Rect::from_min_size(body.min - vec2(0.0, 2.5 * s), vec2(7.0 * s, 4.0 * s));
            painter.rect_filled(tab, CornerRadius::same((1.5 * s) as u8), fg);
            painter.rect_filled(body, CornerRadius::same((2.0 * s) as u8), fg);
        }
        Kind::Remote => {
            // Serveur : deux tiroirs avec leur voyant.
            for dy in [-3.6, 3.6] {
                let r = Rect::from_center_size(c + vec2(0.0, dy * s), vec2(16.0 * s, 6.0 * s));
                painter.rect_stroke(
                    r,
                    CornerRadius::same((1.5 * s) as u8),
                    st,
                    StrokeKind::Middle,
                );
                painter.circle_filled(r.right_center() - vec2(3.2 * s, 0.0), 1.1 * s, fg);
            }
        }
        Kind::Github => {
            // Branche git : deux commits et une bifurcation.
            let a = c + vec2(-3.5 * s, -5.5 * s);
            let b = c + vec2(-3.5 * s, 5.5 * s);
            let d = c + vec2(4.0 * s, -2.0 * s);
            painter.line_segment([a, b], st);
            let mid = c + vec2(-3.5 * s, 2.0 * s);
            painter.add(Shape::CubicBezier(
                egui::epaint::CubicBezierShape::from_points_stroke(
                    [d, d + vec2(0.0, 3.0 * s), mid + vec2(3.0 * s, -s), mid],
                    false,
                    Color32::TRANSPARENT,
                    st,
                ),
            ));
            for p in [a, b, d] {
                painter.circle_filled(p, 2.2 * s, fg);
            }
        }
    }
}

/// Loupe du champ de recherche.
pub fn search_icon(painter: &egui::Painter, center: Pos2, color: Color32) {
    let st = Stroke::new(1.8, color);
    painter.circle_stroke(center - vec2(1.5, 1.5), 6.0, st);
    painter.line_segment([center + vec2(2.8, 2.8), center + vec2(7.0, 7.0)], st);
}

/// Pastille de filtre (texte + compteur), sélectionnable.
pub fn pill(ui: &mut Ui, label: &str, count: Option<usize>, selected: bool) -> Response {
    let pal = Pal::of(ui);
    let font = FontId::proportional(13.5);
    let small = FontId::proportional(12.0);
    let (fg, count_fg) = if selected {
        (Color32::WHITE, Color32::WHITE)
    } else {
        (pal.text, pal.weak)
    };
    let g = ui.painter().layout_no_wrap(label.into(), font, fg);
    let cg = count.map(|n| ui.painter().layout_no_wrap(n.to_string(), small, count_fg));
    let cw = cg.as_ref().map_or(0.0, |g| g.size().x + 14.0);
    let size = vec2(g.size().x + 24.0 + cw, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let (fill, stroke) = if selected {
        (pal.accent, Stroke::NONE)
    } else {
        let f = lerp(pal.surface, pal.border, hover * 0.6);
        (f, Stroke::new(1.0, pal.border))
    };
    let cr = CornerRadius::same(14);
    ui.painter()
        .rect(rect, cr, fill, stroke, StrokeKind::Inside);
    let y = rect.center().y;
    ui.painter()
        .galley(pos2(rect.left() + 12.0, y - g.size().y / 2.0), g, fg);
    if let Some(cg) = cg {
        let cpos = pos2(rect.right() - 12.0 - cg.size().x, y - cg.size().y / 2.0);
        let bubble = Rect::from_min_size(cpos - vec2(5.0, 1.0), cg.size() + vec2(10.0, 2.0));
        let bfill = if selected {
            Color32::from_white_alpha(46)
        } else {
            pal.bg
        };
        ui.painter()
            .rect_filled(bubble, CornerRadius::same(9), bfill);
        ui.painter().galley(cpos, cg, count_fg);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Petit badge arrondi (visibilité, statut).
pub fn badge(
    painter: &egui::Painter,
    right_center: Pos2,
    text: &str,
    fg: Color32,
    bg: Color32,
) -> Rect {
    let g = painter.layout_no_wrap(text.into(), FontId::proportional(11.5), fg);
    let size = g.size() + vec2(14.0, 4.0);
    let rect = Rect::from_min_size(right_center - vec2(size.x, size.y / 2.0), size);
    painter.rect_filled(rect, CornerRadius::same(9), bg);
    painter.galley(rect.min + vec2(7.0, 2.0), g, fg);
    rect
}

/// Bouton plein (accent) ou discret.
pub fn button(ui: &mut Ui, text: &str, primary: bool) -> Response {
    let pal = Pal::of(ui);
    let fg = if primary { Color32::WHITE } else { pal.text };
    let b = egui::Button::new(egui::RichText::new(text).color(fg).size(14.0))
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 34.0));
    let b = if primary {
        b.fill(pal.accent).stroke(Stroke::NONE)
    } else {
        b.fill(pal.surface).stroke(Stroke::new(1.0, pal.border))
    };
    ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Touche de clavier dessinée (aide en bas de fenêtre).
pub fn keycap(ui: &mut Ui, key: &str, what: &str) {
    let pal = Pal::of(ui);
    let g = ui
        .painter()
        .layout_no_wrap(key.into(), FontId::proportional(11.5), pal.weak);
    let (rect, _) = ui.allocate_exact_size(vec2(g.size().x + 10.0, 20.0), Sense::hover());
    ui.painter().rect(
        rect,
        CornerRadius::same(5),
        pal.surface,
        Stroke::new(1.0, pal.border),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - g.size() / 2.0, g, pal.weak);
    ui.add_space(-3.0);
    ui.label(egui::RichText::new(what).size(12.0).color(pal.faint));
    ui.add_space(4.0);
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

/// Taille d'un texte sur une ligne.
pub fn text_width(ui: &Ui, text: &str, font: FontId) -> Vec2 {
    ui.painter()
        .layout_no_wrap(text.into(), font, Color32::WHITE)
        .size()
}

/// Indicateur d'activité. Comme egui::Spinner, mais à 30 images/s au plus :
/// sans vsync, une animation qui redemande un rendu à chaque image tournerait
/// à vide.
pub fn spinner(ui: &mut Ui, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    spinner_at(ui, rect, color);
    resp
}

pub fn spinner_at(ui: &Ui, rect: Rect, color: Color32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
    let radius = rect.height().min(rect.width()) / 2.0 - 1.5;
    let n = (radius.round() as u32).clamp(8, 64);
    let time = ui.input(|i| i.time);
    let start = time * std::f64::consts::TAU;
    let end = start + 240f64.to_radians() * time.sin();
    let points: Vec<Pos2> = (0..n)
        .map(|i| {
            let a = start + (end - start) * i as f64 / n as f64;
            let (sin, cos) = a.sin_cos();
            rect.center() + radius * vec2(cos as f32, sin as f32)
        })
        .collect();
    ui.painter()
        .add(Shape::line(points, Stroke::new(2.0, color)));
}
