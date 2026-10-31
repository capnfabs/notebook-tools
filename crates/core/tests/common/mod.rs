//! Fixtures shared by the integration tests. Each test binary uses only some of them.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use image::RgbaImage;
use notebook_core::claude_transcribe::parse_response;
use notebook_core::clean::Crop;
use notebook_core::decode::decode;
use notebook_core::markdown_style::MarkdownStyle;
use notebook_core::transcript::Transcript;

/// Where the recorded responses were cut from: all of a 1200x900 photo.
pub const RECORDED_CROP: Crop = Crop {
    corners: [[0.0, 0.0], [1200.0, 0.0], [1200.0, 900.0], [0.0, 900.0]],
    size: (1200, 900),
};

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The bytes of fixture file `name`, e.g. `page-grey-table.jpg`.
pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(name)).unwrap()
}

/// The photo `<name>.jpg`, decoded.
pub fn photo(name: &str) -> RgbaImage {
    decode(&fixture(&format!("{name}.jpg"))).unwrap()
}

/// The recorded API response `claude/<name>.json`.
pub fn response(name: &str) -> Vec<u8> {
    fixture(&format!("claude/{name}.json"))
}

/// The recorded response `name` parsed as if for [`RECORDED_CROP`].
pub fn recorded(name: &str) -> Transcript {
    parse_response(
        200,
        &response(name),
        RECORDED_CROP,
        &MarkdownStyle::default(),
    )
    .unwrap()
}
