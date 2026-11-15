//! Icons drawn with shapes, because egui's bundled fonts have no glyphs for them.

use eframe::egui;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke};

/// A circled "i" in `colour`, filling `rect`, a square.
pub fn paint_info(painter: &Painter, rect: Rect, colour: Color32) {
    let size = rect.width();
    painter.circle_stroke(rect.center(), size / 2.0 - 0.5, Stroke::new(1.0, colour));
    let font = FontId::proportional(size * 0.75);
    painter.text(rect.center(), Align2::CENTER_CENTER, "i", font, colour);
}

/// A warning sign, a "!" in a triangle, in `colour`, filling `rect`, a square.
pub fn paint_warning(painter: &Painter, rect: Rect, colour: Color32) {
    let size = rect.width();
    let stroke = Stroke::new((size / 12.0).max(1.0), colour);
    let inset = rect.shrink(stroke.width);
    let (top, bottom) = (
        inset.top() + inset.height() * 0.07,
        inset.bottom() - inset.height() * 0.07,
    );
    let points = vec![
        Pos2::new(inset.center().x, top),
        Pos2::new(inset.right(), bottom),
        Pos2::new(inset.left(), bottom),
    ];
    painter.add(Shape::convex_polygon(points, Color32::TRANSPARENT, stroke));
    // In the triangle's lower, wider part.
    let at = Pos2::new(inset.center().x, top + (bottom - top) * 0.62);
    let font = FontId::proportional(size * 0.55);
    painter.text(at, Align2::CENTER_CENTER, "!", font, colour);
}
