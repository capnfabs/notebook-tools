//! UI-agnostic logic for notebook-tools. Keep the public API to plain data types.

#[cfg(not(target_arch = "wasm32"))]
pub mod cache;
pub mod clean;
pub mod export;
pub mod ocr;
pub mod page;

pub use clean::{
    CleanParams, CleanReport, Cleaned, LightingReport, NoOutline, Quad, clean, crop_to_page,
    detect_outline,
};
pub use image::RgbaImage;
pub use page::{DecodeError, Page, Pages, decode, shrink_to_fit};
