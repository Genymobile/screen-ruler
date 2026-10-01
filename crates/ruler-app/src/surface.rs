//! One monitor as the UI sees it: its capture, its geometry and whatever
//! analysis has finished for it, with the conversions between the window's
//! logical pixels (where the pointer and all drawing live) and the capture's
//! image pixels (where the edge map lives).

use std::sync::Arc;

use image::RgbaImage;
use ruler_core::boundary::Bounds;
use ruler_core::color::{KernelCache, Sample};
use ruler_core::edges::EdgeMap;
use ruler_core::geometry::{scale_from_widths, MonitorGeometry, Rect};
use ruler_core::measure::{self, PixelRect};
use ruler_core::regions::RegionMap;

/// Crosshair ray lengths from the cursor to the nearest edge, logical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalRays {
    pub north: f32,
    pub south: f32,
    pub west: f32,
    pub east: f32,
}

impl LogicalRays {
    /// The measured `(width, height)`.
    pub fn size(self) -> (f32, f32) {
        (self.west + self.east, self.north + self.south)
    }
}

pub struct Surface {
    /// Index of this monitor, as `Point`/`Rect` record it.
    pub monitor: usize,
    pub geometry: MonitorGeometry,
    /// Shared with the analysis thread.
    pub image: Arc<RgbaImage>,
    /// `None` until the first analysis for this monitor lands.
    pub edges: Option<EdgeMap>,
    /// Connected regions between edges, for container detection; built
    /// alongside `edges`.
    pub regions: Option<RegionMap>,
}

impl Surface {
    pub fn new(monitor: usize, geometry: MonitorGeometry, image: RgbaImage) -> Self {
        Self {
            monitor,
            geometry,
            image: Arc::new(image),
            edges: None,
            regions: None,
        }
    }

    /// Re-derives the scale from how wide the window really is.
    ///
    /// The window system is the authority on logical size: the screenshot is
    /// stretched over the window, so image px per logical px is exactly the
    /// ratio of the two widths, whatever scale the capture backend guessed.
    pub fn reconcile_scale(&mut self, window_logical_width: f32) {
        if let Some(scale) = scale_from_widths(self.image.width() as usize, window_logical_width) {
            self.geometry.scale = scale;
        }
    }

    /// The image pixel under a logical point, clamped into the image.
    pub fn logical_to_pixel(&self, x: f32, y: f32) -> (u32, u32) {
        let (ix, iy) = self.geometry.logical_to_image(x, y);
        let clamp = |v: f32, len: u32| (v.max(0.0) as u32).min(len.saturating_sub(1));
        (
            clamp(ix, self.image.width()),
            clamp(iy, self.image.height()),
        )
    }

    /// Rays from a logical point to the nearest edge in each direction, or
    /// `None` while the edge map is not ready.
    ///
    /// Each ray ends on its edge's boundary (see `ruler_core::boundary`), so
    /// the span is exact whichever pixel of the edge the map marked. A
    /// boundary can sit inside the cursor's own pixel; that ray is then empty
    /// rather than pointing backwards.
    pub fn rays_at(&self, x: f32, y: f32) -> Option<LogicalRays> {
        let edges = self.edges.as_ref()?;
        let (px, py) = self.logical_to_pixel(x, y);
        let bounds = self.logical_bounds(measure::measure_around(edges, &self.image, px, py));
        Some(LogicalRays {
            north: (y - bounds.top).max(0.0),
            south: (bounds.bottom - y).max(0.0),
            west: (x - bounds.left).max(0.0),
            east: (bounds.right - x).max(0.0),
        })
    }
}

impl Surface {
    /// Pulls a logical point onto nearby edges within `radius` logical px.
    ///
    /// `None` when nothing was in range (or the edge map is not ready), so an
    /// unmoved point is never mistaken for a snapped one. Each axis snaps on
    /// its own, which is what makes dragging onto a UI border predictable.
    pub fn snap(&self, x: f32, y: f32, radius: f32) -> Option<(f32, f32)> {
        let edges = self.edges.as_ref()?;
        if radius <= 0.0 {
            return None;
        }
        let (ix, iy) = self.geometry.logical_to_image(x, y);
        let scale = self.geometry.sanitised_scale();
        let radius = (radius * scale).ceil() as u32;
        // The perpendicular search band grows with density, so snapping
        // feels the same on a 2x panel as on a 1x one.
        let band = scale.ceil().max(1.0) as u32;
        let (sx, sy) = measure::snap_to_boundary(edges, &self.image, ix, iy, radius, band)?;
        Some(self.geometry.image_to_logical(sx, sy))
    }

    /// Bounding box of the UI container under a logical point.
    pub fn container_at(&self, x: f32, y: f32) -> Option<Rect> {
        let (px, py) = self.logical_to_pixel(x, y);
        let stats = self.regions.as_ref()?.container_at(px, py)?;
        let bounds =
            measure::region_bounds(&self.image, stats.x, stats.y, stats.width, stats.height);
        Some(self.logical_rect(bounds))
    }

    /// Tightens a logical rectangle onto the content it encloses. The rect is
    /// returned unchanged while the edge map is not ready.
    pub fn shrink(&self, rect: Rect) -> Rect {
        let Some(edges) = self.edges.as_ref() else {
            return rect;
        };
        let (x0, y0) = self.geometry.logical_to_image(rect.x, rect.y);
        let (x1, y1) = self
            .geometry
            .logical_to_image(rect.x + rect.width, rect.y + rect.height);
        let pixels = PixelRect::from_corners(
            x0.round() as i32,
            y0.round() as i32,
            x1.round() as i32,
            y1.round() as i32,
            self.image.width().saturating_sub(1),
            self.image.height().saturating_sub(1),
        );
        self.logical_rect(measure::shrink_to_bounds(edges, &self.image, pixels))
    }

    /// The colour under a logical point, averaged over `radius` logical px,
    /// and the logical position of the pixel it was centred on (so the
    /// marker sits on a real pixel, not between two).
    pub fn sample(
        &self,
        kernels: &mut KernelCache,
        x: f32,
        y: f32,
        radius: f32,
    ) -> ((f32, f32), Sample) {
        let (px, py) = self.logical_to_pixel(x, y);
        let radius = (radius.max(0.0) * self.geometry.sanitised_scale()).round() as u32;
        let sample = kernels.sample(&self.image, px, py, radius);
        (self.geometry.image_to_logical(px as f32, py as f32), sample)
    }

    /// Image-pixel boundaries as logical ones on this monitor.
    fn logical_bounds(&self, bounds: Bounds) -> Bounds {
        let (left, top) = self.geometry.image_to_logical(bounds.left, bounds.top);
        let (right, bottom) = self.geometry.image_to_logical(bounds.right, bounds.bottom);
        Bounds {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Image-pixel boundaries as a logical rectangle on this monitor.
    fn logical_rect(&self, bounds: Bounds) -> Rect {
        let bounds = self.logical_bounds(bounds);
        Rect {
            monitor: self.monitor,
            x: bounds.left,
            y: bounds.top,
            width: bounds.width(),
            height: bounds.height(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(width: u32, height: u32, scale: f32) -> Surface {
        Surface::new(
            0,
            MonitorGeometry {
                name: "TEST".to_string(),
                position: (0, 0),
                size: (width, height),
                scale,
                is_primary: true,
            },
            RgbaImage::new(width, height),
        )
    }

    #[test]
    fn rays_wait_for_the_edge_map() {
        assert_eq!(surface(10, 10, 1.0).rays_at(5.0, 5.0), None);
    }

    #[test]
    fn rays_are_reported_in_logical_pixels() {
        // A 2x capture: an edge column 8 image px right of the cursor is 4
        // logical px away.
        let mut s = surface(20, 20, 2.0);
        let mut edges = vec![false; 400];
        for y in 0..20 {
            edges[y * 20 + 18] = true;
        }
        s.edges = EdgeMap::new(20, 20, edges);

        let rays = s.rays_at(5.0, 5.0).expect("edge map present");
        assert_eq!(rays.east, 4.0);
    }

    #[test]
    fn logical_points_off_the_image_are_clamped() {
        let s = surface(10, 10, 1.0);
        assert_eq!(s.logical_to_pixel(-5.0, 50.0), (0, 9));
    }

    #[test]
    fn scale_follows_the_real_window_width() {
        // Capture claimed 1x, but the 3840 px capture fills a 1920 px window.
        let mut s = surface(3840, 2160, 1.0);
        s.reconcile_scale(1920.0);
        assert_eq!(s.geometry.scale, 2.0);
        // A nonsensical width leaves it alone.
        s.reconcile_scale(0.0);
        assert_eq!(s.geometry.scale, 2.0);
    }

    /// An edge map with one vertical edge column at image x = `column`.
    fn with_column(mut s: Surface, column: u32) -> Surface {
        let (w, h) = (s.image.width(), s.image.height());
        let data = (0..w * h).map(|i| i % w == column).collect();
        s.edges = EdgeMap::new(w, h, data);
        s
    }

    #[test]
    fn snapping_pulls_onto_an_edge_within_the_radius() {
        let s = with_column(surface(40, 40, 1.0), 20);
        let (x, y) = s.snap(17.0, 10.0, 5.0).expect("edge in range");
        assert_eq!((x, y), (20.0, 10.0), "snaps in x, keeps y");
        assert_eq!(s.snap(10.0, 10.0, 5.0), None, "edge out of range");
        assert_eq!(s.snap(19.0, 10.0, 0.0), None, "a zero radius never snaps");
    }

    #[test]
    fn the_snap_radius_is_logical() {
        // At 2x, 5 logical px reach 10 image px: an edge 8 image px away.
        let s = with_column(surface(40, 40, 2.0), 20);
        let (x, _) = s.snap(6.0, 5.0, 5.0).expect("edge in range");
        assert_eq!(x, 10.0);
    }

    #[test]
    fn the_rect_tools_wait_for_the_analysis() {
        let s = surface(10, 10, 1.0);
        assert_eq!(s.container_at(5.0, 5.0), None);
        let rect = Rect {
            monitor: 0,
            x: 1.0,
            y: 1.0,
            width: 5.0,
            height: 5.0,
        };
        assert_eq!(s.shrink(rect), rect);
    }

    #[test]
    fn samples_land_on_a_whole_pixel() {
        let mut s = surface(10, 10, 2.0);
        s.image = Arc::new(RgbaImage::from_pixel(
            10,
            10,
            image::Rgba([230, 25, 94, 255]),
        ));
        let ((x, y), sample) = s.sample(&mut KernelCache::new(), 2.3, 1.8, 0.0);
        // 2.3 logical is image px 4 (4.6 truncated), back to 2.0 logical.
        assert_eq!((x, y), (2.0, 1.5));
        assert_eq!(sample.hex(), "#E6195E");
    }

    /// A 1.5x capture of a black 137 x 89 logical box on white, at logical
    /// (40, 30): its edges fall between device pixels and are antialiased,
    /// as a compositor draws them. Analysed by the real pipeline.
    fn fractional_box() -> Surface {
        let scale = 1.502_343_8_f32;
        let (x0, y0, x1, y1) = (40.0 * scale, 30.0 * scale, 177.0 * scale, 119.0 * scale);
        let overlap = |p: u32, a: f32, b: f32| ((p + 1) as f32).min(b) - (p as f32).max(a);
        let image = RgbaImage::from_fn(330, 240, |x, y| {
            let cover = overlap(x, x0, x1).max(0.0) * overlap(y, y0, y1).max(0.0);
            let v = (255.0 * (1.0 - cover)).round() as u8;
            image::Rgba([v, v, v, 255])
        });
        let analysis = ruler_core::analysis::Analysis::of(
            &image,
            ruler_core::edges::sensitivity_to_thresholds(ruler_core::edges::DEFAULT_SENSITIVITY),
        );
        let mut s = surface(330, 240, scale);
        s.image = Arc::new(image);
        s.edges = Some(analysis.edges);
        s.regions = Some(analysis.regions);
        s
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.05
    }

    #[test]
    fn every_tool_measures_a_fractional_box_exactly() {
        let s = fractional_box();

        let (w, h) = s.rays_at(100.0, 70.0).expect("analysed").size();
        assert!(near(w, 137.0) && near(h, 89.0), "crosshair {w} x {h}");

        let c = s.container_at(100.0, 70.0).expect("a container");
        assert!(
            near(c.width, 137.0) && near(c.height, 89.0),
            "container {c:?}"
        );
        assert!(near(c.x, 40.0) && near(c.y, 30.0), "container {c:?}");

        let loose = Rect {
            monitor: 0,
            x: 25.0,
            y: 15.0,
            width: 175.0,
            height: 120.0,
        };
        let r = s.shrink(loose);
        assert!(near(r.width, 137.0) && near(r.height, 89.0), "shrink {r:?}");

        // Corner to corner, as a drag does: both corners snap onto the box.
        let (ax, ay) = s.snap(42.0, 32.0, 10.0).expect("near the corner");
        let (bx, by) = s.snap(175.0, 117.0, 10.0).expect("near the corner");
        assert!(
            near(bx - ax, 137.0) && near(by - ay, 89.0),
            "snapped {ax},{ay} -> {bx},{by}"
        );
    }
}
