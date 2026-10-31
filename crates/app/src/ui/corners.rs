//! The page's corners over the original photo, joined by its sides. Each corner can be dragged;
//! or, while placing, the user clicks four new ones. Corners are in the photo's pixels. Only
//! outlines that can be straightened ([`is_page_shaped`]) are allowed: a dragged corner stops
//! where it would spoil the shape, and a fourth click that would is refused. A corner's handle
//! can also take the keyboard focus, and then the arrow keys nudge it.
//!
//! See `decisions/2026-10-29-manual-corners.md` and `decisions/2026-10-31-keyboard-focus.md`.

use eframe::egui;
use egui::accesskit::Role;
use egui::{
    Color32, CursorIcon, Id, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke, Vec2, WidgetInfo,
};
use notebook_core::geom::{Quad, is_page_shaped, order_corners};

use crate::t;

/// What corner `i`'s handle is called, in [`Quad`] order.
fn label(i: usize) -> String {
    match i {
        0 => t!("corner-top-left"),
        1 => t!("corner-top-right"),
        2 => t!("corner-bottom-right"),
        _ => t!("corner-bottom-left"),
    }
}

/// Radius of a corner's handle, in points. It can be grabbed a little further out.
const HANDLE_RADIUS: f32 = 5.0;
const GRAB_RADIUS: f32 = 10.0;
/// Radius of the ring around the handle that has the keyboard focus, in points.
const FOCUS_RADIUS: f32 = HANDLE_RADIUS + 3.0;

/// How far an arrow key nudges a corner, in points; with Shift, further.
const NUDGE: f32 = 1.0;
const SHIFT_NUDGE: f32 = 10.0;

const ARROWS: [Key; 4] = [
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowUp,
    Key::ArrowDown,
];

/// How many times a dragged corner halves the distance to where the pointer is, looking for the
/// furthest it can go without spoiling the outline.
const CONSTRAIN_STEPS: usize = 12;

/// Corners being clicked in, for the page named `name`.
struct Placing {
    name: String,
    points: Vec<[f32; 2]>,
    /// The last fourth corner clicked was refused.
    refused: bool,
}

/// Which corner is being dragged, where it is on screen, and where it is from the pointer.
#[derive(Clone, Copy)]
struct Drag {
    corner: usize,
    at: Pos2,
    grip: Vec2,
}

/// Which corner the arrow keys are nudging, and where it is on screen, until they're let go.
#[derive(Clone, Copy)]
struct Nudge {
    corner: usize,
    at: Pos2,
}

#[derive(Default)]
pub struct Corners {
    drag: Option<Drag>,
    nudge: Option<Nudge>,
    placing: Option<Placing>,
}

impl Corners {
    /// Starts clicking in new corners for the page named `name`, forgetting any clicked so far.
    pub fn place(&mut self, name: &str) {
        self.drag = None;
        self.nudge = None;
        self.placing = Some(Placing {
            name: name.to_owned(),
            points: Vec::new(),
            refused: false,
        });
    }

    /// What to tell the user while new corners are being clicked in for the page named `name`:
    /// to click them, or why a fourth was refused.
    pub fn prompt(&self, name: &str) -> Option<String> {
        let placing = self.placing.as_ref().filter(|p| p.name == name)?;
        Some(if placing.refused {
            t!("corners-refused-prompt")
        } else {
            t!("corners-placing-prompt")
        })
    }

    /// `quad`, the corners of the page named `name`, over its photo of `source_size` pixels drawn
    /// at `image_rect`. Returns new corners once the user lets go of one, whether dragged or nudged
    /// with the arrow keys, or clicks in the fourth.
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
        let mut focused = None;
        let ids: [Id; 4] = std::array::from_fn(|i| ui.scope_id().with(("page corner", i)));
        // A press that didn't move the corner, or would spoil the outline, changes nothing.
        let moved_to = |i: usize, at: Pos2| {
            let mut moved = quad;
            moved[i] = to_source(at);
            (at.distance(placed[i]) > 0.5 && is_page_shaped(&moved)).then(|| order_corners(moved))
        };
        for (i, id) in ids.into_iter().enumerate() {
            let at = match (self.drag, self.nudge) {
                (Some(drag), _) if drag.corner == i => drag.at,
                (_, Some(nudge)) if nudge.corner == i => nudge.at,
                _ => corners[i],
            };
            let handle = Rect::from_center_size(at, Vec2::splat(2.0 * GRAB_RADIUS));
            let response = ui
                .interact(handle, id, Sense::drag())
                .on_hover_cursor(CursorIcon::Grab);
            response.widget_info(|| WidgetInfo::labeled(Role::GraphicsObject, true, label(i)));
            let fits = |p: Pos2| {
                let mut moved = quad;
                moved[i] = to_source(p);
                is_page_shaped(&moved)
            };
            corners[i] = at;
            if response.dragged()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                let grip = match self.drag {
                    Some(drag) if drag.corner == i => drag.grip,
                    _ => at - ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer),
                };
                let wanted = image_rect.clamp(pointer + grip);
                let at = furthest(at, wanted, fits);
                self.drag = Some(Drag {
                    corner: i,
                    at,
                    grip,
                });
                corners[i] = at;
            }
            if response.drag_stopped() {
                self.drag = None;
                // Drawn where it was let go, not back where it was, until the new corners come in.
                changed = moved_to(i, at);
                if changed.is_none() {
                    corners[i] = placed[i];
                }
            }

            if response.has_focus() {
                focused = Some(i);
                let step = nudge(ui);
                if step != Vec2::ZERO {
                    let at = furthest(at, image_rect.clamp(at + step), fits);
                    self.nudge = Some(Nudge { corner: i, at });
                    corners[i] = at;
                }
            }
            // Once the arrows are let go, or the handle loses the focus, the nudges so far count as
            // one move, so holding an arrow down cleans the page once.
            let held = ui.input(|input| ARROWS.iter().any(|&k| input.key_down(k)));
            if let Some(nudge) = self.nudge
                && nudge.corner == i
                && !(held && response.has_focus())
            {
                self.nudge = None;
                changed = moved_to(i, nudge.at);
            }
        }

        paint_corners(ui, image_rect, corners, focused);
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
        response
            .widget_info(|| WidgetInfo::labeled(Role::Canvas, true, t!("corners-placing-label")));
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            placing.points.push(to_source(pos));
        }
        if let Ok(points) = <[[f32; 2]; 4]>::try_from(placing.points.as_slice()) {
            let quad = order_corners(points);
            if is_page_shaped(&quad) {
                self.placing = None;
                return Some(quad);
            }
            placing.points.pop();
            placing.refused = true;
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

/// How far this frame's arrow presses move the focused corner, on screen. Takes the presses, so
/// nothing else acts on them, and keeps them from moving the focus.
fn nudge(ui: &egui::Ui) -> Vec2 {
    crate::ui::focus::keep_arrows(ui.ctx());
    ui.input_mut(|i| {
        let step = if i.modifiers.shift {
            SHIFT_NUDGE
        } else {
            NUDGE
        };
        // Matching with no modifiers also takes the presses with Shift.
        let mut presses = |key| i.count_and_consume_key(Modifiers::NONE, key) as f32;
        let x = presses(Key::ArrowRight) - presses(Key::ArrowLeft);
        let y = presses(Key::ArrowDown) - presses(Key::ArrowUp);
        Vec2::new(x, y) * step
    })
}

/// The furthest point from `from` towards `to` that `fits`. If `from` doesn't fit either, `to`:
/// an outline that's already unusable doesn't hold the corner back.
fn furthest(from: Pos2, to: Pos2, fits: impl Fn(Pos2) -> bool) -> Pos2 {
    if fits(to) || !fits(from) {
        return to;
    }
    let (mut good, mut bad) = (0.0, 1.0);
    for _ in 0..CONSTRAIN_STEPS {
        let mid = (good + bad) / 2.0;
        if fits(from.lerp(to, mid)) {
            good = mid;
        } else {
            bad = mid;
        }
    }
    from.lerp(to, good)
}

/// The sides' stroke, and the handles' fill.
fn colours(ui: &egui::Ui) -> (Stroke, Color32) {
    let accent = ui.visuals().selection.bg_fill;
    (Stroke::new(2.0, accent), Color32::WHITE)
}

/// The page's outline through `corners`, on screen, with a handle on each, over the image at
/// `image_rect`. The handle of corner `focused` is ringed.
fn paint_corners(ui: &egui::Ui, image_rect: Rect, corners: [Pos2; 4], focused: Option<usize>) {
    let painter = ui.painter_at(image_rect.expand(GRAB_RADIUS));
    let (line, fill) = colours(ui);
    painter.add(Shape::closed_line(corners.to_vec(), line));
    for corner in corners {
        painter.circle(corner, HANDLE_RADIUS, fill, line);
    }
    if let Some(i) = focused {
        let ring = Stroke::new(2.0, ui.visuals().selection.stroke.color);
        painter.circle_stroke(corners[i], FOCUS_RADIUS, ring);
    }
}
