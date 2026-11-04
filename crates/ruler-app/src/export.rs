//! Cutting the image export out of the window snapshots, and stitching the
//! pieces of a region that spans screens into one image.
//!
//! A region is dragged on one window, in its logical px, and may run onto
//! others. Every window is placed in the desktop's shared frame (see
//! [`ruler_core::capture::desktop_frame_is_logical`]), which is what relates
//! one window's px to another's: [`composite`] splits the region into one
//! [`Part`] per screen it covers, and [`stitch`] puts their pixels together.

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

/// Frame units a point may lie off a window and still count as on it.
///
/// A window's logical size is its whole device px over a scale that the
/// compositor may have rounded, so it can fall a unit or so short of the
/// screen it covers; a pointer at the screen's far edge is still on it.
const LAYOUT_TOLERANCE: f32 = 2.0;

/// Where one overlay window sits in the desktop's shared frame, and how
/// dense its capture is there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The window's top-left corner, in frame units.
    pub origin: (f32, f32),
    /// Frame units per logical px of the window: see [`frame_units`].
    pub units: f32,
    /// Capture px per frame unit: the capture's scale over `units`.
    pub density: f32,
    /// The window's size, in its logical px.
    pub size: (f32, f32),
}

/// Frame units per logical px of a window whose own scale factor is
/// `window_scale` and whose screen's, as the window system reports it, is
/// `system_scale`. `logical` is whether the frame is in logical px
/// ([`ruler_core::capture::desktop_frame_is_logical`]).
///
/// A physical frame is in device px, and the window has `window_scale` of
/// them per logical px. A logical frame is in the window system's logical
/// px, which are `system_scale` device px each: one per logical px of the
/// window, unless `SLINT_SCALE_FACTOR` overrides the window's scale. A
/// window at 2 on a screen at 1 then spans two frame units per logical px.
pub fn frame_units(logical: bool, window_scale: f32, system_scale: f32) -> f32 {
    if logical {
        window_scale / system_scale
    } else {
        window_scale
    }
}

impl Placement {
    /// A placement no arithmetic below can be trusted with is left out of
    /// the layout, as if its window had no place.
    fn is_usable(&self) -> bool {
        let positive = |value: f32| value.is_finite() && value > 0.0;
        positive(self.units)
            && positive(self.density)
            && self.origin.0.is_finite()
            && self.origin.1.is_finite()
            && self.size.0.is_finite()
            && self.size.1.is_finite()
    }

    /// `rect`, in this window's logical px, in frame units.
    fn frame_bounds(&self, rect: Rect) -> Bounds {
        Bounds {
            left: self.origin.0 + rect.x * self.units,
            top: self.origin.1 + rect.y * self.units,
            right: self.origin.0 + (rect.x + rect.width) * self.units,
            bottom: self.origin.1 + (rect.y + rect.height) * self.units,
        }
    }

    /// `bounds`, in frame units, in the logical px of this window,
    /// `monitor`.
    fn local_rect(&self, bounds: Bounds, monitor: usize) -> Rect {
        Rect {
            monitor,
            x: (bounds.left - self.origin.0) / self.units,
            y: (bounds.top - self.origin.1) / self.units,
            width: (bounds.right - bounds.left) / self.units,
            height: (bounds.bottom - bounds.top) / self.units,
        }
    }

    /// The whole window, in frame units.
    fn bounds(&self) -> Bounds {
        self.frame_bounds(Rect {
            monitor: 0,
            x: 0.0,
            y: 0.0,
            width: self.size.0,
            height: self.size.1,
        })
    }

    /// The piece of `region`, in frame units, on this window, when it covers
    /// at least one capture px of it each way.
    ///
    /// A thinner sliver, such as the shared edge of the screen next to a
    /// clicked one, is left out: it would add no pixel of its own, yet its
    /// density could raise the whole image's. At one px or more, it is also
    /// at least one px at any higher density, so no part is ever empty.
    fn overlap(&self, region: Bounds) -> Option<Bounds> {
        let bounds = self.bounds();
        let overlap = Bounds {
            left: region.left.max(bounds.left),
            top: region.top.max(bounds.top),
            right: region.right.min(bounds.right),
            bottom: region.bottom.min(bounds.bottom),
        };
        let covers = |extent: f32| extent * self.density >= 1.0;
        (covers(overlap.right - overlap.left) && covers(overlap.bottom - overlap.top))
            .then_some(overlap)
    }

    /// Whether `point`, in frame units, is on this window, give or take
    /// [`LAYOUT_TOLERANCE`].
    fn contains(&self, (x, y): (f32, f32)) -> bool {
        let bounds = self.bounds();
        (bounds.left - LAYOUT_TOLERANCE..=bounds.right + LAYOUT_TOLERANCE).contains(&x)
            && (bounds.top - LAYOUT_TOLERANCE..=bounds.bottom + LAYOUT_TOLERANCE).contains(&y)
    }
}

/// A rectangle in the desktop's frame, by its edges.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Bounds {
    /// The smallest bounds around both.
    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

/// The piece of an export region on one screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// The window it is on.
    pub monitor: usize,
    /// The piece, in that window's logical px, clipped to the window.
    pub region: Rect,
    /// Where its pixels go in the image: `(left, top, width, height)` px.
    pub destination: (u32, u32, u32, u32),
    /// Whether its capture is less dense than the image, so that its pixels
    /// are upscaled to the destination rather than copied as they are.
    pub upscale: bool,
}

/// How the image of a region is made: its size, and the piece each screen
/// gives.
#[derive(Clone, Debug, PartialEq)]
pub struct Composite {
    /// The image's size in px, around every part.
    pub size: (u32, u32),
    pub parts: Vec<Part>,
}

/// Where `monitor`'s window sits in `layout`, unless it has no usable place.
fn place_of(layout: &[Option<Placement>], monitor: usize) -> Option<Placement> {
    layout
        .get(monitor)
        .copied()
        .flatten()
        .filter(Placement::is_usable)
}

/// The windows `layout` places, by monitor.
fn placed(layout: &[Option<Placement>]) -> impl Iterator<Item = (usize, Placement)> + '_ {
    (0..layout.len()).filter_map(|monitor| Some((monitor, place_of(layout, monitor)?)))
}

/// How to make the image of `region`, dragged on its monitor's window in its
/// logical px, given where each window sits in the desktop's frame, indexed
/// by monitor. A window with no place is left out, as is a gap between
/// screens: their pixels stay transparent. A seam under
/// [`LAYOUT_TOLERANCE`] is not a gap: see [`close_seams`]. When windows
/// overlap ([`placements_overlap`]), the layout cannot be trusted, and only
/// the start window is used.
///
/// The image has the density of the densest screen it covers, so that no
/// screen loses pixels; a less dense one is upscaled. Each part's edges are
/// rounded from the start window's origin, not from the part's size, so that
/// neighbouring parts meet without a seam or an overlap, and so that the
/// start window's part is the crop of that screen alone.
///
/// `None` when the start window has no place, or the region covers no
/// screen.
pub fn composite(region: Rect, layout: &[Option<Placement>]) -> Option<Composite> {
    let start = place_of(layout, region.monitor)?;
    let frame = start.frame_bounds(region);
    let trusted = !placements_overlap(layout);
    let overlaps: Vec<_> = placed(layout)
        .filter(|&(monitor, _)| trusted || monitor == region.monitor)
        .filter_map(|(monitor, placement)| Some((monitor, placement, placement.overlap(frame)?)))
        .collect();
    let density = overlaps
        .iter()
        .map(|(_, placement, _)| placement.density)
        .reduce(f32::max)?;
    let pieces: Vec<_> = overlaps.iter().map(|(_, _, overlap)| *overlap).collect();
    let pieces = close_seams(&pieces);
    let extent = pieces.iter().copied().reduce(Bounds::union)?;

    // An edge's px from the start window's origin at the image's density:
    // two parts sharing an edge round it alike, and the start window's own
    // edges round as its crop does, wherever its origin falls on the grid.
    let edge_x = |frame_units: f32| ((frame_units - start.origin.0) * density).round() as i64;
    let edge_y = |frame_units: f32| ((frame_units - start.origin.1) * density).round() as i64;
    let (left, top) = (edge_x(extent.left), edge_y(extent.top));
    let size = (
        (edge_x(extent.right) - left) as u32,
        (edge_y(extent.bottom) - top) as u32,
    );
    let parts = overlaps
        .into_iter()
        .zip(pieces)
        .map(|((monitor, placement, overlap), piece)| {
            let (part_left, part_top) = (edge_x(piece.left), edge_y(piece.top));
            Part {
                monitor,
                region: placement.local_rect(overlap, monitor),
                destination: (
                    (part_left - left) as u32,
                    (part_top - top) as u32,
                    (edge_x(piece.right) - part_left) as u32,
                    (edge_y(piece.bottom) - part_top) as u32,
                ),
                upscale: density / placement.density - 1.0 > SAME_RESOLUTION_TOLERANCE,
            }
        })
        .collect();
    Some(Composite { size, parts })
}

/// Whether two windows of `layout` overlap by more than
/// [`LAYOUT_TOLERANCE`] each way.
///
/// Screens sit side by side, so windows on the same place mean a layout that
/// cannot be right: mirrored displays, or winit naming the wrong monitor for
/// a window. Stitched from it, a later part would silently cover an earlier
/// one.
pub fn placements_overlap(layout: &[Option<Placement>]) -> bool {
    let windows: Vec<_> = placed(layout)
        .map(|(_, placement)| placement.bounds())
        .collect();
    windows.iter().enumerate().any(|(index, a)| {
        windows[index + 1..].iter().any(|b| {
            let across = a.right.min(b.right) - a.left.max(b.left);
            let down = a.bottom.min(b.bottom) - a.top.max(b.top);
            across > LAYOUT_TOLERANCE && down > LAYOUT_TOLERANCE
        })
    })
}

/// `pieces` of a region, in frame units, each with its right and bottom
/// edges moved onto the left or top edge of a neighbouring piece that starts
/// less than [`LAYOUT_TOLERANCE`] past them.
///
/// A window can fall a unit or so short of its screen, which would leave a
/// transparent seam a px or two wide before the next screen. The window's
/// part is stretched over it instead, as [`stitch`] does for a pixel of
/// rounding. [`lands_on_a_window`] accepts the same shortfall, and more: a
/// drag ending in a gap up to twice [`LAYOUT_TOLERANCE`] still counts as on
/// a screen, while only a seam under it is closed. A wider gap is between
/// screens that do not touch, and stays transparent.
fn close_seams(pieces: &[Bounds]) -> Vec<Bounds> {
    let is_seam = |gap: f32| gap > 0.0 && gap < LAYOUT_TOLERANCE;
    pieces
        .iter()
        .map(|&piece| {
            let mut closed = piece;
            for other in pieces {
                let beside = piece.top < other.bottom && other.top < piece.bottom;
                let above = piece.left < other.right && other.left < piece.right;
                if beside && is_seam(other.left - piece.right) {
                    closed.right = closed.right.max(other.left);
                }
                if above && is_seam(other.top - piece.bottom) {
                    closed.bottom = closed.bottom.max(other.top);
                }
            }
            closed
        })
        .collect()
}

/// `region`, dragged on its monitor's window, in the logical px of
/// `monitor`'s window, when it covers some of that screen: where the outline
/// is drawn there. Unclipped, so that the window clips it and the outline's
/// pieces line up across screens.
///
/// The start window always has it, whatever the layout: that is where it is
/// being dragged. When windows overlap ([`placements_overlap`]), no other
/// window has it, as [`composite`] uses none of them.
pub fn region_on(region: Rect, layout: &[Option<Placement>], monitor: usize) -> Option<Rect> {
    if monitor == region.monitor {
        return Some(region);
    }
    if placements_overlap(layout) {
        return None;
    }
    covered_part(region, layout, monitor)
}

/// Whether [`composite`] leaves out a screen `region` covers, because the
/// windows of `layout` overlap: what the user is told about.
pub fn leaves_out_overlapping_screens(region: Rect, layout: &[Option<Placement>]) -> bool {
    placements_overlap(layout)
        && placed(layout).any(|(monitor, _)| {
            monitor != region.monitor && covered_part(region, layout, monitor).is_some()
        })
}

/// `region`, dragged on its monitor's window, in the logical px of another
/// window, `monitor`, when it covers some of that screen, whether or not the
/// layout can be trusted.
fn covered_part(region: Rect, layout: &[Option<Placement>], monitor: usize) -> Option<Rect> {
    let start = place_of(layout, region.monitor)?;
    let placement = place_of(layout, monitor)?;
    let frame = start.frame_bounds(region);
    placement.overlap(frame)?;
    Some(placement.local_rect(frame, monitor))
}

/// Whether `point`, in the logical px of `monitor`'s window, lands on a
/// window of `layout`.
///
/// The end of a drag is always on a screen. When the layout puts it on none,
/// the layout is wrong, and so would be an image stitched from it.
pub fn lands_on_a_window(layout: &[Option<Placement>], monitor: usize, point: (f32, f32)) -> bool {
    let Some(start) = place_of(layout, monitor) else {
        return false;
    };
    let frame = start.frame_bounds(Rect {
        monitor,
        x: point.0,
        y: point.1,
        width: 0.0,
        height: 0.0,
    });
    placed(layout).any(|(_, placement)| placement.contains((frame.left, frame.top)))
}

/// The image of `composite`, from `images`, the pixels of each of its parts
/// in order, at their own capture's resolution. Where no part lands, the
/// image is transparent.
///
/// A part at the image's density is copied pixel for pixel: its pixels and
/// its destination can differ by a pixel of rounding, or by a seam
/// [`composite`] closed, made up by repeating the last row or column. A less
/// dense part is upscaled, smoothly, so that the annotations' thin lines stay
/// clean: next to a closed seam, the whole part is then stretched over it.
pub fn stitch(composite: &Composite, images: &[RgbaImage]) -> RgbaImage {
    let mut canvas = RgbaImage::new(composite.size.0, composite.size.1);
    for (part, image) in composite.parts.iter().zip(images) {
        let (left, top, width, height) = part.destination;
        if image.width() == 0 || image.height() == 0 {
            continue;
        }
        let piece = if part.upscale {
            imageops::resize(image, width, height, FilterType::CatmullRom)
        } else {
            RgbaImage::from_fn(width, height, |x, y| {
                *image.get_pixel(x.min(image.width() - 1), y.min(image.height() - 1))
            })
        };
        imageops::replace(&mut canvas, &piece, left.into(), top.into());
    }
    canvas
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

    // ------------------------------------------------- across screens

    /// A window `size` logical px at `origin` in the frame, with `units`
    /// frame units per logical px and `density` capture px per frame unit.
    fn window(origin: (f32, f32), units: f32, density: f32, size: (f32, f32)) -> Option<Placement> {
        Some(Placement {
            origin,
            units,
            density,
            size,
        })
    }

    /// A region dragged on `monitor`'s window.
    fn region_on_monitor(monitor: usize, x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            monitor,
            ..region(x, y, width, height)
        }
    }

    /// The destinations of `composite`'s parts, by monitor.
    fn destinations(composite: &Composite) -> Vec<(usize, (u32, u32, u32, u32))> {
        composite
            .parts
            .iter()
            .map(|part| (part.monitor, part.destination))
            .collect()
    }

    /// Two 1920 × 1080 screens side by side at 1x, in a logical frame.
    fn side_by_side() -> Vec<Option<Placement>> {
        vec![
            window((0.0, 0.0), 1.0, 1.0, (1920.0, 1080.0)),
            window((1920.0, 0.0), 1.0, 1.0, (1920.0, 1080.0)),
        ]
    }

    /// A 100 × 100 screen with another to its right, 50 px lower: below the
    /// first and above the second is a gap.
    fn offset_vertically() -> Vec<Option<Placement>> {
        vec![
            window((0.0, 0.0), 1.0, 1.0, (100.0, 100.0)),
            window((100.0, 50.0), 1.0, 1.0, (100.0, 100.0)),
        ]
    }

    /// An image of one colour.
    fn filled(width: u32, height: u32, colour: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(colour))
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    #[test]
    fn a_region_on_one_screen_is_one_part_the_size_of_its_crop() {
        // At 2x, the logical region (2, 1) 3 × 2 is device (4, 2) 6 × 4, as
        // the crop of a single screen makes it.
        let layout = [window((0.0, 0.0), 1.0, 2.0, (100.0, 50.0))];
        let region = region(2.0, 1.0, 3.0, 2.0);
        let composite = composite(region, &layout).expect("a composite");
        assert_eq!(composite.size, (6, 4));
        assert_eq!(
            composite.parts,
            vec![Part {
                monitor: 0,
                region,
                destination: (0, 0, 6, 4),
                upscale: false,
            }]
        );
        assert_eq!(region.device_crop(2.0, (200, 100)), Some((4, 2, 6, 4)));
    }

    #[test]
    fn a_region_running_off_its_only_screen_stops_at_its_edge() {
        let layout = [window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0))];
        let composite = composite(region(90.0, 40.0, 30.0, 30.0), &layout).expect("a composite");
        assert_eq!(composite.size, (10, 10));
        assert_eq!(composite.parts[0].region, region(90.0, 40.0, 10.0, 10.0));
    }

    #[test]
    fn side_by_side_screens_at_one_scale_meet_without_a_seam() {
        let composite =
            composite(region(1900.0, 10.0, 50.0, 20.0), &side_by_side()).expect("a composite");
        assert_eq!(composite.size, (50, 20));
        assert_eq!(
            destinations(&composite),
            vec![(0, (0, 0, 20, 20)), (1, (20, 0, 30, 20))]
        );
        assert_eq!(composite.parts[0].region, region(1900.0, 10.0, 20.0, 20.0));
        assert_eq!(
            composite.parts[1].region,
            region_on_monitor(1, 0.0, 10.0, 30.0, 20.0)
        );
    }

    #[test]
    fn a_region_dragged_back_from_the_neighbouring_screen_spans_both() {
        // Dragged from the right screen leftwards: the region starts at a
        // negative x on its own window.
        let composite = composite(
            region_on_monitor(1, -20.0, 0.0, 30.0, 10.0),
            &side_by_side(),
        )
        .expect("a composite");
        assert_eq!(
            destinations(&composite),
            vec![(0, (0, 0, 20, 10)), (1, (20, 0, 10, 10))]
        );
        assert_eq!(composite.parts[0].region, region(1900.0, 0.0, 20.0, 10.0));
    }

    #[test]
    fn at_mixed_scales_the_image_takes_the_highest_density() {
        // A 2x screen with a 1x one to its right, in a logical frame.
        let layout = [
            window((0.0, 0.0), 1.0, 2.0, (100.0, 50.0)),
            window((100.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
        ];
        let composite = composite(region(90.0, 0.0, 20.0, 10.0), &layout).expect("a composite");
        assert_eq!(composite.size, (40, 20));
        assert_eq!(
            destinations(&composite),
            vec![(0, (0, 0, 20, 20)), (1, (20, 0, 20, 20))]
        );
        assert!(!composite.parts[0].upscale);
        assert!(composite.parts[1].upscale);
        assert_eq!(
            composite.parts[1].region,
            region_on_monitor(1, 0.0, 0.0, 10.0, 10.0)
        );
    }

    #[test]
    fn a_physical_frame_places_each_window_by_its_scale() {
        // As on X11: a 2x screen 960 logical px wide is 1920 frame units,
        // next to a 1x screen. Both captures are one px per frame unit.
        let layout = [
            window((0.0, 0.0), 2.0, 1.0, (960.0, 540.0)),
            window((1920.0, 0.0), 1.0, 1.0, (1920.0, 1080.0)),
        ];
        let composite = composite(region(950.0, 0.0, 20.0, 10.0), &layout).expect("a composite");
        assert_eq!(composite.size, (40, 20));
        assert_eq!(
            destinations(&composite),
            vec![(0, (0, 0, 20, 20)), (1, (20, 0, 20, 20))]
        );
        assert_eq!(composite.parts[0].region, region(950.0, 0.0, 10.0, 10.0));
        assert_eq!(
            composite.parts[1].region,
            region_on_monitor(1, 0.0, 0.0, 20.0, 20.0)
        );
        assert!(composite.parts.iter().all(|part| !part.upscale));
    }

    #[test]
    fn screens_left_of_the_frame_origin_are_placed_too() {
        let layout = [
            window((-100.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
            window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
        ];
        let composite =
            composite(region_on_monitor(1, -20.0, 0.0, 40.0, 10.0), &layout).expect("a composite");
        assert_eq!(composite.size, (40, 10));
        assert_eq!(
            destinations(&composite),
            vec![(0, (0, 0, 20, 10)), (1, (20, 0, 20, 10))]
        );
        assert_eq!(composite.parts[0].region, region(80.0, 0.0, 20.0, 10.0));
    }

    #[test]
    fn a_gap_between_screens_is_in_no_part() {
        let composite =
            composite(region(50.0, 20.0, 100.0, 100.0), &offset_vertically()).expect("a composite");
        assert_eq!(composite.size, (100, 100));
        // The first screen ends 80 px down, the second starts 30 px down.
        assert_eq!(
            destinations(&composite),
            vec![(0, (0, 0, 50, 80)), (1, (50, 30, 50, 70))]
        );
    }

    #[test]
    fn parts_at_a_fractional_density_tile_the_image_exactly() {
        // Both edges of each part round from the frame, so the parts always
        // add up to the image, however the region falls on the px grid.
        let layout = [
            window((0.0, 0.0), 1.0, 1.5, (101.0, 50.0)),
            window((101.0, 0.0), 1.0, 1.5, (101.0, 50.0)),
        ];
        for step in 0..20 {
            let x = 95.0 + step as f32 * 0.17;
            let composite = composite(region(x, 0.3, 9.1, 3.3), &layout).expect("a composite");
            let ends: Vec<_> = composite
                .parts
                .iter()
                .map(|part| (part.destination.0, part.destination.0 + part.destination.2))
                .collect();
            assert_eq!(ends.first().map(|end| end.0), Some(0), "{x}");
            assert_eq!(ends.last().map(|end| end.1), Some(composite.size.0), "{x}");
            for pair in ends.windows(2) {
                assert_eq!(pair[0].1, pair[1].0, "{x}: {ends:?}");
            }
        }
    }

    #[test]
    fn a_region_on_one_screen_off_the_pixel_grid_crops_as_the_screen_alone_does() {
        // A 1.5x screen whose origin, 1707 × 1.5, falls between two px: its
        // edges must round as its own crop rounds them, not shifted by half
        // a px, which made this image a column wider than the crop.
        let layout = [window((1707.0, 0.0), 1.0, 1.5, (400.0, 300.0))];
        let region = region(289.33203, 68.66797, 61.335938, 256.66406);
        let composite = composite(region, &layout).expect("a composite");
        let (_, _, width, height) = region.device_crop(1.5, (600, 450)).expect("a crop");
        assert_eq!((width, height), (92, 347));
        assert_eq!(composite.size, (width, height));
        assert_eq!(destinations(&composite), vec![(0, (0, 0, width, height))]);
    }

    #[test]
    fn a_window_a_unit_short_of_its_neighbour_leaves_no_transparent_seam() {
        // The first window ends at 1919, a unit short of the second screen:
        // rounding, which a drag ending on the second screen accepts.
        let layout = [
            window((0.0, 0.0), 1.0, 2.0, (1919.0, 1080.0)),
            window((1920.0, 0.0), 1.0, 2.0, (1920.0, 1080.0)),
        ];
        let composite = composite(region(1900.0, 0.0, 40.0, 10.0), &layout).expect("a composite");
        assert_eq!(composite.size, (80, 20));
        assert_eq!(
            destinations(&composite),
            vec![(0, (0, 0, 40, 20)), (1, (40, 0, 40, 20))]
        );
        // Each part's own pixels: 19 and 20 logical px at 2x.
        let image = stitch(&composite, &[filled(38, 20, RED), filled(40, 20, BLUE)]);
        assert!(image.pixels().all(|pixel| pixel.0[3] == 255));
    }

    #[test]
    fn overlapping_placements_are_not_stitched() {
        // Two windows on one place, as mirrored displays give: only the
        // screen the drag started on is used.
        let layout = [
            window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
            window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
        ];
        assert!(placements_overlap(&layout));
        let composite =
            composite(region_on_monitor(1, 10.0, 10.0, 20.0, 10.0), &layout).expect("a composite");
        assert_eq!(destinations(&composite), vec![(1, (0, 0, 20, 10))]);
    }

    #[test]
    fn screens_that_touch_or_overlap_by_rounding_do_not_count_as_overlapping() {
        assert!(!placements_overlap(&side_by_side()));
        let a_unit_over = [
            window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
            window((99.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
        ];
        assert!(!placements_overlap(&a_unit_over));
    }

    #[test]
    fn a_logical_frame_has_one_unit_per_logical_px_unless_the_scale_is_overridden() {
        // On a 1.5x screen, the window's own scale is the screen's.
        assert_eq!(frame_units(true, 1.5, 1.5), 1.0);
        // `SLINT_SCALE_FACTOR=2` on a 1x screen: two frame units per px.
        assert_eq!(frame_units(true, 2.0, 1.0), 2.0);
    }

    #[test]
    fn a_physical_frame_has_the_windows_scale_in_units_per_logical_px() {
        assert_eq!(frame_units(false, 2.0, 2.0), 2.0);
        assert_eq!(frame_units(false, 2.0, 1.0), 2.0);
    }

    #[test]
    fn a_click_on_a_screen_leaves_out_its_neighbours_edge() {
        // A whole 1x screen, next to a 2x one that begins a tenth of a px
        // early: that sliver is under a px, and must not make the image 2x.
        let layout = [
            window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
            window((99.9, 0.0), 1.0, 2.0, (100.0, 50.0)),
        ];
        let composite = composite(region(0.0, 0.0, 100.0, 50.0), &layout).expect("a composite");
        assert_eq!(composite.size, (100, 50));
        assert_eq!(destinations(&composite), vec![(0, (0, 0, 100, 50))]);
    }

    #[test]
    fn a_window_with_no_place_is_left_out() {
        let layout = [window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0)), None];
        let composite = composite(region(90.0, 0.0, 20.0, 10.0), &layout).expect("a composite");
        assert_eq!(destinations(&composite), vec![(0, (0, 0, 10, 10))]);
    }

    #[test]
    fn an_unusable_placement_is_left_out() {
        // `f32::max` and `f32::min` drop a NaN, so a NaN unit or size would
        // otherwise clip nothing and take the whole region.
        for broken in [
            window((100.0, 0.0), f32::NAN, 1.0, (100.0, 50.0)),
            window((100.0, 0.0), 1.0, 1.0, (f32::NAN, 50.0)),
            window((100.0, 0.0), 0.0, 1.0, (100.0, 50.0)),
            window((100.0, 0.0), 1.0, f32::NAN, (100.0, 50.0)),
            window((100.0, 0.0), 1.0, -1.0, (100.0, 50.0)),
            window((f32::INFINITY, 0.0), 1.0, 1.0, (100.0, 50.0)),
        ] {
            let layout = [window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0)), broken];
            let composite = composite(region(90.0, 0.0, 20.0, 10.0), &layout).expect("a composite");
            assert_eq!(
                destinations(&composite),
                vec![(0, (0, 0, 10, 10))],
                "{broken:?}"
            );
        }
    }

    #[test]
    fn a_region_on_no_screen_composes_nothing() {
        let layout = [window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0))];
        // Off the window, empty, on a window with no place, on no window.
        assert_eq!(composite(region(200.0, 0.0, 10.0, 10.0), &layout), None);
        assert_eq!(composite(region(10.0, 10.0, 0.0, 0.0), &layout), None);
        assert_eq!(composite(region(10.0, 10.0, 5.0, 5.0), &[None]), None);
        assert_eq!(composite(region(10.0, 10.0, 5.0, 5.0), &[]), None);
        let unusable = [window((0.0, 0.0), 1.0, 0.0, (100.0, 50.0))];
        assert_eq!(composite(region(10.0, 10.0, 5.0, 5.0), &unusable), None);
    }

    #[test]
    fn the_outline_is_drawn_on_each_screen_the_region_covers() {
        let dragged = region(1900.0, 10.0, 50.0, 20.0);
        let layout = side_by_side();
        assert_eq!(region_on(dragged, &layout, 0), Some(dragged));
        // Unclipped, in the second window's px.
        assert_eq!(
            region_on(dragged, &layout, 1),
            Some(region_on_monitor(1, -20.0, 10.0, 50.0, 20.0))
        );
    }

    #[test]
    fn a_screen_the_region_misses_has_no_outline() {
        let dragged = region(10.0, 10.0, 50.0, 20.0);
        assert_eq!(region_on(dragged, &side_by_side(), 1), None);
        assert_eq!(region_on(dragged, &side_by_side(), 2), None);
    }

    #[test]
    fn the_start_screen_has_the_outline_whatever_the_layout() {
        // Even a press's first, empty, region, and with no layout at all.
        let pressed = region(10.0, 10.0, 0.0, 0.0);
        assert_eq!(region_on(pressed, &[], 0), Some(pressed));
        assert_eq!(region_on(pressed, &side_by_side(), 0), Some(pressed));
    }

    /// A screen with two more on one place to its right, as mirrored
    /// displays give.
    fn mirrored_on_the_right() -> Vec<Option<Placement>> {
        vec![
            window((0.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
            window((100.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
            window((100.0, 0.0), 1.0, 1.0, (100.0, 50.0)),
        ]
    }

    #[test]
    fn overlapping_placements_have_the_outline_on_the_start_screen_only() {
        // As the copy, which uses the start screen alone.
        let dragged = region(80.0, 10.0, 40.0, 20.0);
        let layout = mirrored_on_the_right();
        assert_eq!(region_on(dragged, &layout, 0), Some(dragged));
        assert_eq!(region_on(dragged, &layout, 1), None);
        assert_eq!(region_on(dragged, &layout, 2), None);
    }

    #[test]
    fn overlapping_placements_are_reported_only_when_the_region_reaches_another_screen() {
        let layout = mirrored_on_the_right();
        assert!(leaves_out_overlapping_screens(
            region(80.0, 10.0, 40.0, 20.0),
            &layout
        ));
        assert!(!leaves_out_overlapping_screens(
            region(10.0, 10.0, 40.0, 20.0),
            &layout
        ));
        // A layout that can be trusted leaves nothing out.
        assert!(!leaves_out_overlapping_screens(
            region(1900.0, 10.0, 50.0, 20.0),
            &side_by_side()
        ));
    }

    #[test]
    fn a_drag_ending_on_a_placed_screen_lands() {
        let layout = side_by_side();
        assert!(lands_on_a_window(&layout, 0, (10.0, 10.0)));
        assert!(lands_on_a_window(&layout, 0, (2500.0, 500.0)));
        assert!(lands_on_a_window(&layout, 1, (-1000.0, 500.0)));
        // A unit past the far edge is rounding, not another screen.
        assert!(lands_on_a_window(&layout, 0, (3841.0, 1081.0)));
    }

    #[test]
    fn a_drag_ending_off_every_placed_screen_does_not_land() {
        // Past the only placed screen, onto one with no place.
        let layout = [window((0.0, 0.0), 1.0, 1.0, (1920.0, 1080.0)), None];
        assert!(!lands_on_a_window(&layout, 0, (2500.0, 500.0)));
        // In the gap of a layout that cannot be right.
        assert!(!lands_on_a_window(&offset_vertically(), 0, (150.0, 10.0)));
        // With the start window not placed at all.
        assert!(!lands_on_a_window(&[None], 0, (10.0, 10.0)));
    }

    #[test]
    fn the_gap_between_screens_is_transparent() {
        let composite =
            composite(region(50.0, 20.0, 100.0, 100.0), &offset_vertically()).expect("a composite");
        let image = stitch(&composite, &[filled(50, 80, RED), filled(50, 70, BLUE)]);
        assert_eq!(image.dimensions(), (100, 100));
        assert_eq!(image.get_pixel(10, 10), &Rgba(RED));
        assert_eq!(image.get_pixel(60, 40), &Rgba(BLUE));
        // Below the first screen, and above the second.
        assert_eq!(image.get_pixel(10, 90).0[3], 0);
        assert_eq!(image.get_pixel(60, 10).0[3], 0);
    }

    #[test]
    fn side_by_side_parts_stitch_into_one_unbroken_image() {
        let composite =
            composite(region(1900.0, 10.0, 50.0, 20.0), &side_by_side()).expect("a composite");
        let image = stitch(&composite, &[filled(20, 20, RED), filled(30, 20, BLUE)]);
        assert_eq!(image.get_pixel(19, 0), &Rgba(RED));
        assert_eq!(image.get_pixel(20, 0), &Rgba(BLUE));
        assert!(image.pixels().all(|pixel| pixel.0[3] == 255));
    }

    #[test]
    fn a_part_at_the_image_density_is_copied_pixel_for_pixel() {
        let part = Part {
            monitor: 0,
            region: region(0.0, 0.0, 3.0, 2.0),
            destination: (1, 0, 3, 2),
            upscale: false,
        };
        let composite = Composite {
            size: (4, 2),
            parts: vec![part],
        };
        let pixels = RgbaImage::from_fn(3, 2, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        let image = stitch(&composite, std::slice::from_ref(&pixels));
        for (x, y, pixel) in pixels.enumerate_pixels() {
            assert_eq!(image.get_pixel(x + 1, y), pixel);
        }
    }

    #[test]
    fn a_part_a_pixel_short_of_its_destination_repeats_its_edge() {
        let composite = Composite {
            size: (3, 2),
            parts: vec![Part {
                monitor: 0,
                region: region(0.0, 0.0, 3.0, 2.0),
                destination: (0, 0, 3, 2),
                upscale: false,
            }],
        };
        // Red then blue: the missing third column is blue again, and the
        // second stays blue rather than being smoothed half-way.
        let pixels = RgbaImage::from_fn(2, 2, |x, _| Rgba(if x == 0 { RED } else { BLUE }));
        let image = stitch(&composite, &[pixels]);
        assert_eq!(image.get_pixel(0, 1), &Rgba(RED));
        assert_eq!(image.get_pixel(1, 1), &Rgba(BLUE));
        assert_eq!(image.get_pixel(2, 1), &Rgba(BLUE));
    }

    #[test]
    fn a_less_dense_part_is_upscaled_to_fill_its_destination() {
        let composite = Composite {
            size: (4, 4),
            parts: vec![Part {
                monitor: 0,
                region: region(0.0, 0.0, 2.0, 2.0),
                destination: (0, 0, 4, 4),
                upscale: true,
            }],
        };
        // Red on the left, blue on the right: upscaled, each colour fills
        // its own half, where repeating the edge would give blue three
        // quarters of the destination.
        let pixels = RgbaImage::from_fn(2, 2, |x, _| Rgba(if x == 0 { RED } else { BLUE }));
        let image = stitch(&composite, &[pixels]);
        let Rgba([red, _, blue, _]) = *image.get_pixel(1, 0);
        assert!(red > blue, "{:?}", image.get_pixel(1, 0));
        let Rgba([red, _, blue, _]) = *image.get_pixel(2, 0);
        assert!(blue > red, "{:?}", image.get_pixel(2, 0));
    }
}
