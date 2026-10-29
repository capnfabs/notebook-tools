//! Turns a photo of a notebook page (or spread) into something close to a scan: find the page
//! outline, straighten and crop to it, then flatten the lighting to white paper.
//!
//! See `decisions/2026-10-23-cleaning-pipeline.md` and `decisions/2026-10-29-manual-corners.md`.

use std::fmt;

use image::{GrayImage, Luma, Rgba, RgbaImage, imageops};
use imageproc::contours::{BorderType, find_contours};
use imageproc::distance_transform::Norm;
use imageproc::filter::gaussian_blur_f32;
use imageproc::geometric_transformations::{Border, Interpolation, Projection, warp_into};
use imageproc::geometry::{approximate_polygon_dp, arc_length, contour_area, convex_hull};
use imageproc::morphology::{Mask, grayscale_close, grayscale_dilate, open};
use imageproc::point::Point;
use rayon::prelude::*;

use crate::page::{fitted_size, shrunk_copy};

/// Corners of a page outline in source pixels: top-left, top-right, bottom-right, bottom-left.
pub type Quad = [[f32; 2]; 4];

/// Sharpest corner an outline may have, in degrees; its flattest is 180° less this. Past these,
/// straightening stretches part of the page towards infinity, and a dent (over 180°) isn't a page.
pub const MIN_CORNER_DEG: f32 = 15.0;

/// Tuning knobs for [`clean`]. Pixel sizes are measured on fixed-size working copies, so results
/// don't depend on the photo's resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct CleanParams {
    /// Outline detection runs on a copy scaled down to this long side.
    pub detect_side_px: u32,
    /// Radius of the morphological close that erases ink before looking for paper, in detection
    /// pixels.
    pub detect_ink_radius: u8,
    /// Blur after erasing ink, in detection pixels.
    pub detect_blur_sigma: f32,
    /// Radius of the local brightness estimate, in pixels at `background_side_px`. Paper is
    /// judged against nearby paper, so a page partly in shadow is still found.
    pub detect_light_radius: u8,
    /// Paper is at least this fraction of the local brightness...
    pub detect_paper_ratio: f32,
    /// ...and at least this fraction of the brightest paper in the photo, which rules out
    /// stretches of table far from any paper.
    pub detect_min_brightness: f32,
    /// Opening radius in detection pixels. Cuts thin bridges between the page and bright clutter
    /// next to it.
    pub detect_open_radius: u8,
    /// Smallest outline worth trusting, as a fraction of the image area.
    pub min_outline_area: f32,
    /// Largest share of the outline allowed to run along the image border. More means the page
    /// doesn't fit in the photo or isn't distinguishable from the table.
    pub max_outline_on_border: f32,
    /// Longest side of the cleaned image.
    pub max_output_side: u32,
    /// The lighting estimate is made on a copy scaled down to this long side.
    pub background_side_px: u32,
    /// Radius of the max filter that removes ink from the lighting estimate, in background pixels.
    pub background_radius: u8,
    /// Fraction of pixels, darkest first, that end up pure black.
    pub black_percentile: f32,
    /// Brightness percentile that ends up pure white. Paper is most of a page, so the median works.
    pub white_percentile: f32,
}

impl Default for CleanParams {
    fn default() -> Self {
        Self {
            detect_side_px: 512,
            detect_ink_radius: 6,
            detect_blur_sigma: 1.5,
            detect_light_radius: 8,
            detect_paper_ratio: 0.75,
            detect_min_brightness: 0.35,
            detect_open_radius: 2,
            min_outline_area: 0.2,
            max_outline_on_border: 0.05,
            // Claude's long-side limit. Transcription also caps the pixel count; see
            // `ocr::MAX_IMAGE_PIXELS` and
            // https://platform.claude.com/docs/en/build-with-claude/vision-coordinates#how-claude-resizes-and-pads-images
            max_output_side: 2576,
            background_side_px: 128,
            background_radius: 3,
            black_percentile: 0.005,
            white_percentile: 0.5,
        }
    }
}

pub struct Cleaned {
    pub image: RgbaImage,
    pub report: CleanReport,
}

/// What [`clean`] found and did, for debugging and tuning.
#[derive(Clone, Debug, PartialEq)]
pub struct CleanReport {
    pub source_size: (u32, u32),
    /// What outline detection found, whether or not it was used.
    pub detected: Result<Quad, NoOutline>,
    /// The corners the page was straightened and cropped to: the user's if they placed some, else
    /// the detected ones. `None` if there were neither, or they made no sense.
    pub outline: Option<Quad>,
    pub lighting: LightingReport,
    pub output_size: (u32, u32),
}

/// Why no page outline was found. The image is then cleaned without straightening or cropping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoOutline {
    NoRegion,
    TooSmall { area_fraction: f32 },
    OnBorder { fraction: f32 },
    NotQuad { vertices: usize },
}

impl fmt::Display for NoOutline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRegion => write!(f, "no bright region"),
            Self::TooSmall { area_fraction } => write!(
                f,
                "brightest region covers only {:.0}% of the image",
                area_fraction * 100.0
            ),
            Self::OnBorder { fraction } => write!(
                f,
                "{:.0}% of the outline runs along the photo's edge",
                fraction * 100.0
            ),
            Self::NotQuad { vertices } => write!(f, "region has only {vertices} corners"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightingReport {
    /// Average paper colour before correction; white balancing maps it to white.
    pub paper_rgb: [u8; 3],
    /// Brightness levels stretched to black and white after flattening.
    pub black_point: u8,
    pub white_point: u8,
}

/// [`crop_to_page`], then flattens the lighting.
pub fn clean(img: &RgbaImage, params: &CleanParams, corners: Option<Quad>) -> Cleaned {
    let Cropped {
        mut image,
        detected,
        outline,
    } = crop_to_page(img, params, corners);
    let lighting = fix_lighting(&mut image, params);
    Cleaned {
        report: CleanReport {
            source_size: img.dimensions(),
            detected,
            outline,
            lighting,
            output_size: image.dimensions(),
        },
        image,
    }
}

/// A photo cropped to its page by [`crop_to_page`].
pub struct Cropped {
    pub image: RgbaImage,
    /// What outline detection found, whether or not it was used.
    pub detected: Result<Quad, NoOutline>,
    /// The corners `image` was cropped to, if any.
    pub outline: Option<Quad>,
}

/// The page straightened and cropped to `corners`, else to its detected outline, no longer than
/// `params.max_output_side`, with the lighting left alone. Without either, the whole photo scaled
/// down.
pub fn crop_to_page(img: &RgbaImage, params: &CleanParams, corners: Option<Quad>) -> Cropped {
    let detected = detect_outline(img, params);
    let quad = corners.or_else(|| detected.as_ref().ok().copied());
    let straightened =
        quad.and_then(|quad| Some((quad, straighten(img, &quad, params.max_output_side)?)));
    let (outline, image) = match straightened {
        Some((quad, image)) => (Some(quad), image),
        None => (None, shrunk_copy(img, params.max_output_side)),
    };
    Cropped {
        image,
        detected,
        outline,
    }
}

/// `img`'s own corners, in [`Quad`] order: cropping to them only scales it down.
pub fn photo_corners(width: u32, height: u32) -> Quad {
    let (w, h) = (width as f32, height as f32);
    [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]
}

/// Finds the page (or open notebook) as the largest region of paper, simplified to four corners.
pub fn detect_outline(img: &RgbaImage, params: &CleanParams) -> Result<Quad, NoOutline> {
    let small = shrunk_copy(img, params.detect_side_px);
    let (w, h) = small.dimensions();
    let mask = paper_mask(&small, params);

    let hull = find_contours::<i32>(&mask)
        .into_iter()
        .filter(|c| c.border_type == BorderType::Outer)
        .map(|c| convex_hull(c.points))
        .max_by(|a, b| contour_area(a).total_cmp(&contour_area(b)))
        .filter(|hull| hull.len() >= 3)
        .ok_or(NoOutline::NoRegion)?;
    let fraction = share_on_border(&hull, w, h);
    if fraction > params.max_outline_on_border {
        return Err(NoOutline::OnBorder { fraction });
    }
    let corners = four_corners(&hull)?;

    let area_fraction = (contour_area(&corners) / (f64::from(w) * f64::from(h))) as f32;
    if area_fraction < params.min_outline_area {
        return Err(NoOutline::TooSmall { area_fraction });
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
fn four_corners(hull: &[Point<i32>]) -> Result<[Point<i32>; 4], NoOutline> {
    // Thin the hull out first so the exhaustive search below stays cheap.
    const MAX_CANDIDATES: usize = 16;
    let mut epsilon = 0.002 * arc_length(hull, true);
    let mut poly = simplify_closed(hull, epsilon);
    while poly.len() > MAX_CANDIDATES {
        epsilon *= 1.5;
        poly = simplify_closed(hull, epsilon);
    }
    if poly.len() < 4 {
        return Err(NoOutline::NotQuad {
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
    let cx = quad.iter().map(|p| p[0]).sum::<f32>() / 4.0;
    let cy = quad.iter().map(|p| p[1]).sum::<f32>() / 4.0;
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

/// Orders corners top-left, top-right, bottom-right, bottom-left. Sorted by angle around their
/// centre, so the sides between them never cross, whatever order they came in.
pub fn order_corners(mut corners: Quad) -> Quad {
    let cx = corners.iter().map(|p| p[0]).sum::<f32>() / 4.0;
    let cy = corners.iter().map(|p| p[1]).sum::<f32>() / 4.0;
    // With y pointing down, increasing angle runs clockwise on screen.
    let angle = |p: &[f32; 2]| (p[1] - cy).atan2(p[0] - cx);
    corners.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
    let top_left = (0..4)
        .min_by(|&i, &j| {
            let sum = |p: [f32; 2]| p[0] + p[1];
            sum(corners[i]).total_cmp(&sum(corners[j]))
        })
        .unwrap_or(0);
    corners.rotate_left(top_left);
    corners
}

/// Whether `quad`, in [`Quad`] order, can be straightened: it's convex, clockwise on screen, and
/// every corner is between [`MIN_CORNER_DEG`] and 180° less that.
pub fn is_page_shaped(quad: &Quad) -> bool {
    let (min, max) = (
        MIN_CORNER_DEG.to_radians(),
        (180.0 - MIN_CORNER_DEG).to_radians(),
    );
    (0..4).all(|i| {
        let [px, py] = quad[(i + 3) % 4];
        let [x, y] = quad[i];
        let [nx, ny] = quad[(i + 1) % 4];
        let (a, b) = ([px - x, py - y], [nx - x, ny - y]);
        // With y pointing down, the next corner is clockwise of the previous one: a right turn.
        let cross = a[0] * b[1] - a[1] * b[0];
        let angle = cross.abs().atan2(a[0] * b[0] + a[1] * b[1]);
        cross < 0.0 && (min..=max).contains(&angle)
    })
}

/// Maps the `quad` region of `img` to an upright rectangle no longer than `max_side`.
/// Returns `None` if the quad isn't [`is_page_shaped`].
fn straighten(img: &RgbaImage, quad: &Quad, max_side: u32) -> Option<RgbaImage> {
    if !is_page_shaped(quad) {
        return None;
    }
    let dist = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let [tl, tr, br, bl] = *quad;
    let w = (dist(tl, tr) + dist(bl, br)) / 2.0;
    let h = (dist(tl, bl) + dist(tr, br)) / 2.0;
    let scale = (max_side as f32 / w.max(h)).min(1.0);

    // Shrink the source first so the warp samples at about 1:1; bilinear sampling alone aliases
    // when shrinking.
    let src_side = (img.width().max(img.height()) as f32 * scale).ceil() as u32;
    let src = shrunk_copy(img, src_side);
    let s = src.width() as f32 / img.width() as f32;

    let (out_w, out_h) = ((w * scale).round().max(1.0), (h * scale).round().max(1.0));
    let from = quad.map(|[x, y]| (x * s, y * s));
    let to = [(0.0, 0.0), (out_w, 0.0), (out_w, out_h), (0.0, out_h)];
    let projection = Projection::from_control_points(from, to)?;
    let mut out = RgbaImage::new(out_w as u32, out_h as u32);
    warp_into(
        &src,
        projection,
        Interpolation::Bilinear,
        Border::Replicate,
        &mut out,
    );
    Some(out)
}

/// Divides out the paper colour and uneven lighting, then stretches brightness so paper is white
/// and ink is dark. Keeps ink colours.
fn fix_lighting(img: &mut RgbaImage, params: &CleanParams) -> LightingReport {
    let background = Background::estimate(img, params);
    let (w, h) = img.dimensions();
    for_each_row(img, |y, row| {
        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let bg = background.at(x as f32 / w as f32, y as f32 / h as f32);
            for c in 0..3 {
                px[c] = (f32::from(px[c]) / bg[c].max(1.0) * 255.0).min(255.0) as u8;
            }
        }
    });

    let mut histogram = [0u64; 256];
    for px in img.pixels() {
        histogram[usize::from(luma(px))] += 1;
    }
    let white = percentile(&histogram, params.white_percentile).max(1);
    let black = percentile(&histogram, params.black_percentile).min(white - 1);
    let (lo, range) = (f32::from(black), f32::from(white) - f32::from(black));
    for_each_row(img, |_, row| {
        for px in row.as_chunks_mut::<4>().0 {
            for v in &mut px[..3] {
                *v = ((f32::from(*v) - lo) / range * 255.0).clamp(0.0, 255.0) as u8;
            }
        }
    });

    LightingReport {
        paper_rgb: background.mean().map(|c| c.round() as u8),
        black_point: black,
        white_point: white,
    }
}

/// Brightness of the surroundings across an image, at low resolution: a max filter (which erases
/// ink, since ink is darker than paper in every channel) followed by a blur. One value per channel.
struct Background<const N: usize> {
    w: u32,
    h: u32,
    values: Vec<[f32; N]>,
}

/// [`Background`] of one channel.
fn local_brightness(img: &GrayImage, side_px: u32, radius: u8) -> Background<1> {
    let (w, h) = fitted_size(img.width(), img.height(), side_px);
    Background::from_channels([imageops::thumbnail(img, w, h)], radius)
}

impl Background<3> {
    fn estimate(img: &RgbaImage, params: &CleanParams) -> Self {
        let small = shrunk_copy(img, params.background_side_px);
        let (w, h) = small.dimensions();
        let channels = std::array::from_fn(|c| {
            GrayImage::from_fn(w, h, |x, y| Luma([small.get_pixel(x, y)[c]]))
        });
        Self::from_channels(channels, params.background_radius)
    }
}

impl<const N: usize> Background<N> {
    fn from_channels(channels: [GrayImage; N], radius: u8) -> Self {
        let (w, h) = channels[0].dimensions();
        let mask = Mask::square(radius);
        let sigma = f32::from(radius).max(1.0);
        let channels = channels.map(|ch| gaussian_blur_f32(&grayscale_dilate(&ch, &mask), sigma));
        let values = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                channels
                    .each_ref()
                    .map(|ch| f32::from(ch.get_pixel(x, y)[0]))
            })
            .collect();
        Self { w, h, values }
    }

    /// Bilinearly interpolated colour at `(u, v)`, each in 0..1 across the image.
    fn at(&self, u: f32, v: f32) -> [f32; N] {
        let x = (u * self.w as f32 - 0.5).clamp(0.0, (self.w - 1) as f32);
        let y = (v * self.h as f32 - 0.5).clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x.fract(), y.fract());
        let get = |x: u32, y: u32| self.values[(y * self.w + x) as usize];
        let (a, b, c, d) = (get(x0, y0), get(x1, y0), get(x0, y1), get(x1, y1));
        std::array::from_fn(|i| {
            let top = a[i] + (b[i] - a[i]) * fx;
            let bottom = c[i] + (d[i] - c[i]) * fx;
            top + (bottom - top) * fy
        })
    }

    fn mean(&self) -> [f32; N] {
        let n = self.values.len() as f32;
        std::array::from_fn(|i| self.values.iter().map(|p| p[i]).sum::<f32>() / n)
    }
}

fn luma(px: &Rgba<u8>) -> u8 {
    let [r, g, b, _] = px.0.map(u32::from);
    ((r * 299 + g * 587 + b * 114) / 1000) as u8
}

/// The lowest level at or below which `fraction` of the histogram's samples fall.
fn percentile(histogram: &[u64; 256], fraction: f32) -> u8 {
    let total: u64 = histogram.iter().sum();
    let target = (total as f64 * f64::from(fraction)).ceil() as u64;
    let mut seen = 0;
    for (level, count) in histogram.iter().enumerate() {
        seen += count;
        if seen >= target.max(1) {
            return level as u8;
        }
    }
    255
}

/// Runs `f(y, row)` on each row of pixels, in parallel.
fn for_each_row(img: &mut RgbaImage, f: impl Fn(u32, &mut [u8]) + Send + Sync) {
    let stride = img.width() as usize * 4;
    if stride == 0 {
        return;
    }
    img.par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row)| f(y as u32, row));
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROWN: Rgba<u8> = Rgba([120, 80, 50, 255]);
    const CREAM: Rgba<u8> = Rgba([235, 225, 200, 255]);
    const INK: Rgba<u8> = Rgba([30, 40, 120, 255]);

    /// Ink lines run across the middle 80% of the page, centred at these fractions of its height.
    const LINES: [f32; 4] = [0.2, 0.4, 0.6, 0.8];
    const LINE_THICKNESS: f32 = 0.02;

    /// A cream page with blue lines, photographed at an angle on a brown table, lit more brightly
    /// on the right.
    fn photo(w: u32, h: u32, quad: Quad) -> RgbaImage {
        let (pw, ph) = (800, 1000);
        let page = RgbaImage::from_fn(pw, ph, |x, y| {
            let (u, v) = (x as f32 / pw as f32, y as f32 / ph as f32);
            let on_line = LINES.iter().any(|l| (v - l).abs() < LINE_THICKNESS / 2.0);
            if on_line && (0.1..0.9).contains(&u) {
                INK
            } else {
                CREAM
            }
        });
        let corners = [
            (0.0, 0.0),
            (pw as f32, 0.0),
            (pw as f32, ph as f32),
            (0.0, ph as f32),
        ];
        let projection = Projection::from_control_points(corners, quad.map(|[x, y]| (x, y)));
        let mut img = RgbaImage::new(w, h);
        warp_into(
            &page,
            projection.unwrap(),
            Interpolation::Bilinear,
            Border::Constant(BROWN),
            &mut img,
        );
        for (x, _, px) in img.enumerate_pixels_mut() {
            let light = 0.6 + 0.4 * x as f32 / w as f32;
            for c in &mut px.0[..3] {
                *c = (f32::from(*c) * light) as u8;
            }
        }
        img
    }

    const QUAD: Quad = [
        [200.0, 120.0],
        [700.0, 90.0],
        [740.0, 800.0],
        [150.0, 760.0],
    ];

    fn assert_corners_near(actual: &Quad, expected: &Quad, tolerance: f32) {
        for (a, e) in actual.iter().zip(expected) {
            let off = (a[0] - e[0]).hypot(a[1] - e[1]);
            assert!(off <= tolerance, "{actual:?} vs {expected:?}");
        }
    }

    fn mean_luma(img: &RgbaImage, u: f32, v: f32) -> u8 {
        let (x, y) = (
            (u * img.width() as f32) as u32,
            (v * img.height() as f32) as u32,
        );
        luma(img.get_pixel(x, y))
    }

    #[test]
    fn finds_outline_of_skewed_page() {
        let outline = detect_outline(&photo(900, 900, QUAD), &CleanParams::default()).unwrap();
        assert_corners_near(&outline, &QUAD, 9.0);
    }

    #[test]
    fn outline_does_not_depend_on_resolution() {
        let params = CleanParams::default();
        let small = detect_outline(&photo(900, 900, QUAD), &params).unwrap();
        let big = detect_outline(
            &photo(2700, 2700, QUAD.map(|p| p.map(|c| c * 3.0))),
            &params,
        );
        assert_corners_near(&big.unwrap(), &small.map(|p| p.map(|c| c * 3.0)), 27.0);
    }

    #[test]
    fn cleaned_page_is_white_with_dark_coloured_ink() {
        let cleaned = clean(&photo(900, 900, QUAD), &CleanParams::default(), None);
        let img = &cleaned.image;

        // Roughly the page's average edge lengths.
        let (w, h) = img.dimensions();
        assert!(
            (w as i32 - 545).abs() < 15 && (h as i32 - 676).abs() < 15,
            "{w}x{h}"
        );

        // Paper is white in both the dim and the bright half.
        for u in [0.05, 0.5, 0.95] {
            assert!(mean_luma(img, u, 0.3) > 240, "paper at {u}");
        }
        // Ink stays dark and blue.
        let px = img.get_pixel(w / 2, (0.4 * h as f32) as u32);
        assert!(luma(px) < 100, "ink {px:?}");
        assert!(px[2] > px[0] + 40, "ink {px:?}");

        assert!(cleaned.report.detected.is_ok());
        assert_eq!(cleaned.report.outline, cleaned.report.detected.ok());
        assert_eq!(cleaned.report.output_size, (w, h));
        let [r, g, b] = cleaned.report.lighting.paper_rgb;
        assert!(r > g && g > b, "paper {:?}", cleaned.report.lighting);
    }

    #[test]
    fn crop_matches_clean_without_relighting() {
        let photo = photo(900, 900, QUAD);
        let params = CleanParams::default();
        let Cropped {
            image: cropped,
            outline,
            ..
        } = crop_to_page(&photo, &params, None);
        let cleaned = clean(&photo, &params, None);
        assert_eq!(outline, cleaned.report.outline);
        assert_eq!(cropped.dimensions(), cleaned.image.dimensions());
        // The dim left side stays dim; cleaning would make it white.
        assert!(mean_luma(&cropped, 0.05, 0.3) < 200);
        assert!(mean_luma(&cleaned.image, 0.05, 0.3) > 240);
    }

    #[test]
    fn output_is_capped() {
        let params = CleanParams {
            max_output_side: 300,
            ..CleanParams::default()
        };
        let cleaned = clean(&photo(900, 900, QUAD), &params, None);
        assert_eq!(cleaned.image.width().max(cleaned.image.height()), 300);
    }

    #[test]
    fn uniform_image_has_no_outline_and_is_only_relit() {
        let img = RgbaImage::from_pixel(300, 200, Rgba([128, 128, 128, 255]));
        let cleaned = clean(&img, &CleanParams::default(), None);
        assert_eq!(cleaned.report.detected, Err(NoOutline::NoRegion));
        assert_eq!(cleaned.report.outline, None);
        assert_eq!(cleaned.image.dimensions(), (300, 200));
    }

    #[test]
    fn small_bright_region_is_not_trusted() {
        let mut img = RgbaImage::from_pixel(400, 400, BROWN);
        for (x, y, px) in img.enumerate_pixels_mut() {
            if (100..200).contains(&x) && (100..200).contains(&y) {
                *px = CREAM;
            }
        }
        let outline = detect_outline(&img, &CleanParams::default());
        assert!(
            matches!(outline, Err(NoOutline::TooSmall { area_fraction }) if (area_fraction - 0.0625).abs() < 0.01),
            "{outline:?}"
        );
    }

    #[test]
    fn orders_corners_clockwise_from_top_left() {
        let shuffled = [
            [700.0, 90.0],
            [150.0, 760.0],
            [200.0, 120.0],
            [740.0, 800.0],
        ];
        assert_eq!(order_corners(shuffled), QUAD);
    }

    /// Whether segments `a`-`b` and `c`-`d` cross.
    fn cross(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> bool {
        let side = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
            ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])).signum()
        };
        side(a, b, c) != side(a, b, d) && side(c, d, a) != side(c, d, b)
    }

    #[test]
    fn ordered_corners_never_cross() {
        let concave = [[0.0, 0.0], [100.0, 0.0], [50.0, 30.0], [50.0, 100.0]];
        let skewed = [[10.0, 50.0], [90.0, 0.0], [100.0, 60.0], [0.0, 100.0]];
        for points in [QUAD, concave, skewed] {
            // Every order the user could click them in.
            for perm in 0..24 {
                let mut rest: Vec<[f32; 2]> = points.to_vec();
                let mut shuffled = [[0.0; 2]; 4];
                let mut n = perm;
                for (i, slot) in shuffled.iter_mut().enumerate() {
                    *slot = rest.remove(n % (4 - i));
                    n /= 4 - i;
                }
                let q = order_corners(shuffled);
                assert!(
                    !cross(q[0], q[1], q[2], q[3]) && !cross(q[1], q[2], q[3], q[0]),
                    "{shuffled:?} ordered to {q:?}"
                );
            }
        }
    }

    #[test]
    fn crops_to_the_users_corners() {
        let photo = photo(900, 900, QUAD);
        let params = CleanParams::default();
        // The top half of the page, as if the user had dragged the bottom corners up.
        let lerp = |a: [f32; 2], b: [f32; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let top = [
            QUAD[0],
            QUAD[1],
            lerp(QUAD[1], QUAD[2]),
            lerp(QUAD[0], QUAD[3]),
        ];
        let cleaned = clean(&photo, &params, Some(top));
        assert_eq!(cleaned.report.outline, Some(top));
        assert!(cleaned.report.detected.is_ok(), "still detected");

        // Straightened to the corners' average edge lengths.
        let dist = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
        let [tl, tr, br, bl] = top;
        let w = ((dist(tl, tr) + dist(bl, br)) / 2.0).round() as u32;
        let h = ((dist(tl, bl) + dist(tr, br)) / 2.0).round() as u32;
        assert_eq!(cleaned.image.dimensions(), (w, h));
    }

    #[test]
    fn page_shaped_quads_are_convex_without_extreme_corners() {
        assert!(is_page_shaped(&QUAD));
        assert!(is_page_shaped(&photo_corners(300, 200)));
        let not = [
            (
                "dented",
                [[0.0, 0.0], [100.0, 0.0], [50.0, 30.0], [50.0, 100.0]],
            ),
            (
                "anticlockwise",
                [[0.0, 0.0], [0.0, 100.0], [100.0, 100.0], [100.0, 0.0]],
            ),
            (
                "crossed",
                [[0.0, 0.0], [100.0, 100.0], [100.0, 0.0], [0.0, 100.0]],
            ),
            (
                "flat corner",
                [[0.0, 0.0], [50.0, 2.0], [100.0, 0.0], [50.0, 100.0]],
            ),
            (
                "sharp corner",
                [[0.0, 0.0], [300.0, 45.0], [50.0, 60.0], [0.0, 50.0]],
            ),
            (
                "two corners together",
                [[0.0, 0.0], [0.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
            ),
        ];
        for (name, quad) in not {
            assert!(!is_page_shaped(&quad), "{name}");
        }
    }

    /// Shapes that once crashed straightening: parts of them map to near infinity.
    #[test]
    fn cropping_to_any_corners_doesnt_panic() {
        let img = RgbaImage::from_pixel(40, 30, CREAM);
        let params = CleanParams::default();
        let crashed: [Quad; 2] = [
            [[20.0, 75.0], [100.0, 90.0], [140.0, 90.0], [20.0, 135.0]],
            [[40.0, 0.0], [160.0, 0.0], [100.0, 45.0], [0.0, 150.0]],
        ];
        for quad in crashed {
            let cropped = crop_to_page(&img, &params, Some(quad.map(|p| p.map(|c| c / 5.0))));
            assert_eq!(cropped.outline, None, "not page-shaped, so not cropped");
        }
        // Corners on a coarse grid, so lines and coincident points come up often.
        let mut seed: u32 = 1;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % 9
        };
        for _ in 0..300 {
            let quad = order_corners([(); 4].map(|_| [next() as f32 * 5.0, next() as f32 * 3.75]));
            let cropped = crop_to_page(&img, &params, Some(quad));
            assert_eq!(cropped.outline.is_some(), is_page_shaped(&quad), "{quad:?}");
        }
    }

    #[test]
    fn cropping_to_the_photos_corners_only_scales_it() {
        let img = RgbaImage::from_pixel(300, 200, Rgba([128, 128, 128, 255]));
        let corners = photo_corners(300, 200);
        let cropped = crop_to_page(&img, &CleanParams::default(), Some(corners));
        assert_eq!(cropped.outline, Some(corners));
        assert_eq!(cropped.image.dimensions(), (300, 200));
    }
}
