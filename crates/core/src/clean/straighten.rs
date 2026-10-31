//! Maps a page's corners in a photo to an upright rectangle.

use image::RgbaImage;
use imageproc::geometric_transformations::{Border, Interpolation, Projection, warp_into};

use crate::geom::{Quad, dist, is_page_shaped, rect_corners};
use crate::imaging::shrunk_copy;

/// Maps the `quad` region of `img` to an upright rectangle no longer than `max_side`.
/// Returns `None` if the quad isn't [`is_page_shaped`].
pub(super) fn straighten(img: &RgbaImage, quad: &Quad, max_side: u32) -> Option<RgbaImage> {
    if !is_page_shaped(quad) {
        return None;
    }
    let [tl, tr, br, bl] = *quad;
    let w = (dist(tl, tr) + dist(bl, br)) / 2.0;
    let h = (dist(tl, bl) + dist(tr, br)) / 2.0;
    let scale = (max_side as f32 / w.max(h)).min(1.0);

    // Shrink the source first so the warp samples at about 1:1; bilinear sampling alone aliases
    // when shrinking.
    let src_side = (img.width().max(img.height()) as f32 * scale).ceil() as u32;
    let src = shrunk_copy(img, src_side);
    let s = src.width() as f32 / img.width() as f32;

    let out_w = (w * scale).round().max(1.0) as u32;
    let out_h = (h * scale).round().max(1.0) as u32;
    let from = quad.map(|[x, y]| (x * s, y * s));
    let to = rect_corners(out_w, out_h).map(|[x, y]| (x, y));
    let projection = Projection::from_control_points(from, to)?;
    let mut out = RgbaImage::new(out_w, out_h);
    warp_into(
        &src,
        projection,
        Interpolation::Bilinear,
        Border::Replicate,
        &mut out,
    );
    Some(out)
}
