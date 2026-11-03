//! Cutting the image export out of a window snapshot.

use image::imageops::{self, FilterType};
use image::{GenericImageView, ImageBuffer, Rgba, RgbaImage};
use ruler_core::geometry::Rect;
use slint::{Rgba8Pixel, SharedPixelBuffer};

/// How far apart the snapshot's resolution and the capture's may be and
/// still be read 1:1, as a fraction.
///
/// A window's size is whole device px, so at the capture's own scale the
/// snapshot can still be a pixel off the capture: a fraction of a per cent of
/// a screen's width, which the crop's clamping absorbs. A compositor that
/// renders the window at another scale is off by far more: a capture at 1.5
/// is a quarter smaller than a window at 2.
const SAME_RESOLUTION_TOLERANCE: f32 = 0.01;

/// Snapshot px around the region that are resampled with it, so that the
/// filter has the neighbours of its edge pixels.
const RESAMPLE_MARGIN: u32 = 1;

/// Whether a snapshot `snapshot_width` px wide must be resampled to give the
/// pixels of a capture of the same screen `capture_width` px wide.
fn needs_resample(snapshot_width: u32, capture_width: u32) -> bool {
    let ratio = capture_width as f32 / snapshot_width as f32;
    (ratio - 1.0).abs() > SAME_RESOLUTION_TOLERANCE
}

/// The pixels of `snapshot` under `rect`, a region in the window's logical
/// px, at the resolution of the screen's capture, `capture_size` device px.
/// `window_scale` is the window's own scale factor, which the snapshot was
/// rendered at.
///
/// The window is normally rendered at the capture's resolution, and the
/// region is cut out as it is. When it is not, the region is resampled to
/// the capture's resolution, so that the image still has the screen's own
/// pixels.
///
/// `None` when the region does not overlap the snapshot, or the scale is
/// unusable.
pub fn image(
    snapshot: &SharedPixelBuffer<Rgba8Pixel>,
    rect: Rect,
    window_scale: f32,
    capture_size: (u32, u32),
) -> Option<RgbaImage> {
    let (width, height) = (snapshot.width(), snapshot.height());
    // A view of the snapshot, not a copy: only the region's pixels, and a
    // margin when they are resampled, are read.
    let pixels = ImageBuffer::<Rgba<u8>, &[u8]>::from_raw(width, height, snapshot.as_bytes())?;
    if needs_resample(width, capture_size.0) {
        resample(&pixels, rect, window_scale, capture_size)
    } else {
        crop(&pixels, rect, window_scale)
    }
}

/// The pixels of `pixels` under `rect`, at `scale` device px per logical px.
fn crop(
    pixels: &impl GenericImageView<Pixel = Rgba<u8>>,
    rect: Rect,
    scale: f32,
) -> Option<RgbaImage> {
    let (left, top, width, height) = rect.device_crop(scale, pixels.dimensions())?;
    Some(RgbaImage::from_fn(width, height, |x, y| {
        pixels.get_pixel(left + x, top + y)
    }))
}

/// The pixels under `rect` at the capture's resolution, resampled from a
/// snapshot rendered at another one. Only the piece of the snapshot under
/// the region, and its margin, is resampled.
///
/// For example, a snapshot 3840 px wide at 2x and a capture 2880 px wide:
/// 0.75 capture px per snapshot px. The region at x 100 logical px, 20 px
/// wide, is capture px 150 to 180, which is snapshot px 200 to 240, read
/// from 199 to 241 with the margin. Resampled at 0.75, that piece starts at
/// capture px 149.25, so the image starts at its px 1.
fn resample(
    pixels: &ImageBuffer<Rgba<u8>, &[u8]>,
    rect: Rect,
    window_scale: f32,
    capture_size: (u32, u32),
) -> Option<RgbaImage> {
    let (snapshot_width, snapshot_height) = pixels.dimensions();
    // Capture px per snapshot px.
    let ratio = capture_size.0 as f32 / snapshot_width as f32;
    let (left, top, width, height) = rect.device_crop(window_scale * ratio, capture_size)?;

    // The snapshot px under the image, and the margin around them.
    let to_snapshot = |capture_px: u32| capture_px as f32 / ratio;
    let piece_left = (to_snapshot(left).floor() as u32).saturating_sub(RESAMPLE_MARGIN);
    let piece_top = (to_snapshot(top).floor() as u32).saturating_sub(RESAMPLE_MARGIN);
    let piece_right =
        (to_snapshot(left + width).ceil() as u32 + RESAMPLE_MARGIN).min(snapshot_width);
    let piece_bottom =
        (to_snapshot(top + height).ceil() as u32 + RESAMPLE_MARGIN).min(snapshot_height);
    let piece = imageops::crop_imm(
        pixels,
        piece_left,
        piece_top,
        piece_right.checked_sub(piece_left)?,
        piece_bottom.checked_sub(piece_top)?,
    );

    // Smoothed, so that the annotations' thin lines stay clean.
    let resized_width = (piece.width() as f32 * ratio).round() as u32;
    let resized_height = (piece.height() as f32 * ratio).round() as u32;
    if resized_width == 0 || resized_height == 0 {
        return None;
    }
    let resized = imageops::resize(&*piece, resized_width, resized_height, FilterType::Triangle);

    // Where the image starts in the resized piece. Rounding can leave it a
    // pixel short at the far edges, where the last pixel is repeated.
    let offset_x = (left as f32 - piece_left as f32 * ratio).round() as u32;
    let offset_y = (top as f32 - piece_top as f32 * ratio).round() as u32;
    Some(RgbaImage::from_fn(width, height, |x, y| {
        *resized.get_pixel(
            (offset_x + x).min(resized_width - 1),
            (offset_y + y).min(resized_height - 1),
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A snapshot whose every pixel encodes its own position: red is x,
    /// green is y, each modulo 256.
    fn snapshot(width: u32, height: u32) -> SharedPixelBuffer<Rgba8Pixel> {
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        for (index, pixel) in buffer.make_mut_slice().iter_mut().enumerate() {
            let (x, y) = (index as u32 % width, index as u32 / width);
            *pixel = Rgba8Pixel::new(x as u8, y as u8, 0, 255);
        }
        buffer
    }

    fn region(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            monitor: 0,
            x,
            y,
            width,
            height,
        }
    }

    /// [`image`] with a capture the size of the snapshot, as when the window
    /// is rendered at the capture's scale.
    fn image_at_capture_size(
        snapshot: &SharedPixelBuffer<Rgba8Pixel>,
        rect: Rect,
        scale: f32,
    ) -> Option<RgbaImage> {
        image(snapshot, rect, scale, (snapshot.width(), snapshot.height()))
    }

    #[test]
    fn a_crop_takes_the_snapshot_pixels_under_the_region() {
        // At 2x, the logical region (2, 1) 3 × 2 is device (4, 2) 6 × 4.
        let region = region(2.0, 1.0, 3.0, 2.0);
        let image = image_at_capture_size(&snapshot(20, 10), region, 2.0).expect("a crop");
        assert_eq!(image.dimensions(), (6, 4));
        assert_eq!(image.get_pixel(0, 0), &Rgba([4, 2, 0, 255]));
        assert_eq!(image.get_pixel(5, 3), &Rgba([9, 5, 0, 255]));
    }

    #[test]
    fn a_whole_window_region_crops_the_whole_snapshot() {
        // A click exports its window's logical size, physical px over the
        // scale, which a fractional scale makes fractional: scaled back, it
        // must still give every pixel, without losing an edge row or column.
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let whole = region(0.0, 0.0, 37.0 / scale, 23.0 / scale);
            let image = image_at_capture_size(&snapshot(37, 23), whole, scale).expect("a crop");
            assert_eq!(image.dimensions(), (37, 23), "{scale}");
        }
    }

    #[test]
    fn a_region_off_the_snapshot_crops_nothing() {
        let off = region(30.0, 0.0, 5.0, 5.0);
        assert_eq!(image_at_capture_size(&snapshot(20, 10), off, 1.0), None);
    }

    #[test]
    fn a_snapshot_a_pixel_off_the_capture_is_read_as_it_is() {
        // A rounding gap, not another scale: the screen's own pixels, uncut.
        let image = image(
            &snapshot(1920, 1080),
            region(100.0, 50.0, 10.0, 10.0),
            1.0,
            (1921, 1080),
        )
        .expect("a crop");
        assert_eq!(image.dimensions(), (10, 10));
        assert_eq!(image.get_pixel(0, 0), &Rgba([100, 50, 0, 255]));
    }

    #[test]
    fn a_snapshot_at_another_scale_gives_the_captures_size() {
        // A 20 × 10 logical window rendered at 2x, captured at 1.5x.
        let whole = region(0.0, 0.0, 20.0, 10.0);
        let image = image(&snapshot(40, 20), whole, 2.0, (30, 15)).expect("an image");
        assert_eq!(image.dimensions(), (30, 15));
    }

    #[test]
    fn a_small_region_of_a_resampled_snapshot_is_the_region_at_the_captures_scale() {
        // A 400 × 200 logical window rendered at 2x, captured at 1.5x: the
        // logical region (100, 60) 20 × 10 is capture px (150, 90) 30 × 15.
        let small = region(100.0, 60.0, 20.0, 10.0);
        let image = image(&snapshot(800, 400), small, 2.0, (600, 300)).expect("an image");
        assert_eq!(image.dimensions(), (30, 15));
    }

    #[test]
    fn a_resampled_region_keeps_its_place_on_the_screen() {
        // Snapshot px x at 2x is capture px 0.75 x at 1.5x: the image's
        // first pixel, capture px (150, 90), comes from snapshot px (200,
        // 120), whose red and green are 200 and 120.
        let small = region(100.0, 60.0, 20.0, 10.0);
        let image = image(&snapshot(800, 400), small, 2.0, (600, 300)).expect("an image");
        let Rgba([red, green, _, _]) = *image.get_pixel(0, 0);
        assert!(red.abs_diff(200) <= 2, "red {red}");
        assert!(green.abs_diff(120) <= 2, "green {green}");
    }
}
