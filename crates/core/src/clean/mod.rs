//! Turns a photo of a notebook page (or spread) into something close to a scan: find the page's
//! corners ([`detect`]), straighten and crop to them ([`straighten`]), then flatten the lighting to
//! white paper ([`lighting`]).
//!
//! See `decisions/2026-10-31-page-cleaning.md` and `decisions/2026-10-29-manual-corners.md`.

mod detect;
mod lighting;
mod straighten;

use std::fmt;

use image::RgbaImage;

pub use detect::detect_corners;
use lighting::fix_lighting;
use straighten::straighten;

use crate::geom::{Quad, is_page_shaped, rect_corners};
use crate::imaging::shrunk_copy;

/// Tuning knobs for [`clean`]. Pixel sizes are measured on fixed-size working copies, so results
/// don't depend on the photo's resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct CleanParams {
    /// Corner detection runs on a copy scaled down to this long side.
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
    /// Smallest page worth trusting, as a fraction of the image area.
    pub min_page_area: f32,
    /// Largest share of the page's outline allowed to run along the image border. More means the
    /// page doesn't fit in the photo or isn't distinguishable from the table.
    pub max_page_on_border: f32,
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
            min_page_area: 0.2,
            max_page_on_border: 0.05,
            // Claude's long-side limit. Transcription also caps the pixel count; see
            // `claude_transcribe::MAX_IMAGE_PIXELS` and
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
    /// What corner detection found, whether or not it was used.
    pub detected: Result<Quad, NoCorners>,
    /// The corners the page was straightened and cropped to: the user's if they placed some, else
    /// the detected ones. `None` if there were neither, or they made no sense.
    pub corners: Option<Quad>,
    pub lighting: LightingReport,
    pub output_size: (u32, u32),
}

impl CleanReport {
    /// Where the cleaned image was cut from the photo: [`Self::corners`], else the whole photo.
    pub fn crop(&self) -> Crop {
        let (w, h) = self.source_size;
        Crop {
            corners: self.corners.unwrap_or_else(|| rect_corners(w, h)),
            size: self.output_size,
        }
    }
}

/// Where an image was cut from its photo: the corners in the photo it was straightened from, and
/// its size in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub corners: Quad,
    pub size: (u32, u32),
}

impl Crop {
    /// All of a `width` x `height` photo, at its own size.
    pub fn whole(width: u32, height: u32) -> Self {
        Self {
            corners: rect_corners(width, height),
            size: (width, height),
        }
    }
}

/// Why no page corners were found. The image is then cleaned without straightening or cropping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoCorners {
    NoRegion,
    TooSmall { area_fraction: f32 },
    OnBorder { fraction: f32 },
    NotQuad { vertices: usize },
}

impl fmt::Display for NoCorners {
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
        corners,
    } = crop_to_page(img, params, corners);
    let lighting = fix_lighting(&mut image, params);
    Cleaned {
        report: CleanReport {
            source_size: img.dimensions(),
            detected,
            corners,
            lighting,
            output_size: image.dimensions(),
        },
        image,
    }
}

/// A photo cropped to its page by [`crop_to_page`].
pub struct Cropped {
    pub image: RgbaImage,
    /// What corner detection found, whether or not it was used.
    pub detected: Result<Quad, NoCorners>,
    /// The corners `image` was cropped to, if any.
    pub corners: Option<Quad>,
}

/// The page straightened and cropped to `corners`, else to its detected ones, no longer than
/// `params.max_output_side`, with the lighting left alone. Without either, the whole photo scaled
/// down.
pub fn crop_to_page(img: &RgbaImage, params: &CleanParams, corners: Option<Quad>) -> Cropped {
    let detected = detect_corners(img, params);
    let straightened = page_corners(corners, &detected)
        .and_then(|quad| Some((quad, straighten(img, &quad, params.max_output_side)?)));
    let (corners, image) = match straightened {
        Some((quad, image)) => (Some(quad), image),
        None => (None, shrunk_copy(img, params.max_output_side)),
    };
    Cropped {
        image,
        detected,
        corners,
    }
}

/// The corners [`crop_to_page`] crops to: the user's `corners`, else the `detected` ones, if
/// they're [`is_page_shaped`]. `None` means the whole photo.
pub fn page_corners(corners: Option<Quad>, detected: &Result<Quad, NoCorners>) -> Option<Quad> {
    corners
        .or_else(|| detected.as_ref().ok().copied())
        .filter(is_page_shaped)
}

#[cfg(test)]
mod tests {
    use image::Rgba;
    use imageproc::geometric_transformations::{Border, Interpolation, Projection, warp_into};

    use super::lighting::luma;
    use super::*;
    use crate::geom::{dist, order_corners};
    use crate::test_images::{BROWN, CREAM};

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
        let corners = rect_corners(pw, ph).map(|[x, y]| (x, y));
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
    fn finds_corners_of_skewed_page() {
        let corners = detect_corners(&photo(900, 900, QUAD), &CleanParams::default()).unwrap();
        assert_corners_near(&corners, &QUAD, 9.0);
    }

    #[test]
    fn corners_do_not_depend_on_resolution() {
        let params = CleanParams::default();
        let small = detect_corners(&photo(900, 900, QUAD), &params).unwrap();
        let big = detect_corners(
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
        assert_eq!(cleaned.report.corners, cleaned.report.detected.ok());
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
            corners,
            ..
        } = crop_to_page(&photo, &params, None);
        let cleaned = clean(&photo, &params, None);
        assert_eq!(corners, cleaned.report.corners);
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
    fn uniform_image_has_no_corners_and_is_only_relit() {
        let img = RgbaImage::from_pixel(300, 200, Rgba([128, 128, 128, 255]));
        let cleaned = clean(&img, &CleanParams::default(), None);
        assert_eq!(cleaned.report.detected, Err(NoCorners::NoRegion));
        assert_eq!(cleaned.report.corners, None);
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
        let detected = detect_corners(&img, &CleanParams::default());
        assert!(
            matches!(detected, Err(NoCorners::TooSmall { area_fraction }) if (area_fraction - 0.0625).abs() < 0.01),
            "{detected:?}"
        );
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
        assert_eq!(cleaned.report.corners, Some(top));
        assert!(cleaned.report.detected.is_ok(), "still detected");

        // Straightened to the corners' average edge lengths.
        let [tl, tr, br, bl] = top;
        let w = ((dist(tl, tr) + dist(bl, br)) / 2.0).round() as u32;
        let h = ((dist(tl, bl) + dist(tr, br)) / 2.0).round() as u32;
        assert_eq!(cleaned.image.dimensions(), (w, h));
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
            assert_eq!(cropped.corners, None, "not page-shaped, so not cropped");
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
            assert_eq!(cropped.corners.is_some(), is_page_shaped(&quad), "{quad:?}");
        }
    }

    #[test]
    fn cropping_to_the_photos_corners_only_scales_it() {
        let img = RgbaImage::from_pixel(300, 200, Rgba([128, 128, 128, 255]));
        let corners = rect_corners(300, 200);
        let cropped = crop_to_page(&img, &CleanParams::default(), Some(corners));
        assert_eq!(cropped.corners, Some(corners));
        assert_eq!(cropped.image.dimensions(), (300, 200));
    }
}
