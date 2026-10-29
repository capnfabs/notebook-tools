//! The page's corners over the original photo, joined by its sides. Each corner can be dragged;
//! or, while placing, the user clicks four new ones. Corners are in the photo's pixels.

use eframe::egui;
use egui::accesskit::Role;
use egui::{Color32, CursorIcon, Key, Pos2, Rect, Sense, Shape, Stroke, Vec2, WidgetInfo};
use notebook_core::{Quad, order_corners};

/// What each corner's handle is called, in [`Quad`] order.
pub const LABELS: [&str; 4] = [
    "Top-left corner",
    "Top-right corner",
    "Bottom-right corner",
    "Bottom-left corner",
];

/// What the area clicked while placing corners is called.
pub const PLACING_LABEL: &str = "Click the page's corners";

/// Radius of a corner's handle, in points. It can be grabbed a little further out.
const HANDLE_RADIUS: f32 = 5.0;
const GRAB_RADIUS: f32 = 10.0;

/// Corners being clicked in, for the page named `name`.
struct Placing {
    name: String,
    points: Vec<[f32; 2]>,
}

/// Which corner is being dragged, and where it is on screen.
#[derive(Clone, Copy)]
struct Drag {
    corner: usize,
    at: Pos2,
}

#[derive(Default)]
pub struct Corners {
    drag: Option<Drag>,
    placing: Option<Placing>,
}

impl Corners {
    /// Starts clicking in new corners for the page named `name`, forgetting any clicked so far.
    pub fn place(&mut self, name: &str) {
        self.drag = None;
        self.placing = Some(Placing {
            name: name.to_owned(),
            points: Vec::new(),
        });
    }

    /// Whether new corners are being clicked in for the page named `name`.
    pub fn placing(&self, name: &str) -> bool {
        self.placing.as_ref().is_some_and(|p| p.name == name)
    }

    /// `quad`, the corners of the page named `name`, over its photo of `source_size` pixels drawn
    /// at `image_rect`. Returns new corners once the user lets go of one, or clicks in the fourth.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        quad: Quad,
        source_size: (u32, u32),
        image_rect: Rect,
    ) -> Option<Quad> {
        let (w, h) = (source_size.0 as f32, source_size.1 as f32);
        let scale = image_rect.size() / Vec2::new(w, h);
        let to_screen = |[x, y]: [f32; 2]| image_rect.min + Vec2::new(x, y) * scale;
        let to_source = |p: Pos2| {
            let p = image_rect.clamp(p) - image_rect.min;
            [p.x / scale.x, p.y / scale.y]
        };
        if self.placing.as_ref().is_some_and(|p| p.name != name) {
            self.placing = None;
        }
        if self.placing.is_some() {
            return self.place_points(ui, image_rect, to_screen, to_source);
        }

        let placed = quad.map(to_screen);
        let mut corners = placed;
        let mut changed = None;
        for (i, label) in LABELS.into_iter().enumerate() {
            let at = match self.drag {
                Some(drag) if drag.corner == i => drag.at,
                _ => corners[i],
            };
            let handle = Rect::from_center_size(at, Vec2::splat(2.0 * GRAB_RADIUS));
            let id = ui.scope_id().with(("page corner", i));
            let response = ui
                .interact(handle, id, Sense::drag())
                .on_hover_cursor(CursorIcon::Grab);
            response.widget_info(|| WidgetInfo::labeled(Role::GraphicsObject, true, label));
            if response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                let at = image_rect.clamp(at + response.drag_delta());
                self.drag = Some(Drag { corner: i, at });
            }
            if response.drag_stopped() {
                self.drag = None;
                // A press that didn't move the corner changes nothing.
                if at.distance(placed[i]) > 0.5 {
                    let mut moved = quad;
                    moved[i] = to_source(at);
                    changed = Some(order_corners(moved));
                }
            }
            if let Some(drag) = self.drag.filter(|d| d.corner == i) {
                corners[i] = drag.at;
            }
        }

        let painter = ui.painter_at(image_rect.expand(GRAB_RADIUS));
        let (line, fill) = colours(ui);
        painter.add(Shape::closed_line(corners.to_vec(), line));
        for corner in corners {
            painter.circle(corner, HANDLE_RADIUS, fill, line);
        }
        changed
    }

    /// Takes clicks on the image as corners, the points so far joined in the order clicked and the
    /// last one to the pointer. Escape gives up. Returns the corners once there are four.
    fn place_points(
        &mut self,
        ui: &mut egui::Ui,
        image_rect: Rect,
        to_screen: impl Fn([f32; 2]) -> Pos2,
        to_source: impl Fn(Pos2) -> [f32; 2],
    ) -> Option<Quad> {
        let placing = self.placing.as_mut()?;
        let response = ui
            .interact(
                image_rect,
                ui.scope_id().with("place corners"),
                Sense::click(),
            )
            .on_hover_cursor(CursorIcon::Crosshair);
        response.widget_info(|| WidgetInfo::labeled(Role::Canvas, true, PLACING_LABEL));
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            placing.points.push(to_source(pos));
        }
        if let Ok(points) = <[[f32; 2]; 4]>::try_from(placing.points.as_slice()) {
            self.placing = None;
            return Some(order_corners(points));
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.placing = None;
            return None;
        }

        let mut path: Vec<Pos2> = placing.points.iter().copied().map(to_screen).collect();
        let painter = ui.painter_at(image_rect.expand(GRAB_RADIUS));
        let (line, fill) = colours(ui);
        for &point in &path {
            painter.circle(point, HANDLE_RADIUS, fill, line);
        }
        if let Some(pointer) = response.hover_pos()
            && !path.is_empty()
        {
            path.push(pointer);
        }
        painter.add(Shape::line(path, line));
        None
    }
}

/// The sides' stroke, and the handles' fill.
fn colours(ui: &egui::Ui) -> (Stroke, Color32) {
    let accent = ui.visuals().selection.bg_fill;
    (Stroke::new(2.0, accent), Color32::WHITE)
}
