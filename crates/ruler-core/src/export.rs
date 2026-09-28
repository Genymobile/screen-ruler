//! Composite image export.
//!
//! Rather than re-rendering annotations through a second code path, the export
//! reads back the window's own contents (Slint's `Window::take_snapshot`) for
//! one frame drawn without any UI chrome. What lands on the clipboard is
//! therefore exactly what was on screen, and there is no second renderer to
//! keep in sync.

use crate::geometry::MonitorGeometry;
use crate::image::RgbaImage;
use crate::state::Rect;

/// A crop rectangle `(left, top, width, height)` in device pixels.
pub type DeviceCrop = (u32, u32, u32, u32);

/// Converts a logical export rectangle into device pixels within the window.
///
/// Returns `None` when the rectangle does not overlap the window at all.
pub fn device_crop(
    rect: Rect,
    geometry: &MonitorGeometry,
    window: (u32, u32),
) -> Option<DeviceCrop> {
    let scale = geometry.sanitised_scale();
    let left = (rect.x * scale).round().max(0.0) as u32;
    let top = (rect.y * scale).round().max(0.0) as u32;
    let width = (rect.width * scale).round().max(0.0) as u32;
    let height = (rect.height * scale).round().max(0.0) as u32;

    if width == 0 || height == 0 || left >= window.0 || top >= window.1 {
        return None;
    }

    // Clamp to the window: a drag can end slightly outside it.
    let width = width.min(window.0 - left);
    let height = height.min(window.1 - top);
    Some((left, top, width, height))
}

/// Cuts a region out of a full-window snapshot.
///
/// `None` when the region does not fit inside the snapshot, which means the
/// snapshot and the crop disagree about the window's size.
pub fn crop_snapshot(snapshot: &RgbaImage, crop: DeviceCrop) -> Option<RgbaImage> {
    let (left, top, width, height) = crop;
    let fits = left.checked_add(width)? <= snapshot.width()
        && top.checked_add(height)? <= snapshot.height();
    fits.then(|| ::image::imageops::crop_imm(snapshot, left, top, width, height).to_image())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Rgba;

    fn geometry(scale: f32) -> MonitorGeometry {
        MonitorGeometry {
            name: "TEST".to_string(),
            position: (0, 0),
            size: (1920, 1080),
            scale,
            is_primary: true,
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            monitor: 0,
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn logical_rects_scale_into_device_pixels() {
        let crop = device_crop(rect(10.0, 20.0, 100.0, 50.0), &geometry(2.0), (1920, 1080));
        assert_eq!(crop, Some((20, 40, 200, 100)));

        let unscaled = device_crop(rect(10.0, 20.0, 100.0, 50.0), &geometry(1.0), (1920, 1080));
        assert_eq!(unscaled, Some((10, 20, 100, 50)));
    }

    #[test]
    fn a_rect_running_past_the_window_is_clamped() {
        let crop = device_crop(
            rect(1900.0, 1070.0, 200.0, 200.0),
            &geometry(1.0),
            (1920, 1080),
        );
        assert_eq!(crop, Some((1900, 1070, 20, 10)));
    }

    #[test]
    fn degenerate_and_offscreen_rects_are_rejected() {
        assert!(device_crop(rect(10.0, 10.0, 0.0, 50.0), &geometry(1.0), (1920, 1080)).is_none());
        assert!(
            device_crop(rect(5000.0, 10.0, 50.0, 50.0), &geometry(1.0), (1920, 1080)).is_none()
        );
        assert!(
            device_crop(rect(10.0, 5000.0, 50.0, 50.0), &geometry(1.0), (1920, 1080)).is_none()
        );
    }

    #[test]
    fn cropping_extracts_the_requested_region() {
        // 4x3 window where each pixel encodes its own coordinates.
        let snapshot = RgbaImage::from_fn(4, 3, |x, y| Rgba([x as u8, y as u8, 0, 255]));

        let cropped = crop_snapshot(&snapshot, (1, 1, 2, 2)).expect("crop");
        assert_eq!(cropped.dimensions(), (2, 2));
        assert_eq!(cropped.get_pixel(0, 0).0, [1, 1, 0, 255]);
        assert_eq!(cropped.get_pixel(1, 0).0, [2, 1, 0, 255]);
        assert_eq!(cropped.get_pixel(0, 1).0, [1, 2, 0, 255]);
        assert_eq!(cropped.get_pixel(1, 1).0, [2, 2, 0, 255]);
    }

    #[test]
    fn cropping_rejects_a_region_outside_the_snapshot() {
        let snapshot = RgbaImage::new(4, 3);
        assert!(crop_snapshot(&snapshot, (3, 0, 2, 2)).is_none());
        assert!(crop_snapshot(&snapshot, (0, 2, 2, 2)).is_none());
        assert!(
            crop_snapshot(&snapshot, (u32::MAX, 0, 2, 2)).is_none(),
            "no overflow"
        );
    }

    #[test]
    fn a_crop_computed_for_the_window_fits_its_snapshot() {
        // The two halves meet in the app: device_crop sizes against the
        // window, crop_snapshot cuts the snapshot of that window.
        let snapshot = RgbaImage::new(1920, 1080);
        let crop = device_crop(
            rect(1900.0, 1070.0, 200.0, 200.0),
            &geometry(1.0),
            snapshot.dimensions(),
        )
        .expect("overlaps");
        assert_eq!(
            crop_snapshot(&snapshot, crop).map(|c| c.dimensions()),
            Some((20, 10))
        );
    }
}
