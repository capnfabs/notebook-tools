//! UI-agnostic logic for notebook-tools. Keep the public API to plain data types.

pub mod cache;
pub mod clean;
pub mod compare;
pub mod decode;
pub mod export;
pub mod llm_model;
pub mod ocr;
pub mod page;
pub mod transcript;

pub use clean::{
    CleanParams, CleanReport, Cleaned, LightingReport, NoOutline, Quad, clean, crop_to_page,
    detect_outline,
};
pub use decode::{DecodeError, decode};
pub use image::RgbaImage;
pub use page::{Loaded, Page, Pages, shrink_to_fit};
