//! One monitor as the UI sees it: its capture, its geometry and whatever
//! analysis has finished for it, with the conversions between the window's
//! logical pixels (where the pointer and all drawing live) and the capture's
//! image pixels (where the edge map lives).

use std::sync::Arc;

use image::RgbaImage;
use ruler_core::edges::EdgeMap;
use ruler_core::geometry::{scale_from_widths, MonitorGeometry};
use ruler_core::measure;

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
    pub geometry: MonitorGeometry,
    /// Shared with the analysis thread.
    pub image: Arc<RgbaImage>,
    /// `None` until the first analysis for this monitor lands.
    pub edges: Option<EdgeMap>,
}

impl Surface {
    pub fn new(geometry: MonitorGeometry, image: RgbaImage) -> Self {
        Self {
            geometry,
            image: Arc::new(image),
            edges: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(width: u32, height: u32, scale: f32) -> Surface {
        Surface::new(
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
}
