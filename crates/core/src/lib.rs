//! UI-agnostic logic for notebook-tools. Keep the public API to plain data types.

pub mod cache;
pub mod clean;
pub mod compare;
pub mod export;
pub mod llm_model;
pub mod ocr;
pub mod page;
pub mod transcript;

pub use clean::{
    CleanParams, CleanReport, Cleaned, LightingReport, NoOutline, Quad, clean, crop_to_page,
    detect_outline,
};
pub use image::RgbaImage;
pub use page::{DecodeError, Page, Pages, decode, shrink_to_fit};
