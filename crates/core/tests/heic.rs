//! HEIC decoding, against the JPEG it was converted from (`sips -s format heic`).

use std::path::Path;

use notebook_core::decode;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn heic_decodes_like_the_jpeg_it_came_from() {
    let heic = decode(&fixture("page-grey-table.heic")).unwrap();
    let jpeg = decode(&fixture("page-grey-table.jpg")).unwrap();
    assert_eq!(heic.dimensions(), jpeg.dimensions());

    // Both are lossy, so compare on average.
    let total: u64 = heic
        .as_raw()
        .iter()
        .zip(jpeg.as_raw())
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    let mean = total as f64 / heic.as_raw().len() as f64;
    assert!(mean < 4.0, "mean difference {mean:.1}");
}
