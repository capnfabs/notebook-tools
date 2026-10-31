//! Flattens a photo's lighting so paper is white and ink keeps its colour, and estimates how bright
//! the surroundings are, which finding the page also uses.

use image::{GrayImage, Luma, Rgba, RgbaImage, imageops};
use imageproc::filter::gaussian_blur_f32;
use imageproc::morphology::{Mask, grayscale_dilate};
use rayon::prelude::*;

use super::{CleanParams, LightingReport};
use crate::imaging::{fitted_size, shrunk_copy};

/// Divides out the paper colour and uneven lighting, then stretches brightness so paper is white
/// and ink is dark. Keeps ink colours.
pub(super) fn fix_lighting(img: &mut RgbaImage, params: &CleanParams) -> LightingReport {
    let background = Background::estimate(img, params);
    let (w, h) = img.dimensions();
    for_each_row(img, |y, row| {
        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let bg = background.at(x as f32 / w as f32, y as f32 / h as f32);
            for c in 0..3 {
                px[c] = (f32::from(px[c]) / bg[c].max(1.0) * 255.0).min(255.0) as u8;
            }
        }
    });

    let mut histogram = [0u64; 256];
    for px in img.pixels() {
        histogram[usize::from(luma(px))] += 1;
    }
    let white = percentile(&histogram, params.white_percentile).max(1);
    let black = percentile(&histogram, params.black_percentile).min(white - 1);
    let (lo, range) = (f32::from(black), f32::from(white) - f32::from(black));
    for_each_row(img, |_, row| {
        for px in row.as_chunks_mut::<4>().0 {
            for v in &mut px[..3] {
                *v = ((f32::from(*v) - lo) / range * 255.0).clamp(0.0, 255.0) as u8;
            }
        }
    });

    LightingReport {
        paper_rgb: background.mean().map(|c| c.round() as u8),
        black_point: black,
        white_point: white,
    }
}

/// Brightness of the surroundings across an image, at low resolution: a max filter (which erases
/// ink, since ink is darker than paper in every channel) followed by a blur. One value per channel.
pub(super) struct Background<const N: usize> {
    w: u32,
    h: u32,
    values: Vec<[f32; N]>,
}

/// [`Background`] of one channel.
pub(super) fn local_brightness(img: &GrayImage, side_px: u32, radius: u8) -> Background<1> {
    let (w, h) = fitted_size(img.width(), img.height(), side_px);
    Background::from_channels([imageops::thumbnail(img, w, h)], radius)
}

impl Background<3> {
    fn estimate(img: &RgbaImage, params: &CleanParams) -> Self {
        let small = shrunk_copy(img, params.background_side_px);
        let (w, h) = small.dimensions();
        let channels = std::array::from_fn(|c| {
            GrayImage::from_fn(w, h, |x, y| Luma([small.get_pixel(x, y)[c]]))
        });
        Self::from_channels(channels, params.background_radius)
    }
}

impl<const N: usize> Background<N> {
    fn from_channels(channels: [GrayImage; N], radius: u8) -> Self {
        let (w, h) = channels[0].dimensions();
        let mask = Mask::square(radius);
        let sigma = f32::from(radius).max(1.0);
        let channels = channels.map(|ch| gaussian_blur_f32(&grayscale_dilate(&ch, &mask), sigma));
        let values = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                channels
                    .each_ref()
                    .map(|ch| f32::from(ch.get_pixel(x, y)[0]))
            })
            .collect();
        Self { w, h, values }
    }

    /// Bilinearly interpolated colour at `(u, v)`, each in 0..1 across the image.
    pub(super) fn at(&self, u: f32, v: f32) -> [f32; N] {
        let x = (u * self.w as f32 - 0.5).clamp(0.0, (self.w - 1) as f32);
        let y = (v * self.h as f32 - 0.5).clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x.fract(), y.fract());
        let get = |x: u32, y: u32| self.values[(y * self.w + x) as usize];
        let (a, b, c, d) = (get(x0, y0), get(x1, y0), get(x0, y1), get(x1, y1));
        std::array::from_fn(|i| {
            let top = a[i] + (b[i] - a[i]) * fx;
            let bottom = c[i] + (d[i] - c[i]) * fx;
            top + (bottom - top) * fy
        })
    }

    fn mean(&self) -> [f32; N] {
        let n = self.values.len() as f32;
        std::array::from_fn(|i| self.values.iter().map(|p| p[i]).sum::<f32>() / n)
    }
}

pub(super) fn luma(px: &Rgba<u8>) -> u8 {
    let [r, g, b, _] = px.0.map(u32::from);
    ((r * 299 + g * 587 + b * 114) / 1000) as u8
}

/// The lowest level at or below which `fraction` of the histogram's samples fall.
pub(super) fn percentile(histogram: &[u64; 256], fraction: f32) -> u8 {
    let total: u64 = histogram.iter().sum();
    let target = (total as f64 * f64::from(fraction)).ceil() as u64;
    let mut seen = 0;
    for (level, count) in histogram.iter().enumerate() {
        seen += count;
        if seen >= target.max(1) {
            return level as u8;
        }
    }
    255
}

/// Runs `f(y, row)` on each row of pixels, in parallel.
fn for_each_row(img: &mut RgbaImage, f: impl Fn(u32, &mut [u8]) + Send + Sync) {
    let stride = img.width() as usize * 4;
    if stride == 0 {
        return;
    }
    img.par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row)| f(y as u32, row));
}
