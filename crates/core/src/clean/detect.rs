//! Finds a page's corners in a photo: the largest region of paper, simplified to four corners.

use image::{GrayImage, Luma, RgbaImage};
use imageproc::contours::{BorderType, find_contours};
use imageproc::distance_transform::Norm;
use imageproc::filter::gaussian_blur_f32;
use imageproc::geometry::{approximate_polygon_dp, arc_length, contour_area, convex_hull};
use imageproc::morphology::{Mask, grayscale_close, open};
use imageproc::point::Point;

use super::lighting::{local_brightness, luma, percentile};
use super::{CleanParams, NoCorners};
use crate::geom::{Quad, centroid, order_corners};
use crate::imaging::shrunk_copy;

/// Finds the page (or open notebook) as the largest region of paper, simplified to four corners.
pub fn detect_corners(img: &RgbaImage, params: &CleanParams) -> Result<Quad, NoCorners> {
    let small = shrunk_copy(img, params.detect_side_px);
    let (w, h) = small.dimensions();
    let mask = paper_mask(&small, params);

    let hull = find_contours::<i32>(&mask)
        .into_iter()
        .filter(|c| c.border_type == BorderType::Outer)
        .map(|c| convex_hull(c.points))
        .max_by(|a, b| contour_area(a).total_cmp(&contour_area(b)))
        .filter(|hull| hull.len() >= 3)
        .ok_or(NoCorners::NoRegion)?;
    let fraction = share_on_border(&hull, w, h);
    if fraction > params.max_page_on_border {
        return Err(NoCorners::OnBorder { fraction });
    }
    let corners = four_corners(&hull)?;

    let area_fraction = (contour_area(&corners) / (f64::from(w) * f64::from(h))) as f32;
    if area_fraction < params.min_page_area {
        return Err(NoCorners::TooSmall { area_fraction });
    }
    let corners = enclose(
        order_corners(corners.map(|p| [p.x as f32, p.y as f32])),
        &hull,
    );
    let scale = img.width() as f32 / w as f32;
    Ok(corners.map(|p| p.map(|c| c * scale)))
}

/// Pixels that look like paper: bright and nearly colourless, compared with what's around them.
/// Tables, covers and hands are usually darker or more colourful.
fn paper_mask(img: &RgbaImage, params: &CleanParams) -> GrayImage {
    let paperness = GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let [r, g, b, _] = img.get_pixel(x, y).0.map(i32::from);
        let chroma = r.max(g).max(b) - r.min(g).min(b);
        Luma([(i32::from(luma(img.get_pixel(x, y))) - chroma).clamp(0, 255) as u8])
    });
    let paperness = gaussian_blur_f32(
        &grayscale_close(&paperness, &Mask::square(params.detect_ink_radius)),
        params.detect_blur_sigma,
    );

    let light = local_brightness(
        &paperness,
        params.background_side_px,
        params.detect_light_radius,
    );
    let mut histogram = [0u64; 256];
    for p in paperness.pixels() {
        histogram[usize::from(p[0])] += 1;
    }
    let floor = params.detect_min_brightness * f32::from(percentile(&histogram, 0.99));

    let (w, h) = paperness.dimensions();
    let mask = GrayImage::from_fn(w, h, |x, y| {
        let v = f32::from(paperness.get_pixel(x, y)[0]);
        let local = light.at(x as f32 / w as f32, y as f32 / h as f32)[0];
        let is_paper = v >= floor && v >= params.detect_paper_ratio * local;
        Luma([if is_paper { 255 } else { 0 }])
    });
    open(&mask, Norm::LInf, params.detect_open_radius)
}

/// The share of `hull`'s perimeter that runs along the edge of a `w` x `h` image.
fn share_on_border(hull: &[Point<i32>], w: u32, h: u32) -> f32 {
    let (right, bottom) = (w as i32 - 2, h as i32 - 2);
    let on_same_edge = |a: Point<i32>, b: Point<i32>| {
        (a.x <= 1 && b.x <= 1)
            || (a.y <= 1 && b.y <= 1)
            || (a.x >= right && b.x >= right)
            || (a.y >= bottom && b.y >= bottom)
    };
    let along_border: f64 = (0..hull.len())
        .map(|i| (hull[i], hull[(i + 1) % hull.len()]))
        .filter(|&(a, b)| on_same_edge(a, b))
        .map(|(a, b)| f64::from(a.x - b.x).hypot(f64::from(a.y - b.y)))
        .sum();
    (along_border / arc_length(hull, true).max(1.0)) as f32
}

/// The four vertices of `hull` that enclose the most area. Picks sensible corners even when the
/// page's corners are rounded or the hull starts mid-edge.
fn four_corners(hull: &[Point<i32>]) -> Result<[Point<i32>; 4], NoCorners> {
    // Thin the hull out first so the exhaustive search below stays cheap.
    const MAX_CANDIDATES: usize = 16;
    let mut epsilon = 0.002 * arc_length(hull, true);
    let mut poly = simplify_closed(hull, epsilon);
    while poly.len() > MAX_CANDIDATES {
        epsilon *= 1.5;
        poly = simplify_closed(hull, epsilon);
    }
    if poly.len() < 4 {
        return Err(NoCorners::NotQuad {
            vertices: poly.len(),
        });
    }

    let n = poly.len();
    let mut best = ([poly[0]; 4], 0.0);
    for a in 0..n {
        for b in a + 1..n {
            for c in b + 1..n {
                for d in c + 1..n {
                    let quad = [poly[a], poly[b], poly[c], poly[d]];
                    let area = contour_area(&quad);
                    if area > best.1 {
                        best = (quad, area);
                    }
                }
            }
        }
    }
    Ok(best.0)
}

/// Douglas-Peucker simplification of a closed polygon. `approximate_polygon_dp` measures distance
/// from the line between the first and last points, which is degenerate for a closed curve, so
/// split it at the point farthest from the start and simplify each half as an open curve.
fn simplify_closed(polygon: &[Point<i32>], epsilon: f64) -> Vec<Point<i32>> {
    let start = polygon[0];
    let dist2 = |p: &Point<i32>| (p.x - start.x).pow(2) + (p.y - start.y).pow(2);
    let far = (0..polygon.len())
        .max_by_key(|&i| dist2(&polygon[i]))
        .unwrap_or(0);
    let mut second_half = polygon[far..].to_vec();
    second_half.push(start);

    let mut out = approximate_polygon_dp(&polygon[..=far], epsilon, false);
    out.pop();
    out.extend(approximate_polygon_dp(&second_half, epsilon, false));
    out.pop();
    out
}

/// Pushes each side of `quad` outward, keeping its direction, until the whole of `hull` is inside.
/// Pages curve, so a quad through the corners alone would clip the edges of the page.
fn enclose(quad: Quad, hull: &[Point<i32>]) -> Quad {
    let [cx, cy] = centroid(&quad);
    // Each side as a point on it and its outward unit normal, moved out to the farthest hull point.
    let sides: [([f32; 2], [f32; 2]); 4] = std::array::from_fn(|i| {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(f32::EPSILON);
        let mut n = [(b[1] - a[1]) / len, (a[0] - b[0]) / len];
        if n[0] * (a[0] - cx) + n[1] * (a[1] - cy) < 0.0 {
            n = [-n[0], -n[1]];
        }
        let beyond = hull
            .iter()
            .map(|q| (q.x as f32 - a[0]) * n[0] + (q.y as f32 - a[1]) * n[1])
            .fold(0.0, f32::max);
        ([a[0] + n[0] * beyond, a[1] + n[1] * beyond], n)
    });
    // Corner i is where the sides either side of it meet.
    std::array::from_fn(|i| {
        let ((p, n), (q, m)) = (sides[(i + 3) % 4], sides[i]);
        intersect(p, n, q, m).unwrap_or(quad[i])
    })
}

/// Where the line through `p` with normal `n` meets the line through `q` with normal `m`.
fn intersect(p: [f32; 2], n: [f32; 2], q: [f32; 2], m: [f32; 2]) -> Option<[f32; 2]> {
    // Solve n.x = n.p and m.x = m.q.
    let (c1, c2) = (n[0] * p[0] + n[1] * p[1], m[0] * q[0] + m[1] * q[1]);
    let det = n[0] * m[1] - n[1] * m[0];
    (det.abs() > 1e-6).then(|| [(c1 * m[1] - c2 * n[1]) / det, (n[0] * c2 - m[0] * c1) / det])
}
