//! Icons drawn with egui's painter: file and folder icons in the colour of their kind, and the
//! line symbols of the toolbar and the sidebar. Vector shapes stay sharp at any DeX density.

use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use crate::fileinfo::Category;

fn lighten(c: Color32, f: f32) -> Color32 {
    let l = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * f) as u8;
    Color32::from_rgb(l(c.r()), l(c.g()), l(c.b()))
}

fn darken(c: Color32, f: f32) -> Color32 {
    let d = |v: u8| (f32::from(v) * (1.0 - f)) as u8;
    Color32::from_rgb(d(c.r()), d(c.g()), d(c.b()))
}

/// A folder or a file icon filling `rect` (kept square, centred).
pub fn file_icon(painter: &Painter, rect: Rect, category: Category, ext: &str, dark: bool) {
    let side = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(side, side));
    if category == Category::Folder {
        folder(painter, r, category.color());
    } else {
        document(painter, r, category, ext, dark);
    }
}

pub fn folder(painter: &Painter, r: Rect, color: Color32) {
    let s = r.width();
    let round = (s * 0.08).max(1.5) as u8;
    let body_top = r.top() + s * 0.24;
    let back = Rect::from_min_max(pos2(r.left() + s * 0.04, r.top() + s * 0.14), pos2(r.right() - s * 0.04, r.bottom() - s * 0.12));
    // the tab
    let tab = Rect::from_min_max(pos2(back.left(), back.top()), pos2(back.left() + s * 0.38, body_top + s * 0.04));
    painter.rect_filled(tab, CornerRadius::same(round), darken(color, 0.12));
    painter.rect_filled(back, CornerRadius::same(round), darken(color, 0.12));
    let front = Rect::from_min_max(pos2(back.left(), body_top), back.max);
    painter.rect_filled(front, CornerRadius::same(round), color);
    // a soft highlight at the top of the front
    let shine = Rect::from_min_max(front.min, pos2(front.right(), front.top() + (s * 0.05).max(1.0)));
    painter.rect_filled(shine, CornerRadius { nw: round, ne: round, sw: 0, se: 0 }, lighten(color, 0.3));
}

fn document(painter: &Painter, r: Rect, category: Category, ext: &str, dark: bool) {
    let s = r.width();
    let page = Rect::from_min_max(pos2(r.left() + s * 0.17, r.top() + s * 0.06), pos2(r.right() - s * 0.17, r.bottom() - s * 0.06));
    let fold = page.width() * 0.32;
    let paper = if dark { Color32::from_rgb(0xee, 0xee, 0xf0) } else { Color32::WHITE };
    let edge = if dark { Color32::from_rgb(0x88, 0x8a, 0x90) } else { Color32::from_rgb(0xc4, 0xc6, 0xcc) };
    let pts = vec![
        page.left_top(),
        pos2(page.right() - fold, page.top()),
        pos2(page.right(), page.top() + fold),
        page.right_bottom(),
        page.left_bottom(),
    ];
    painter.add(Shape::convex_polygon(pts.clone(), paper, Stroke::NONE));
    painter.add(Shape::closed_line(pts, Stroke::new((s / 64.0).max(0.8), edge)));
    let corner = vec![pos2(page.right() - fold, page.top()), pos2(page.right() - fold, page.top() + fold), pos2(page.right(), page.top() + fold)];
    painter.add(Shape::convex_polygon(corner, lighten(edge, 0.45), Stroke::new((s / 64.0).max(0.8), edge)));
    let color = category.color();
    if s >= 40.0 && !ext.is_empty() {
        // a coloured band with the extension
        let band = Rect::from_min_max(pos2(page.left() - s * 0.04, page.bottom() - s * 0.30), pos2(page.right() + s * 0.04, page.bottom() - s * 0.10));
        painter.rect_filled(band, CornerRadius::same((s * 0.04) as u8), color);
        let label: String = ext.to_uppercase().chars().take(5).collect();
        painter.text(band.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional((s * 0.15).max(8.0)), Color32::WHITE);
        glyph(painter, Rect::from_min_max(pos2(page.left(), page.top() + s * 0.2), pos2(page.right(), band.top() - s * 0.03)), category, color);
    } else {
        glyph(painter, page.shrink(s * 0.08), category, color);
    }
}

/// A small mark of the kind, inside the page.
fn glyph(painter: &Painter, r: Rect, category: Category, color: Color32) {
    let s = r.width().min(r.height());
    if s < 4.0 {
        return;
    }
    let c = r.center();
    let w = (s / 10.0).max(1.0);
    match category {
        Category::Image => {
            let base = r.bottom() - s * 0.15;
            painter.add(Shape::convex_polygon(vec![pos2(c.x - s * 0.4, base), pos2(c.x - s * 0.1, c.y - s * 0.15), pos2(c.x + s * 0.12, base)], color, Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![pos2(c.x - s * 0.05, base), pos2(c.x + s * 0.18, c.y), pos2(c.x + s * 0.4, base)], lighten(color, 0.35), Stroke::NONE));
            painter.circle_filled(pos2(c.x + s * 0.22, c.y - s * 0.28), s * 0.1, lighten(color, 0.2));
        }
        Category::Video => {
            painter.add(Shape::convex_polygon(vec![pos2(c.x - s * 0.2, c.y - s * 0.28), pos2(c.x + s * 0.3, c.y), pos2(c.x - s * 0.2, c.y + s * 0.28)], color, Stroke::NONE));
        }
        Category::Audio => {
            painter.line_segment([pos2(c.x + s * 0.12, c.y - s * 0.35), pos2(c.x + s * 0.12, c.y + s * 0.2)], Stroke::new(w, color));
            painter.line_segment([pos2(c.x + s * 0.12, c.y - s * 0.35), pos2(c.x + s * 0.32, c.y - s * 0.25)], Stroke::new(w, color));
            painter.circle_filled(pos2(c.x - s * 0.02, c.y + s * 0.22), s * 0.16, color);
        }
        Category::Archive => {
            for i in 0..4 {
                let y = r.top() + s * 0.1 + i as f32 * s * 0.18;
                let x = if i % 2 == 0 { c.x - s * 0.06 } else { c.x + s * 0.06 };
                painter.rect_filled(Rect::from_center_size(pos2(x, y), vec2(s * 0.12, s * 0.1)), 1, color);
            }
        }
        Category::Code => {
            let st = Stroke::new(w, color);
            painter.line_segment([pos2(c.x - s * 0.12, c.y - s * 0.2), pos2(c.x - s * 0.32, c.y)], st);
            painter.line_segment([pos2(c.x - s * 0.32, c.y), pos2(c.x - s * 0.12, c.y + s * 0.2)], st);
            painter.line_segment([pos2(c.x + s * 0.12, c.y - s * 0.2), pos2(c.x + s * 0.32, c.y)], st);
            painter.line_segment([pos2(c.x + s * 0.32, c.y), pos2(c.x + s * 0.12, c.y + s * 0.2)], st);
        }
        Category::Spreadsheet => {
            let g = Rect::from_center_size(c, vec2(s * 0.7, s * 0.55));
            painter.rect_stroke(g, 1, Stroke::new(w * 0.8, color), StrokeKind::Inside);
            for i in 1..3 {
                let y = g.top() + g.height() * i as f32 / 3.0;
                painter.line_segment([pos2(g.left(), y), pos2(g.right(), y)], Stroke::new(w * 0.6, color));
            }
            painter.line_segment([pos2(g.center().x, g.top()), pos2(g.center().x, g.bottom())], Stroke::new(w * 0.6, color));
        }
        Category::App => {
            let g = Rect::from_center_size(c, vec2(s * 0.5, s * 0.5));
            painter.rect_filled(g, CornerRadius::same((s * 0.1) as u8), color);
        }
        _ => {
            // text lines
            for i in 0..4 {
                let y = r.top() + s * 0.15 + i as f32 * s * 0.18;
                let len = if i == 3 { 0.4 } else { 0.7 };
                painter.line_segment([pos2(c.x - s * 0.35, y), pos2(c.x - s * 0.35 + s * len, y)], Stroke::new(w * 0.8, lighten(color, 0.2)));
            }
        }
    }
}

/// Line symbols for buttons and the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sym {
    Back,
    Forward,
    Up,
    Refresh,
    Grid,
    List,
    Details,
    Content,
    Search,
    Settings,
    Star,
    Home,
    Drive,
    Sd,
    Network,
    Server,
    Trash,
    Plus,
    Preview,
    DualPane,
    Sidebar,
    Close,
    Folder,
    Download,
    Document,
    Image,
    Music,
    Film,
    Camera,
    Desktop,
    Copy,
    Sort,
    Eye,
    Edit,
    More,
    Share,
    Clock,
    Root,
}

pub fn sym(painter: &Painter, rect: Rect, s: Sym, color: Color32) {
    let size = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(size, size)).shrink(size * 0.14);
    let w = (size / 12.0).clamp(1.2, 2.2);
    let st = Stroke::new(w, color);
    let c = r.center();
    let h = r.width() / 2.0;
    let p = |x: f32, y: f32| pos2(c.x + x * h, c.y + y * h);
    let line = |pts: &[Pos2]| {
        painter.add(Shape::line(pts.to_vec(), st));
    };
    match s {
        Sym::Back => line(&[p(0.3, -0.7), p(-0.4, 0.0), p(0.3, 0.7)]),
        Sym::Forward => line(&[p(-0.3, -0.7), p(0.4, 0.0), p(-0.3, 0.7)]),
        Sym::Up => line(&[p(-0.7, 0.3), p(0.0, -0.4), p(0.7, 0.3)]),
        Sym::Refresh => {
            let n = 20;
            let pts: Vec<Pos2> = (0..=n).map(|i| {
                let a = 0.6 + (i as f32 / n as f32) * 5.0;
                p(0.7 * a.cos(), 0.7 * a.sin())
            }).collect();
            let end = pts[pts.len() - 1];
            line(&pts);
            line(&[pos2(end.x - h * 0.35, end.y - h * 0.05), end, pos2(end.x + h * 0.05, end.y - h * 0.38)]);
        }
        Sym::Grid => {
            for (x, y) in [(-0.4, -0.4), (0.4, -0.4), (-0.4, 0.4), (0.4, 0.4)] {
                painter.rect_stroke(Rect::from_center_size(p(x, y), Vec2::splat(h * 0.6)), 2, st, StrokeKind::Middle);
            }
        }
        Sym::List => {
            for y in [-0.6, 0.0, 0.6] {
                painter.circle_filled(p(-0.75, y), w * 0.8, color);
                line(&[p(-0.4, y), p(0.85, y)]);
            }
        }
        Sym::Details => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(h * 1.8, h * 1.6)), 2, st, StrokeKind::Middle);
            line(&[p(-0.9, -0.3), p(0.9, -0.3)]);
            line(&[p(-0.9, 0.25), p(0.9, 0.25)]);
            line(&[p(-0.2, -0.8), p(-0.2, 0.8)]);
        }
        Sym::Content => {
            for y in [-0.45, 0.45] {
                painter.rect_stroke(Rect::from_center_size(p(-0.55, y), Vec2::splat(h * 0.6)), 2, st, StrokeKind::Middle);
                line(&[p(-0.05, y - 0.15), p(0.9, y - 0.15)]);
                line(&[p(-0.05, y + 0.15), p(0.5, y + 0.15)]);
            }
        }
        Sym::Search => {
            painter.circle_stroke(p(-0.15, -0.15), h * 0.55, st);
            line(&[p(0.25, 0.25), p(0.8, 0.8)]);
        }
        Sym::Settings => {
            painter.circle_stroke(c, h * 0.3, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                line(&[p(0.5 * a.cos(), 0.5 * a.sin()), p(0.85 * a.cos(), 0.85 * a.sin())]);
            }
            painter.circle_stroke(c, h * 0.55, st);
        }
        Sym::Star => {
            let pts: Vec<Pos2> = (0..10).map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let rad = if i % 2 == 0 { 0.9 } else { 0.4 };
                p(rad * a.cos(), rad * a.sin())
            }).collect();
            painter.add(Shape::closed_line(pts, st));
        }
        Sym::Home => {
            line(&[p(-0.85, 0.0), p(0.0, -0.8), p(0.85, 0.0)]);
            line(&[p(-0.6, -0.2), p(-0.6, 0.8), p(0.6, 0.8), p(0.6, -0.2)]);
            line(&[p(-0.15, 0.8), p(-0.15, 0.3), p(0.15, 0.3), p(0.15, 0.8)]);
        }
        Sym::Drive | Sym::Root => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(h * 1.8, h * 1.0)), 3, st, StrokeKind::Middle);
            painter.circle_filled(p(0.55, 0.0), w, color);
            if s == Sym::Root {
                line(&[p(-0.6, 0.0), p(0.1, 0.0)]);
            }
        }
        Sym::Sd => {
            line(&[p(-0.6, -0.9), p(0.3, -0.9), p(0.65, -0.55), p(0.65, 0.9), p(-0.6, 0.9), p(-0.6, -0.9)]);
            for x in [-0.3, 0.0, 0.3] {
                line(&[p(x, -0.6), p(x, -0.3)]);
            }
        }
        Sym::Network => {
            painter.circle_stroke(c, h * 0.85, st);
            line(&[p(-0.85, 0.0), p(0.85, 0.0)]);
            let n = 12;
            let arc = |sign: f32| -> Vec<Pos2> { (0..=n).map(|i| {
                let t = -1.0 + 2.0 * i as f32 / n as f32;
                p(sign * 0.4 * (1.0 - t * t).sqrt(), 0.85 * t)
            }).collect() };
            line(&arc(1.0));
            line(&arc(-1.0));
        }
        Sym::Server => {
            for y in [-0.45, 0.45] {
                painter.rect_stroke(Rect::from_center_size(p(0.0, y), vec2(h * 1.7, h * 0.75)), 2, st, StrokeKind::Middle);
                painter.circle_filled(p(-0.5, y), w * 0.8, color);
            }
        }
        Sym::Trash => {
            line(&[p(-0.8, -0.55), p(0.8, -0.55)]);
            line(&[p(-0.25, -0.55), p(-0.2, -0.85), p(0.2, -0.85), p(0.25, -0.55)]);
            line(&[p(-0.6, -0.55), p(-0.48, 0.85), p(0.48, 0.85), p(0.6, -0.55)]);
            line(&[p(-0.15, -0.2), p(-0.12, 0.55)]);
            line(&[p(0.15, -0.2), p(0.12, 0.55)]);
        }
        Sym::Plus => {
            line(&[p(0.0, -0.8), p(0.0, 0.8)]);
            line(&[p(-0.8, 0.0), p(0.8, 0.0)]);
        }
        Sym::Preview | Sym::Sidebar | Sym::DualPane => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(h * 1.9, h * 1.5)), 3, st, StrokeKind::Middle);
            match s {
                Sym::Preview => line(&[p(0.3, -0.75), p(0.3, 0.75)]),
                Sym::Sidebar => line(&[p(-0.3, -0.75), p(-0.3, 0.75)]),
                _ => line(&[p(0.0, -0.75), p(0.0, 0.75)]),
            }
        }
        Sym::Close => {
            line(&[p(-0.6, -0.6), p(0.6, 0.6)]);
            line(&[p(0.6, -0.6), p(-0.6, 0.6)]);
        }
        Sym::Folder => {
            line(&[p(-0.9, 0.75), p(-0.9, -0.7), p(-0.3, -0.7), p(-0.1, -0.45), p(0.9, -0.45), p(0.9, 0.75), p(-0.9, 0.75)]);
        }
        Sym::Download => {
            line(&[p(0.0, -0.85), p(0.0, 0.35)]);
            line(&[p(-0.45, -0.05), p(0.0, 0.4), p(0.45, -0.05)]);
            line(&[p(-0.8, 0.85), p(0.8, 0.85)]);
        }
        Sym::Document => {
            line(&[p(-0.6, -0.9), p(0.25, -0.9), p(0.65, -0.5), p(0.65, 0.9), p(-0.6, 0.9), p(-0.6, -0.9)]);
            line(&[p(-0.3, -0.1), p(0.35, -0.1)]);
            line(&[p(-0.3, 0.3), p(0.35, 0.3)]);
        }
        Sym::Image => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(h * 1.8, h * 1.5)), 2, st, StrokeKind::Middle);
            line(&[p(-0.8, 0.6), p(-0.25, 0.0), p(0.15, 0.4), p(0.4, 0.15), p(0.85, 0.6)]);
            painter.circle_filled(p(0.35, -0.3), w * 1.2, color);
        }
        Sym::Music => {
            line(&[p(-0.2, 0.55), p(-0.2, -0.75), p(0.6, -0.9), p(0.6, 0.4)]);
            painter.circle_filled(p(-0.42, 0.6), h * 0.25, color);
            painter.circle_filled(p(0.38, 0.45), h * 0.25, color);
        }
        Sym::Film => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(h * 1.8, h * 1.5)), 2, st, StrokeKind::Middle);
            painter.add(Shape::convex_polygon(vec![p(-0.25, -0.35), p(0.4, 0.0), p(-0.25, 0.35)], color, Stroke::NONE));
        }
        Sym::Camera => {
            painter.rect_stroke(Rect::from_center_size(p(0.0, 0.1), vec2(h * 1.8, h * 1.3)), 3, st, StrokeKind::Middle);
            painter.circle_stroke(p(0.0, 0.12), h * 0.35, st);
            line(&[p(-0.35, -0.55), p(-0.2, -0.8), p(0.2, -0.8), p(0.35, -0.55)]);
        }
        Sym::Desktop => {
            painter.rect_stroke(Rect::from_center_size(p(0.0, -0.15), vec2(h * 1.8, h * 1.2)), 2, st, StrokeKind::Middle);
            line(&[p(-0.4, 0.85), p(0.4, 0.85)]);
            line(&[p(0.0, 0.45), p(0.0, 0.85)]);
        }
        Sym::Copy => {
            painter.rect_stroke(Rect::from_min_size(p(-0.8, -0.8), Vec2::splat(h * 1.1)), 2, st, StrokeKind::Middle);
            painter.rect_stroke(Rect::from_min_size(p(-0.3, -0.3), Vec2::splat(h * 1.1)), 2, st, StrokeKind::Middle);
        }
        Sym::Sort => {
            line(&[p(-0.4, 0.8), p(-0.4, -0.8)]);
            line(&[p(-0.75, -0.45), p(-0.4, -0.8), p(-0.05, -0.45)]);
            line(&[p(0.4, -0.8), p(0.4, 0.8)]);
            line(&[p(0.05, 0.45), p(0.4, 0.8), p(0.75, 0.45)]);
        }
        Sym::Eye => {
            let n = 16;
            let top: Vec<Pos2> = (0..=n).map(|i| { let t = -1.0 + 2.0 * i as f32 / n as f32; p(0.9 * t, -0.5 * (1.0 - t * t)) }).collect();
            let bottom: Vec<Pos2> = (0..=n).map(|i| { let t = -1.0 + 2.0 * i as f32 / n as f32; p(0.9 * t, 0.5 * (1.0 - t * t)) }).collect();
            line(&top);
            line(&bottom);
            painter.circle_stroke(c, h * 0.22, st);
        }
        Sym::Edit => {
            line(&[p(-0.7, 0.7), p(-0.6, 0.3), p(0.4, -0.7), p(0.7, -0.4), p(-0.3, 0.6), p(-0.7, 0.7)]);
        }
        Sym::More => {
            for x in [-0.55, 0.0, 0.55] {
                painter.circle_filled(p(x, 0.0), w * 1.1, color);
            }
        }
        Sym::Share => {
            line(&[p(0.0, 0.3), p(0.0, -0.85)]);
            line(&[p(-0.4, -0.45), p(0.0, -0.85), p(0.4, -0.45)]);
            line(&[p(-0.35, -0.2), p(-0.7, -0.2), p(-0.7, 0.85), p(0.7, 0.85), p(0.7, -0.2), p(0.35, -0.2)]);
        }
        Sym::Clock => {
            painter.circle_stroke(c, h * 0.85, st);
            line(&[p(0.0, -0.5), p(0.0, 0.0), p(0.4, 0.25)]);
        }
    }
}

/// A flat button showing a symbol; `on`: drawn as pressed.
pub fn sym_button(ui: &mut Ui, s: Sym, on: bool, tooltip: &str) -> Response {
    let size = Vec2::splat(ui.spacing().interact_size.y + 2.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let vis = ui.visuals();
    let bg = if on {
        Some(vis.widgets.active.weak_bg_fill)
    } else if resp.hovered() && enabled {
        Some(vis.widgets.hovered.weak_bg_fill)
    } else {
        None
    };
    if let Some(bg) = bg {
        ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
    }
    let color = if !enabled { vis.weak_text_color() } else if on { super::theme::ACCENT } else { vis.text_color() };
    sym(ui.painter(), rect.shrink(4.0), s, color);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}
