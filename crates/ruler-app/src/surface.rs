//! One monitor as the UI sees it: its capture, its geometry and whatever
//! analysis has finished for it, with the conversions between the window's
//! logical pixels (where the pointer and all drawing live) and the capture's
//! image pixels (where the edge map lives).

use std::sync::Arc;

use image::RgbaImage;
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
    pub fn rays_at(&self, x: f32, y: f32) -> Option<LogicalRays> {
        let edges = self.edges.as_ref()?;
        let (px, py) = self.logical_to_pixel(x, y);
        let rays = measure::cast_rays(edges, px, py);
        let to_logical = |len: u32| self.geometry.image_len_to_logical(len as f32);
        Some(LogicalRays {
            north: to_logical(rays.north),
            south: to_logical(rays.south),
            west: to_logical(rays.west),
            east: to_logical(rays.east),
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
        let (px, py) = self.logical_to_pixel(x, y);
        let scale = self.geometry.sanitised_scale();
        let radius = (radius * scale).ceil() as u32;
        // The perpendicular search band grows with density, so snapping
        // feels the same on a 2x panel as on a 1x one.
        let band = scale.ceil().max(1.0) as u32;
        let (sx, sy) = measure::snap_to_edge(edges, px, py, radius, band)?;
        Some(self.geometry.image_to_logical(sx as f32, sy as f32))
    }

    /// Bounding box of the UI container under a logical point.
    pub fn container_at(&self, x: f32, y: f32) -> Option<Rect> {
        let (px, py) = self.logical_to_pixel(x, y);
        let stats = self.regions.as_ref()?.container_at(px, py)?;
        Some(self.logical_rect(stats.x, stats.y, stats.width, stats.height))
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
        let shrunk = measure::shrink_to_content(edges, pixels);
        self.logical_rect(shrunk.left, shrunk.top, shrunk.width(), shrunk.height())
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

    /// An image-pixel rectangle as a logical one on this monitor.
    fn logical_rect(&self, x: u32, y: u32, width: u32, height: u32) -> Rect {
        let (lx, ly) = self.geometry.image_to_logical(x as f32, y as f32);
        Rect {
            monitor: self.monitor,
            x: lx,
            y: ly,
            width: self.geometry.image_len_to_logical(width as f32),
            height: self.geometry.image_len_to_logical(height as f32),
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
}
