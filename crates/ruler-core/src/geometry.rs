//! Coordinate spaces.
//!
//! The app juggles three, and mixing them up is the easiest way to break
//! multi-monitor support, so they get distinct types:
//!
//! * **virtual device px** — the whole desktop in physical pixels. What the
//!   window system reports for monitor placement.
//! * **monitor logical px** — one monitor, origin at its top-left, divided by
//!   that monitor's scale factor. What the UI draws in and measurements use.
//! * **monitor image px** — one monitor's captured screenshot, in physical
//!   pixels. The edge and region maps live here.
//!
//! A monitor's scale factor converts between the last two. Each monitor
//! carries its own, so a 2x panel beside a 1x display measures correctly on
//! both.

/// Device pixels per logical pixel, from a captured bitmap's width and the
/// logical width it covers.
///
/// Derived from widths rather than the compositor's advertised scale, which
/// is wrong under fractional scaling.
///
/// `None` when either width is unusable.
pub fn scale_from_widths(image_width: usize, logical_width: f32) -> Option<f32> {
    (image_width > 0 && logical_width > 0.0 && logical_width.is_finite())
        .then(|| image_width as f32 / logical_width)
}

/// Where one monitor sits in the virtual desktop, and how dense its pixels are.
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorGeometry {
    /// Output name, e.g. `DP-1`.
    pub name: String,
    /// Top-left corner in virtual device pixels.
    pub position: (i32, i32),
    /// Extent in device pixels.
    pub size: (u32, u32),
    /// Device pixels per logical pixel. Always > 0.
    pub scale: f32,
    pub is_primary: bool,
}

impl MonitorGeometry {
    /// Scale, or 1.0 if the window system reported a nonsensical one.
    pub fn sanitised_scale(&self) -> f32 {
        if self.scale.is_finite() && self.scale > 0.0 {
            self.scale
        } else {
            1.0
        }
    }

    /// Monitor extent in logical pixels.
    pub fn logical_size(&self) -> (f32, f32) {
        let scale = self.sanitised_scale();
        (self.size.0 as f32 / scale, self.size.1 as f32 / scale)
    }

    /// Converts a monitor-local logical point to image (device) pixels.
    pub fn logical_to_image(&self, x: f32, y: f32) -> (f32, f32) {
        let scale = self.sanitised_scale();
        (x * scale, y * scale)
    }

    /// Converts an image (device) pixel to a monitor-local logical point.
    pub fn image_to_logical(&self, x: f32, y: f32) -> (f32, f32) {
        let scale = self.sanitised_scale();
        (x / scale, y / scale)
    }

    /// Converts a device-pixel length to logical pixels.
    pub fn image_len_to_logical(&self, length: f32) -> f32 {
        length / self.sanitised_scale()
    }

    /// Converts a monitor-local logical point to a virtual-desktop device pixel.
    ///
    /// Unused until cross-monitor measurements land.
    #[allow(dead_code)]
    pub fn logical_to_virtual(&self, x: f32, y: f32) -> (f32, f32) {
        let (ix, iy) = self.logical_to_image(x, y);
        (ix + self.position.0 as f32, iy + self.position.1 as f32)
    }

    /// True when `(x, y)`, in virtual device pixels, falls inside this monitor.
    pub fn contains_virtual(&self, x: i32, y: i32) -> bool {
        x >= self.position.0
            && y >= self.position.1
            && x < self.position.0 + self.size.0 as i32
            && y < self.position.1 + self.size.1 as i32
    }
}

/// Bounding box of every monitor, in virtual device pixels.
///
/// `None` for an empty list.
#[allow(dead_code)]
pub fn virtual_bounds(monitors: &[MonitorGeometry]) -> Option<(i32, i32, u32, u32)> {
    let first = monitors.first()?;
    let mut min_x = first.position.0;
    let mut min_y = first.position.1;
    let mut max_x = first.position.0 + first.size.0 as i32;
    let mut max_y = first.position.1 + first.size.1 as i32;

    for monitor in monitors.iter().skip(1) {
        min_x = min_x.min(monitor.position.0);
        min_y = min_y.min(monitor.position.1);
        max_x = max_x.max(monitor.position.0 + monitor.size.0 as i32);
        max_y = max_y.max(monitor.position.1 + monitor.size.1 as i32);
    }

    Some((
        min_x,
        min_y,
        (max_x - min_x).max(0) as u32,
        (max_y - min_y).max(0) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, position: (i32, i32), size: (u32, u32), scale: f32) -> MonitorGeometry {
        MonitorGeometry {
            name: name.to_string(),
            position,
            size,
            scale,
            is_primary: false,
        }
    }

    #[test]
    fn logical_and_image_spaces_round_trip() {
        let hidpi = monitor("eDP-1", (0, 0), (2560, 1600), 2.0);
        assert_eq!(hidpi.logical_size(), (1280.0, 800.0));
        assert_eq!(hidpi.logical_to_image(100.0, 50.0), (200.0, 100.0));
        assert_eq!(hidpi.image_to_logical(200.0, 100.0), (100.0, 50.0));
        assert_eq!(hidpi.image_len_to_logical(64.0), 32.0);
    }

    #[test]
    fn each_monitor_applies_its_own_scale() {
        // A 100px logical span is 200 device px at 2x and 100 at 1x.
        let laptop = monitor("eDP-1", (0, 0), (2560, 1600), 2.0);
        let external = monitor("DP-1", (2560, 0), (1920, 1080), 1.0);

        assert_eq!(laptop.logical_to_image(100.0, 0.0).0, 200.0);
        assert_eq!(external.logical_to_image(100.0, 0.0).0, 100.0);
    }

    #[test]
    fn logical_points_map_into_the_virtual_desktop() {
        let external = monitor("DP-1", (2560, 0), (1920, 1080), 1.0);
        assert_eq!(external.logical_to_virtual(10.0, 20.0), (2570.0, 20.0));

        let scaled = monitor("eDP-1", (0, 1080), (2560, 1600), 2.0);
        assert_eq!(scaled.logical_to_virtual(10.0, 20.0), (20.0, 1120.0));
    }

    #[test]
    fn hit_testing_is_half_open_on_the_far_edge() {
        let m = monitor("DP-1", (100, 100), (200, 200), 1.0);
        assert!(m.contains_virtual(100, 100));
        assert!(m.contains_virtual(299, 299));
        assert!(!m.contains_virtual(300, 200), "right edge must not overlap");
        assert!(
            !m.contains_virtual(200, 300),
            "bottom edge must not overlap"
        );
        assert!(!m.contains_virtual(99, 100));
    }

    #[test]
    fn adjacent_monitors_do_not_both_claim_a_boundary_pixel() {
        let left = monitor("DP-1", (0, 0), (1920, 1080), 1.0);
        let right = monitor("DP-2", (1920, 0), (1920, 1080), 1.0);
        assert!(left.contains_virtual(1919, 0));
        assert!(!left.contains_virtual(1920, 0));
        assert!(right.contains_virtual(1920, 0));
    }

    #[test]
    fn invalid_scale_factors_fall_back_to_one() {
        assert_eq!(monitor("x", (0, 0), (10, 10), 0.0).sanitised_scale(), 1.0);
        assert_eq!(monitor("x", (0, 0), (10, 10), -2.0).sanitised_scale(), 1.0);
        assert_eq!(
            monitor("x", (0, 0), (10, 10), f32::NAN).sanitised_scale(),
            1.0
        );
        assert_eq!(monitor("x", (0, 0), (10, 10), 1.5).sanitised_scale(), 1.5);
    }

    #[test]
    fn virtual_bounds_span_every_monitor() {
        let monitors = vec![
            monitor("DP-1", (0, 0), (1920, 1080), 1.0),
            monitor("DP-2", (1920, 0), (2560, 1440), 1.0),
        ];
        assert_eq!(virtual_bounds(&monitors), Some((0, 0, 4480, 1440)));
    }

    #[test]
    fn virtual_bounds_handle_negative_origins() {
        // Monitors placed left of / above the primary report negative origins.
        let monitors = vec![
            monitor("DP-1", (0, 0), (1920, 1080), 1.0),
            monitor("DP-2", (-1920, -200), (1920, 1080), 1.0),
        ];
        assert_eq!(virtual_bounds(&monitors), Some((-1920, -200, 3840, 1280)));
    }

    #[test]
    fn virtual_bounds_of_no_monitors_is_none() {
        assert_eq!(virtual_bounds(&[]), None);
    }
}
