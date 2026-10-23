//! UI-agnostic logic for notebook-tools. Keep the public API to plain data types.

pub mod clean;
pub mod page;

pub use clean::{
    CleanParams, CleanReport, Cleaned, LightingReport, NoOutline, Quad, clean, detect_outline,
};
pub use image::RgbaImage;
pub use page::{DecodeError, Page, Pages, decode, shrink_to_fit};
