//! UI-agnostic logic for notebook-tools. Keep the public API to plain data types.

pub mod page;

pub use image::RgbaImage;
pub use page::{DecodeError, Page, Pages, decode, shrink_to_fit};
