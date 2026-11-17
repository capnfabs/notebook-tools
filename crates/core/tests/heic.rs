//! HEIC decoding, against JPEGs of the same pictures.

mod common;

use common::fixture;
use image::RgbaImage;
use notebook_core::decode::{decode, size};

/// The mean difference per channel between two images the same size.
fn mean_difference(a: &RgbaImage, b: &RgbaImage) -> f64 {
    assert_eq!(a.dimensions(), b.dimensions());
    let total: u64 = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    total as f64 / a.as_raw().len() as f64
}

/// `page-grey-table.heic` was converted from the JPEG with `sips -s format heic`.
#[test]
fn heic_decodes_like_the_jpeg_it_came_from() {
    let heic = decode(&fixture("page-grey-table.heic")).unwrap();
    let jpeg = decode(&fixture("page-grey-table.jpg")).unwrap();
    // Both are lossy, so compare on average.
    let mean = mean_difference(&heic, &jpeg);
    assert!(mean < 4.0, "mean difference {mean:.1}");
}

/// `page-rotated.heic` is a portrait iPhone photo, stored landscape with an `irot` saying which
/// way is up; `page-rotated.jpg` is libheif's upright decoding of it (`heif-convert`).
#[test]
fn rotated_heic_is_turned_upright() {
    let heic = decode(&fixture("page-rotated.heic")).unwrap();
    let jpeg = decode(&fixture("page-rotated.jpg")).unwrap();
    assert_eq!(heic.dimensions(), (600, 800));
    let mean = mean_difference(&heic, &jpeg);
    assert!(mean < 4.0, "mean difference {mean:.1}");
}

/// Reading the size from the header gives what decoding would, turned upright.
#[test]
fn header_sizes_match_decoded_sizes() {
    for name in [
        "page-grey-table.heic",
        "page-grey-table.jpg",
        "page-rotated.heic",
        "page-rotated.jpg",
        "spread-wood-1.jpg",
    ] {
        let bytes = fixture(name);
        let decoded = decode(&bytes).unwrap().dimensions();
        assert_eq!(size(&bytes).unwrap(), decoded, "{name}");
    }
}
