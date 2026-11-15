//! The strip of page thumbnails along the bottom of the window: clicking one selects its page,
//! dragging one moves its page. A page whose photo is missing gets a placeholder: see
//! `decisions/2026-11-15-missing-pages-shown.md`.

use std::path::Path;

use eframe::egui;
use egui::accesskit::Role;
use egui::{Color32, Margin, Rect, Sense, Stroke, StrokeKind, TextureHandle, Vec2, WidgetInfo};
use notebook_core::page::Pages;

use crate::t;
use crate::ui::icons;

/// Display size of the thumbnails in the bottom strip, in egui points (physical pixels =
/// points x `pixels_per_point`). Keep at most `THUMBNAIL_SIDE_PX` / 2 so 2x displays stay sharp.
const THUMB_SIZE_PT: f32 = 96.0;

/// Corner radius of the thumbnails, in points. The selected page's frame follows the same curve.
const THUMB_CORNER_PT: f32 = 6.0;

/// Width of the frame around the selected page's thumbnail, and of the drop marker when dragging
/// one, in points.
const ACTIVE_FRAME_PT: f32 = 3.0;

/// Space between thumbnails, in points. The drop marker goes in the middle, so it needs room to
/// stay clear of the selected page's frame.
const THUMB_GAP_PT: f32 = 16.0;

/// Space at either end of the strip, in points, so the frame and the drop marker aren't clipped
/// there. Must fit half a gap plus the marker.
const STRIP_END_PT: i8 = 12;

/// How close to the strip's edge, in points, a dragged thumbnail starts scrolling it, and how far
/// it scrolls per frame.
const DRAG_SCROLL_EDGE_PT: f32 = 32.0;
const DRAG_SCROLL_STEP_PT: f32 = 8.0;

/// Thumbnails can be clicked and dragged, but don't take the keyboard focus: ⌘←/→ go through the
/// pages. See `decisions/2026-10-31-keyboard-focus.md`.
const THUMB_SENSE: Sense = Sense::CLICK.union(Sense::DRAG);

/// What the user did in the strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Clicked the thumbnail of the page with this name.
    Select(String),
    /// Dropped page `from`'s thumbnail so the page should end up at index `to`.
    Move { from: usize, to: usize },
}

/// Drag-and-drop payload: the index of the page whose thumbnail is being dragged.
struct DraggedPage(usize);

/// The strip's state between frames.
#[derive(Default)]
pub struct Thumbnails {
    /// The page whose thumbnail the strip last scrolled into view, and where it was. Scrolls
    /// again only when either changes, so it doesn't fight the user scrolling the strip.
    scrolled_to: Option<(String, usize)>,
    /// Where a dragged thumbnail would go if dropped now: the index of the gap before it, with
    /// `pages.len()` meaning the end.
    drop_slot: Option<usize>,
}

impl Thumbnails {
    /// The strip, scrolling sideways, of `pages` with their `textures` by name, page `selected`
    /// framed. A page without a texture is still loading, and gets a spinner, unless its photo is
    /// missing. Returns what the user did, if anything.
    pub fn show<'a>(
        &mut self,
        ui: &mut egui::Ui,
        pages: &Pages,
        textures: impl Fn(&str) -> Option<&'a TextureHandle>,
        selected: usize,
    ) -> Option<Action> {
        egui::ScrollArea::horizontal()
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(Margin::symmetric(STRIP_END_PT, 0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = THUMB_GAP_PT;
                        ui.horizontal(|ui| self.thumbnails(ui, pages, textures, selected))
                            .inner
                    })
                    .inner
            })
            .inner
    }

    /// The pages' thumbnails in a row, the `selected` one framed. Returns what the user did.
    fn thumbnails<'a>(
        &mut self,
        ui: &mut egui::Ui,
        pages: &Pages,
        textures: impl Fn(&str) -> Option<&'a TextureHandle>,
        selected: usize,
    ) -> Option<Action> {
        let mut action = None;
        let dragged = egui::DragAndDrop::payload::<DraggedPage>(ui.ctx()).map(|p| p.0);
        let selection = ui.visuals().selection.stroke.color;
        let mut rects = Vec::with_capacity(pages.len());
        for (i, page) in pages.iter().enumerate() {
            let tint = if dragged == Some(i) {
                Color32::from_white_alpha(96)
            } else {
                Color32::WHITE
            };
            let response = match (page.missing_photo(), textures(&page.name)) {
                (Some(why), _) => {
                    let path = page.path.as_deref().unwrap_or(Path::new(""));
                    let why = crate::i18n::missing_photo(why, path);
                    missing_tile(ui, tint).on_hover_text(t!(
                        "missing-thumbnail",
                        name = page.name.as_str(),
                        why = why
                    ))
                }
                (None, Some(texture)) => ui
                    .add(
                        egui::Image::new(texture)
                            .max_size(Vec2::splat(THUMB_SIZE_PT))
                            .tint(tint)
                            .corner_radius(THUMB_CORNER_PT)
                            .alt_text(&page.name)
                            .sense(THUMB_SENSE),
                    )
                    .on_hover_text(&page.name),
                (None, None) => loading_tile(ui, tint).on_hover_text(&page.name),
            };
            let active = i == selected;
            response.widget_info(|| WidgetInfo::selected(Role::Image, true, active, &page.name));
            response.dnd_set_drag_payload(DraggedPage(i));
            if active {
                // An outside stroke adds its width to the radius, so this stays concentric.
                ui.painter().rect_stroke(
                    response.rect,
                    THUMB_CORNER_PT,
                    Stroke::new(ACTIVE_FRAME_PT, selection),
                    StrokeKind::Outside,
                );
                if self
                    .scrolled_to
                    .as_ref()
                    .is_none_or(|(name, at)| *name != page.name || *at != i)
                {
                    response.scroll_to_me(None);
                    self.scrolled_to = Some((page.name.clone(), i));
                }
            }
            if response.clicked() {
                action = Some(Action::Select(page.name.clone()));
            }
            rects.push(response.rect);
        }

        let Some(from) = dragged else {
            self.drop_slot = None;
            return action;
        };
        self.drag(ui, from, &rects, selection).or(action)
    }

    /// While page `from`'s thumbnail is dragged over the strip, whose thumbnails are at `rects`,
    /// marks where it would go in `colour` and scrolls near the edges. Returns the move once it's
    /// dropped.
    fn drag(
        &mut self,
        ui: &mut egui::Ui,
        from: usize,
        rects: &[Rect],
        colour: Color32,
    ) -> Option<Action> {
        if let Some(x) = ui.input(|i| i.pointer.latest_pos()).map(|pos| pos.x) {
            self.drop_slot = Some(rects.iter().filter(|r| r.center().x < x).count());
            // Near an edge, scroll towards it while there's more strip beyond it.
            let visible = ui.clip_rect();
            let (first, last) = (rects[0], rects[rects.len() - 1]);
            let scroll = if x < visible.left() + DRAG_SCROLL_EDGE_PT
                && first.left() < visible.left()
            {
                DRAG_SCROLL_STEP_PT
            } else if x > visible.right() - DRAG_SCROLL_EDGE_PT && last.right() > visible.right() {
                -DRAG_SCROLL_STEP_PT
            } else {
                0.0
            };
            if scroll != 0.0 {
                ui.scroll_with_delta(Vec2::X * scroll);
                ui.ctx().request_repaint();
            }
        }
        let slot = self.drop_slot?;
        // Dropping either side of the dragged page leaves it where it is.
        let to = if slot > from { slot - 1 } else { slot };
        if to != from {
            let gap = ui.spacing().item_spacing.x / 2.0;
            let x = match slot.checked_sub(1).and_then(|i| rects.get(i)) {
                Some(before) => before.right() + gap,
                None => rects[0].left() - gap,
            };
            let ys = rects
                .iter()
                .fold(rects[0].y_range(), |ys, r| ys.union(r.y_range()));
            ui.painter()
                .vline(x, ys, Stroke::new(ACTIVE_FRAME_PT, colour));
        }
        if ui.input(|i| i.pointer.any_released())
            && egui::DragAndDrop::take_payload::<DraggedPage>(ui.ctx()).is_some()
        {
            self.drop_slot = None;
            return Some(Action::Move { from, to });
        }
        None
    }
}

/// Stands in for the thumbnail of a page that's loading.
fn loading_tile(ui: &mut egui::Ui, tint: Color32) -> egui::Response {
    let response = empty_tile(ui, tint);
    let spinner = Rect::from_center_size(response.rect.center(), Vec2::splat(THUMB_SIZE_PT / 4.0));
    egui::Spinner::new().paint_at(ui, spinner);
    response
}

/// Stands in for the thumbnail of a page whose photo is missing.
fn missing_tile(ui: &mut egui::Ui, tint: Color32) -> egui::Response {
    let response = empty_tile(ui, tint);
    let colour = ui
        .visuals()
        .warn_fg_color
        .gamma_multiply(tint.a() as f32 / 255.0);
    let icon = Rect::from_center_size(response.rect.center(), Vec2::splat(THUMB_SIZE_PT / 3.0));
    icons::paint_warning(ui.painter(), icon, colour);
    response
}

/// An empty tile the size of a portrait thumbnail, faded by `tint` while it's dragged.
fn empty_tile(ui: &mut egui::Ui, tint: Color32) -> egui::Response {
    let size = Vec2::new(THUMB_SIZE_PT * 3.0 / 4.0, THUMB_SIZE_PT);
    let (rect, response) = ui.allocate_exact_size(size, THUMB_SENSE);
    let fill = ui
        .visuals()
        .extreme_bg_color
        .gamma_multiply(tint.a() as f32 / 255.0);
    ui.painter().rect_filled(rect, THUMB_CORNER_PT, fill);
    response
}
