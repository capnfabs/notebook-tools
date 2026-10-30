//! Maps a transcript's boxes from the crop of the photo Claude was sent to another crop of the same
//! photo, so they follow the page when the user moves its corners. Both crops straighten the same
//! flat page, so the mapping is exact up to rounding; a box only stops being upright when the new
//! crop is turned relative to the old one. Also tells when the new crop loses transcribed lines or
//! takes in much of the page Claude never saw.
//!
//! See `decisions/2026-10-30-boxes-follow-corners.md`.

use imageproc::geometric_transformations::Projection;

use crate::clean::{Crop, Quad};
use crate::transcript::{Bbox, Transcript};

/// Share of a line's box that must fall outside a crop, beyond what fell outside the crop it was
/// transcribed from, for it to count as cut off.
pub const CUT_OFF_SHARE: f32 = 0.5;

/// Largest share of a crop that may lie outside the crop its transcript was made from before the
/// page needs transcribing again.
pub const MAX_UNTRANSCRIBED: f32 = 0.1;

/// Maps points from one [`Crop`] of a photo to another.
#[derive(Clone, Copy, Debug)]
pub struct Reframe {
    /// `None` when the crops are the same.
    projection: Option<Projection>,
}

impl Reframe {
    pub fn new(from: Crop, to: Crop) -> Self {
        if from == to {
            return Self { projection: None };
        }
        let to_photo =
            Projection::from_control_points(rect_corners(from.size), tuples(from.corners));
        let from_photo = Projection::from_control_points(tuples(to.corners), rect_corners(to.size));
        let projection = match (to_photo, from_photo) {
            (Some(to_photo), Some(from_photo)) => to_photo.and_then(from_photo),
            // Corners too degenerate to straighten; scaling is the best guess.
            _ => {
                let scale = |to: u32, from: u32| to as f32 / from.max(1) as f32;
                Projection::scale(scale(to.size.0, from.size.0), scale(to.size.1, from.size.1))
            }
        };
        Self {
            projection: Some(projection),
        }
    }

    /// Whether it leaves points where they are.
    pub fn is_identity(&self) -> bool {
        self.projection.is_none()
    }

    pub fn point(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        match &self.projection {
            Some(projection) => {
                let (x, y) = projection * &(x, y);
                [x, y]
            }
            None => [x, y],
        }
    }

    /// `bbox`'s corners, mapped, in [`Quad`] order. No longer upright if the crops are turned
    /// relative to each other.
    pub fn quad(&self, bbox: Bbox) -> Quad {
        bbox_corners(bbox).map(|p| self.point(p))
    }

    /// The upright box around `bbox`, mapped.
    pub fn bbox(&self, bbox: Bbox) -> Bbox {
        bounds(&self.quad(bbox))
    }
}

/// How a transcript covers a crop other than the one it was made from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// Lines with more than [`CUT_OFF_SHARE`] of their box newly outside the crop.
    pub lines_cut_off: usize,
    /// Share of the crop outside the one the transcript was made from: page Claude never saw.
    pub untranscribed: f32,
}

impl Fit {
    /// More of the page is outside the transcript's crop than [`MAX_UNTRANSCRIBED`].
    pub fn missed_page(&self) -> bool {
        self.untranscribed > MAX_UNTRANSCRIBED
    }
}

impl Transcript {
    /// A copy with its boxes mapped onto `to`.
    pub fn reframed(&self, to: Crop) -> Transcript {
        let reframe = Reframe::new(self.crop, to);
        let mut transcript = self.clone();
        let boxes = transcript.lines.iter_mut().map(|l| &mut l.bbox);
        for bbox in boxes.chain(transcript.uncertain.iter_mut().map(|u| &mut u.bbox)) {
            *bbox = bbox.map(|b| reframe.bbox(b));
        }
        transcript.crop = to;
        transcript
    }

    /// How well it covers `to`.
    pub fn fit(&self, to: Crop) -> Fit {
        let reframe = Reframe::new(self.crop, to);
        if reframe.is_identity() {
            return Fit {
                lines_cut_off: 0,
                untranscribed: 0.0,
            };
        }
        let lines_cut_off = self
            .lines
            .iter()
            .filter_map(|line| line.bbox)
            .filter(|&bbox| {
                let before = share_outside(&bbox_corners(bbox), self.crop.size);
                let after = share_outside(&reframe.quad(bbox), to.size);
                after - before > CUT_OFF_SHARE
            })
            .count();
        let seen = reframe.quad(rect_bbox(self.crop.size));
        let whole = to.size.0 as f32 * to.size.1 as f32;
        Fit {
            lines_cut_off,
            untranscribed: if whole > 0.0 {
                (1.0 - inside_area(&seen, to.size) / whole).clamp(0.0, 1.0)
            } else {
                0.0
            },
        }
    }
}

fn tuples(quad: Quad) -> [(f32, f32); 4] {
    quad.map(|[x, y]| (x, y))
}

fn rect_corners((w, h): (u32, u32)) -> [(f32, f32); 4] {
    let (w, h) = (w as f32, h as f32);
    [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
}

fn rect_bbox((w, h): (u32, u32)) -> Bbox {
    [0.0, 0.0, w as f32, h as f32]
}

fn bbox_corners([x0, y0, x1, y1]: Bbox) -> Quad {
    [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn bounds(points: &[[f32; 2]]) -> Bbox {
    points.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |[x0, y0, x1, y1], &[x, y]| [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
    )
}

/// Share of `polygon`'s area outside a `size` image. 0 for a polygon with no area.
fn share_outside(polygon: &[[f32; 2]], size: (u32, u32)) -> f32 {
    let total = area(polygon);
    if !total.is_finite() || total <= f32::EPSILON {
        return 0.0;
    }
    (1.0 - inside_area(polygon, size) / total).clamp(0.0, 1.0)
}

/// Area of convex `polygon` inside a `size` image.
fn inside_area(polygon: &[[f32; 2]], (w, h): (u32, u32)) -> f32 {
    let (w, h) = (w as f32, h as f32);
    // Sutherland–Hodgman against each edge of the image in turn: an axis, which way is inside,
    // and where the edge is along it.
    let edges = [(0, 1.0, 0.0), (1, 1.0, 0.0), (0, -1.0, w), (1, -1.0, h)];
    let mut clipped = polygon.to_vec();
    for (axis, sign, at) in edges {
        let inside = |p: [f32; 2]| sign * (p[axis] - at);
        let mut kept = Vec::with_capacity(clipped.len() + 1);
        for (i, &p) in clipped.iter().enumerate() {
            let q = clipped[(i + 1) % clipped.len()];
            let (dp, dq) = (inside(p), inside(q));
            if dp >= 0.0 {
                kept.push(p);
            }
            if (dp >= 0.0) != (dq >= 0.0) {
                let t = dp / (dp - dq);
                kept.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
            }
        }
        clipped = kept;
        if clipped.is_empty() {
            return 0.0;
        }
    }
    area(&clipped)
}

/// Shoelace area of `polygon`, whichever way round it goes.
fn area(polygon: &[[f32; 2]]) -> f32 {
    let twice: f32 = (0..polygon.len())
        .map(|i| {
            let ([x0, y0], [x1, y1]) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            x0 * y1 - x1 * y0
        })
        .sum();
    twice.abs() / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Line, Uncertain};

    /// A crop of a 1000x1000 photo, cut at `corners` and sent at `size`.
    fn crop(corners: Quad, size: (u32, u32)) -> Crop {
        Crop { corners, size }
    }

    /// The upright rectangle from `(x0, y0)` to `(x1, y1)` in the photo, sent at its own size.
    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Crop {
        crop(
            [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
            ((x1 - x0) as u32, (y1 - y0) as u32),
        )
    }

    fn assert_near(actual: &[[f32; 2]], expected: &[[f32; 2]]) {
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                (a[0] - e[0]).abs() < 0.05 && (a[1] - e[1]).abs() < 0.05,
                "{actual:?} vs {expected:?}"
            );
        }
    }

    fn line(bbox: Bbox) -> Line {
        Line {
            text_md: "text".to_owned(),
            bbox: Some(bbox),
        }
    }

    /// Ten 50px lines across a crop of `from`, one every 100px.
    fn lined(from: Crop) -> Transcript {
        let lines = (0..10)
            .map(|i| line([10.0, i as f32 * 100.0, 790.0, i as f32 * 100.0 + 50.0]))
            .collect();
        Transcript {
            crop: from,
            ..Transcript::for_test(lines, vec![])
        }
    }

    #[test]
    fn the_same_crop_maps_nothing() {
        let a = rect(100.0, 100.0, 900.0, 1100.0);
        let reframe = Reframe::new(a, a);
        assert!(reframe.is_identity());
        assert_eq!(reframe.bbox([1.0, 2.0, 3.0, 4.0]), [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn a_moved_and_scaled_crop_moves_and_scales_boxes() {
        let from = rect(100.0, 100.0, 900.0, 1100.0);
        // Starts 50px further in, and was sent at half size.
        let to = crop(rect(150.0, 150.0, 950.0, 1150.0).corners, (400, 500));
        let reframe = Reframe::new(from, to);
        assert!(!reframe.is_identity());
        let [x0, y0, x1, y1] = reframe.bbox([50.0, 50.0, 250.0, 150.0]);
        assert_near(&[[x0, y0], [x1, y1]], &[[0.0, 0.0], [100.0, 50.0]]);
    }

    #[test]
    fn perspective_maps_corners_onto_corners() {
        let from = crop(
            [
                [200.0, 120.0],
                [700.0, 90.0],
                [740.0, 800.0],
                [150.0, 760.0],
            ],
            (545, 676),
        );
        let to = rect(0.0, 0.0, 1000.0, 1000.0);
        let reframe = Reframe::new(from, to);
        assert_near(&reframe.quad([0.0, 0.0, 545.0, 676.0]), &from.corners);
    }

    #[test]
    fn a_turned_crop_slants_boxes() {
        let from = rect(0.0, 0.0, 1000.0, 1000.0);
        // The same square, turned a little about its centre.
        let turn = 0.05_f32;
        let corners = from.corners.map(|[x, y]| {
            let (dx, dy) = (x - 500.0, y - 500.0);
            [
                500.0 + dx * turn.cos() - dy * turn.sin(),
                500.0 + dx * turn.sin() + dy * turn.cos(),
            ]
        });
        let reframe = Reframe::new(from, crop(corners, from.size));
        let quad = reframe.quad([100.0, 100.0, 300.0, 150.0]);
        assert!(
            (quad[0][1] - quad[1][1]).abs() > 1.0,
            "the top is no longer level: {quad:?}"
        );
        let [x0, y0, x1, y1] = reframe.bbox([100.0, 100.0, 300.0, 150.0]);
        for [x, y] in quad {
            assert!((x0..=x1).contains(&x) && (y0..=y1).contains(&y));
        }
    }

    #[test]
    fn reframing_maps_every_box() {
        let from = rect(0.0, 0.0, 800.0, 1000.0);
        let to = rect(0.0, 100.0, 800.0, 1100.0);
        let t = Transcript {
            uncertain: vec![Uncertain {
                line: 0,
                text: "text".to_owned(),
                bbox: Some([10.0, 150.0, 60.0, 200.0]),
                alternatives: vec![],
                resolved: false,
            }],
            ..lined(from)
        };
        let moved = t.reframed(to);
        assert_eq!(moved.crop, to);
        assert_eq!(moved.lines[1].bbox, Some([10.0, 0.0, 790.0, 50.0]));
        assert_eq!(moved.uncertain[0].bbox, Some([10.0, 50.0, 60.0, 100.0]));
        assert_eq!(moved.markdown(), t.markdown());
    }

    #[test]
    fn a_small_adjustment_fits() {
        let from = rect(0.0, 0.0, 800.0, 1000.0);
        let fit = lined(from).fit(rect(5.0, -10.0, 805.0, 990.0));
        assert_eq!(fit.lines_cut_off, 0);
        assert!(!fit.missed_page(), "{fit:?}");
        assert_eq!(
            lined(from).fit(from),
            Fit {
                lines_cut_off: 0,
                untranscribed: 0.0
            }
        );
    }

    #[test]
    fn cropping_off_the_bottom_cuts_lines_off() {
        let from = rect(0.0, 0.0, 800.0, 1000.0);
        // Lines from 700px down are mostly gone; the one at 600-650 stays.
        let fit = lined(from).fit(rect(0.0, 0.0, 800.0, 670.0));
        assert_eq!(fit.lines_cut_off, 3);
        assert!(!fit.missed_page());
    }

    #[test]
    fn boxes_already_over_the_edge_dont_count() {
        // Claude's boxes run past the image a little: a quarter of this one is outside already.
        let from = rect(0.0, 0.0, 800.0, 1000.0);
        let t = Transcript {
            lines: vec![line([10.0, 970.0, 790.0, 1010.0])],
            ..lined(from)
        };
        assert_eq!(t.fit(rect(0.0, 0.0, 800.0, 995.0)).lines_cut_off, 0);
        assert_eq!(t.fit(rect(0.0, 0.0, 800.0, 975.0)).lines_cut_off, 1);
    }

    #[test]
    fn growing_the_crop_misses_page() {
        let from = rect(100.0, 100.0, 900.0, 1100.0);
        let fit = lined(from).fit(rect(0.0, 0.0, 1000.0, 1200.0));
        assert_eq!(fit.lines_cut_off, 0);
        // 800x1000 of 1000x1200 was seen.
        assert!((fit.untranscribed - 1.0 / 3.0).abs() < 0.01, "{fit:?}");
        assert!(fit.missed_page());
    }

    #[test]
    fn clipping_measures_area_inside() {
        let square = [[-50.0, -50.0], [50.0, -50.0], [50.0, 50.0], [-50.0, 50.0]];
        assert_eq!(inside_area(&square, (100, 100)), 2500.0);
        assert_eq!(share_outside(&square, (100, 100)), 0.75);
        let diamond = [[50.0, 0.0], [100.0, 50.0], [50.0, 100.0], [0.0, 50.0]];
        assert_eq!(inside_area(&diamond, (100, 100)), 5000.0);
        assert_eq!(inside_area(&square, (0, 0)), 0.0);
    }
}
