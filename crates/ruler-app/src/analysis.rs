//! Edge analysis, off the UI thread.
//!
//! At full resolution Canny takes tens of milliseconds per monitor in a
//! release build and far longer in a debug one, so it never runs on the
//! event loop: the overlays stay responsive, and results are applied when
//! they land. Each run is tagged with a generation so that a slow run
//! finishing after a newer one was requested is simply dropped.
//!
//! The analysis itself is `ruler_core::analysis`; this is only the threading
//! and the conversion of its results into something Slint can draw.

use std::sync::Arc;
use std::time::Duration;

use image::RgbaImage;
use ruler_core::analysis;
use ruler_core::edges::EdgeMap;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// How long to wait for more sensitivity changes before re-analysing, so a
/// wheel burst or slider drag runs Canny once rather than per step.
pub const DEBOUNCE: Duration = Duration::from_millis(30);

/// Analyses every image on a background thread and hands the results (in
/// input order) to `deliver`.
pub fn spawn(
    images: Vec<Arc<RgbaImage>>,
    thresholds: (u16, u16),
    deliver: impl FnOnce(Vec<EdgeMap>) + Send + 'static,
) {
    std::thread::spawn(move || deliver(analysis::analyse_all(&images, thresholds)));
}

/// The edge map as an image: white where there is an edge, transparent
/// elsewhere, so it can be laid over the screenshot without darkening it.
pub fn edge_image(edges: &EdgeMap) -> Image {
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(edges.width(), edges.height());
    for (dst, &edge) in pixels.make_mut_slice().iter_mut().zip(edges.as_slice()) {
        *dst = if edge {
            Rgba8Pixel::new(255, 255, 255, 255)
        } else {
            Rgba8Pixel::new(0, 0, 0, 0)
        };
    }
    Image::from_rgba8(pixels)
}
