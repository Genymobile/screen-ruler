//! Canny edge detection.
//!
//! Hand-rolled instead of binding OpenCV: the ruler needs one fixed pipeline
//! (blur -> Sobel -> non-maximum suppression -> hysteresis), and dropping the
//! dependency is what keeps this a single self-contained binary.

use crate::image::GrayImage;

/// Sensitivity is a user-facing 0..100 dial; these anchor the Canny
/// hysteresis thresholds it maps onto, carried over so existing muscle memory
/// for the slider still holds.
const LOW_AT_100: f32 = 5.0;
const HIGH_AT_100: f32 = 25.0;
const LOW_AT_0: f32 = 80.0;
const HIGH_AT_0: f32 = 220.0;

/// Default dial position, matching the Python defaults of
/// `--threshold-low 16 --threshold-high 54`.
pub const DEFAULT_SENSITIVITY: f32 = 85.0;

/// A boolean edge mask in device pixels, `true` where an edge was detected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeMap {
    width: u32,
    height: u32,
    /// Counted once here because the map is immutable and the controls panel
    /// reports the total on every frame.
    count: usize,
    data: Vec<bool>,
}

impl EdgeMap {
    pub fn new(width: u32, height: u32, data: Vec<bool>) -> Option<Self> {
        let expected = (width as usize).checked_mul(height as usize)?;
        if data.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            count: data.iter().filter(|v| **v).count(),
            data,
        })
    }

    pub fn blank(width: u32, height: u32) -> Self {
        let len = (width as usize)
            .checked_mul(height as usize)
            .expect("edge map dimensions overflow");
        Self {
            width,
            height,
            count: 0,
            data: vec![false; len],
        }
    }

    /// Builds a map from ASCII art where `#` marks an edge pixel.
    #[cfg(test)]
    pub fn from_ascii(rows: &[&str]) -> Self {
        let height = rows.len() as u32;
        let width = rows.first().map_or(0, |row| row.len()) as u32;
        let mut data = Vec::with_capacity(rows.len() * width as usize);
        for row in rows {
            assert_eq!(row.len() as u32, width, "ragged test fixture");
            data.extend(row.chars().map(|c| c == '#'));
        }
        Self::new(width, height, data).expect("valid map")
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    #[inline]
    fn index(&self, x: u32, y: u32) -> usize {
        y as usize * self.width as usize + x as usize
    }

    /// Edge test that treats out-of-bounds coordinates as "no edge".
    #[inline]
    pub fn at(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        self.data[self.index(x, y)]
    }

    pub fn as_slice(&self) -> &[bool] {
        &self.data
    }

    /// Number of edge pixels.
    pub fn count(&self) -> usize {
        self.count
    }

    /// True when any pixel in the inclusive column span of a single row is an
    /// edge. A span running past the border is clamped, so callers may pass an
    /// unclamped search window.
    #[inline]
    pub fn any_in_row(&self, y: u32, x0: u32, x1: u32) -> bool {
        if y >= self.height || x0 > x1 || x0 >= self.width {
            return false;
        }
        let x1 = x1.min(self.width - 1);
        let start = self.index(x0, y);
        self.data[start..=start + (x1 - x0) as usize]
            .iter()
            .any(|v| *v)
    }

    /// True when any pixel in the inclusive row span of a single column is an
    /// edge. Clamped like [`EdgeMap::any_in_row`].
    #[inline]
    pub fn any_in_column(&self, x: u32, y0: u32, y1: u32) -> bool {
        if x >= self.width || y0 > y1 || y0 >= self.height {
            return false;
        }
        let y1 = y1.min(self.height - 1);
        (y0..=y1).any(|y| self.data[self.index(x, y)])
    }
}

/// Maps the 0..100 sensitivity dial onto `(low, high)` Canny thresholds.
///
/// Higher sensitivity means lower thresholds, so more edges survive.
pub fn sensitivity_to_thresholds(sensitivity: f32) -> (u16, u16) {
    let s = sensitivity.clamp(0.0, 100.0);
    let low = (LOW_AT_100 + (100.0 - s) * ((LOW_AT_0 - LOW_AT_100) / 100.0)).round() as u16;
    let high = (HIGH_AT_100 + (100.0 - s) * ((HIGH_AT_0 - HIGH_AT_100) / 100.0)).round() as u16;
    // Hysteresis is meaningless unless the upper threshold is strictly higher.
    let high = if high <= low {
        (low + 1).min(255)
    } else {
        high
    };
    (low, high)
}

/// Inverse of [`sensitivity_to_thresholds`], averaging both estimates so
/// CLI-supplied thresholds land the slider in a sensible spot.
pub fn thresholds_to_sensitivity(low: u16, high: u16) -> f32 {
    let low_scale = (LOW_AT_0 - LOW_AT_100) / 100.0;
    let high_scale = (HIGH_AT_0 - HIGH_AT_100) / 100.0;
    let from_low = 100.0 - ((low as f32 - LOW_AT_100) / low_scale);
    let from_high = 100.0 - ((high as f32 - HIGH_AT_100) / high_scale);
    ((from_low + from_high) / 2.0).clamp(0.0, 100.0)
}

/// Runs Canny edge detection over `gray`.
///
/// `low`/`high` are the hysteresis thresholds applied to the gradient
/// magnitude. Images narrower or shorter than the 3x3 Sobel window yield a
/// blank map rather than an error, so a degenerate monitor never aborts
/// start-up.
pub fn canny(gray: &GrayImage, low: u16, high: u16) -> EdgeMap {
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    if w < 3 || h < 3 {
        return EdgeMap::blank(gray.width(), gray.height());
    }

    // Hysteresis needs a strictly higher upper threshold. Cap the lower one
    // first so `low == u16::MAX` cannot overflow the increment.
    let (low, high) = if high <= low {
        let low = low.min(u16::MAX - 1);
        (low, low + 1)
    } else {
        (low, high)
    };
    let blurred = gaussian_blur_5(gray);
    let (magnitude, direction) = sobel(&blurred, w, h);
    let thin = non_maximum_suppression(&magnitude, &direction, w, h);
    hysteresis(&thin, w, h, low, high)
}

/// Separable 5-tap binomial blur (`[1 4 6 4 1] / 16`, sigma ~= 1.0).
///
/// Suppresses font anti-aliasing and wallpaper noise that would otherwise
/// produce a haze of spurious edges. Borders clamp rather than wrap.
fn gaussian_blur_5(gray: &GrayImage) -> Vec<u16> {
    const K: [u32; 5] = [1, 4, 6, 4, 1];
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    let src = gray.as_raw();

    let mut horizontal = vec![0u16; w * h];
    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            let mut acc = 0u32;
            for (i, k) in K.iter().enumerate() {
                let sx = (x as isize + i as isize - 2).clamp(0, w as isize - 1) as usize;
                acc += k * src[row + sx] as u32;
            }
            horizontal[row + x] = (acc / 16) as u16;
        }
    }

    let mut out = vec![0u16; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0u32;
            for (i, k) in K.iter().enumerate() {
                let sy = (y as isize + i as isize - 2).clamp(0, h as isize - 1) as usize;
                acc += k * horizontal[sy * w + x] as u32;
            }
            out[y * w + x] = (acc / 16) as u16;
        }
    }
    out
}

/// Direction bucket used by non-maximum suppression.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum Dir {
    /// Horizontal gradient: compare against left/right neighbours.
    EastWest = 0,
    NorthEastSouthWest = 1,
    /// Vertical gradient: compare against up/down neighbours.
    NorthSouth = 2,
    NorthWestSouthEast = 3,
}

/// 3x3 Sobel returning L2 gradient magnitude and a quantised direction.
fn sobel(src: &[u16], w: usize, h: usize) -> (Vec<u16>, Vec<Dir>) {
    let mut magnitude = vec![0u16; w * h];
    let mut direction = vec![Dir::EastWest; w * h];

    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let idx = y * w + x;
            let tl = src[idx - w - 1] as i32;
            let tc = src[idx - w] as i32;
            let tr = src[idx - w + 1] as i32;
            let ml = src[idx - 1] as i32;
            let mr = src[idx + 1] as i32;
            let bl = src[idx + w - 1] as i32;
            let bc = src[idx + w] as i32;
            let br = src[idx + w + 1] as i32;

            let gx = (tr + 2 * mr + br) - (tl + 2 * ml + bl);
            let gy = (bl + 2 * bc + br) - (tl + 2 * tc + tr);

            let mag = ((gx * gx + gy * gy) as f32).sqrt();
            magnitude[idx] = mag.min(u16::MAX as f32) as u16;
            direction[idx] = quantise_direction(gx, gy);
        }
    }

    (magnitude, direction)
}

/// Buckets the gradient angle into the four 45-degree neighbour axes.
fn quantise_direction(gx: i32, gy: i32) -> Dir {
    // tan(22.5 deg) ~= 0.414, tan(67.5 deg) ~= 2.414; integer ratios avoid an
    // atan2 per pixel.
    let ax = gx.unsigned_abs() as u64;
    let ay = gy.unsigned_abs() as u64;

    if ay * 1000 < ax * 414 {
        Dir::EastWest
    } else if ax * 1000 < ay * 414 {
        Dir::NorthSouth
    } else if (gx >= 0) == (gy >= 0) {
        Dir::NorthWestSouthEast
    } else {
        Dir::NorthEastSouthWest
    }
}

/// Keeps only gradient maxima along the gradient direction, thinning ridges to
/// one pixel.
fn non_maximum_suppression(magnitude: &[u16], direction: &[Dir], w: usize, h: usize) -> Vec<u16> {
    let mut out = vec![0u16; w * h];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let idx = y * w + x;
            let m = magnitude[idx];
            if m == 0 {
                continue;
            }
            let (a, b) = match direction[idx] {
                Dir::EastWest => (magnitude[idx - 1], magnitude[idx + 1]),
                Dir::NorthSouth => (magnitude[idx - w], magnitude[idx + w]),
                Dir::NorthWestSouthEast => (magnitude[idx - w - 1], magnitude[idx + w + 1]),
                Dir::NorthEastSouthWest => (magnitude[idx - w + 1], magnitude[idx + w - 1]),
            };
            if m >= a && m >= b {
                out[idx] = m;
            }
        }
    }
    out
}

/// Double threshold plus connectivity-driven edge tracking.
///
/// Pixels above `high` seed the output; pixels above `low` join it only when
/// reachable from a seed through 8-connectivity.
fn hysteresis(thin: &[u16], w: usize, h: usize, low: u16, high: u16) -> EdgeMap {
    let mut out = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();

    // A zero gradient is the absence of an edge, so zero-magnitude pixels
    // never qualify however low the thresholds are. Without this, `low == 0`
    // makes every pixel eligible and one strong seed floods the whole image.
    for (idx, m) in thin.iter().enumerate() {
        if *m > 0 && *m >= high {
            out[idx] = true;
            stack.push(idx);
        }
    }

    while let Some(idx) = stack.pop() {
        let x = idx % w;
        let y = idx / w;
        let x0 = x.saturating_sub(1);
        let x1 = (x + 1).min(w - 1);
        let y0 = y.saturating_sub(1);
        let y1 = (y + 1).min(h - 1);
        for ny in y0..=y1 {
            for nx in x0..=x1 {
                let n = ny * w + nx;
                if !out[n] && thin[n] > 0 && thin[n] >= low {
                    out[n] = true;
                    stack.push(n);
                }
            }
        }
    }

    EdgeMap::new(w as u32, h as u32, out).expect("hysteresis preserves dimensions")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Luma;

    /// Builds a grayscale image from a closure over `(x, y)`.
    fn gray_from(width: u32, height: u32, f: impl Fn(u32, u32) -> u8) -> GrayImage {
        GrayImage::from_fn(width, height, |x, y| Luma([f(x, y)]))
    }

    #[test]
    fn sensitivity_mapping_is_monotonic_and_ordered() {
        let (low_max, high_max) = sensitivity_to_thresholds(100.0);
        let (low_min, high_min) = sensitivity_to_thresholds(0.0);
        assert_eq!((low_max, high_max), (5, 25));
        assert_eq!((low_min, high_min), (80, 220));

        // More sensitivity must never mean higher thresholds.
        let mut previous = sensitivity_to_thresholds(0.0);
        for step in 1..=100 {
            let current = sensitivity_to_thresholds(step as f32);
            assert!(current.0 <= previous.0, "low threshold rose at {step}");
            assert!(current.1 <= previous.1, "high threshold rose at {step}");
            assert!(current.1 > current.0, "thresholds crossed at {step}");
            previous = current;
        }
    }

    #[test]
    fn sensitivity_clamps_out_of_range_input() {
        assert_eq!(
            sensitivity_to_thresholds(-40.0),
            sensitivity_to_thresholds(0.0)
        );
        assert_eq!(
            sensitivity_to_thresholds(140.0),
            sensitivity_to_thresholds(100.0)
        );
    }

    #[test]
    fn thresholds_round_trip_through_sensitivity() {
        for sensitivity in [0.0, 12.5, 50.0, 85.0, 100.0] {
            let (low, high) = sensitivity_to_thresholds(sensitivity);
            let recovered = thresholds_to_sensitivity(low, high);
            assert!(
                (recovered - sensitivity).abs() < 1.0,
                "{sensitivity} round-tripped to {recovered}"
            );
        }
    }

    #[test]
    fn default_sensitivity_matches_legacy_cli_thresholds() {
        // The Python implementation shipped --threshold-low 16 --threshold-high 54.
        let recovered = thresholds_to_sensitivity(16, 54);
        assert!(
            (recovered - DEFAULT_SENSITIVITY).abs() < 1.0,
            "got {recovered}"
        );
    }

    #[test]
    fn flat_image_has_no_edges() {
        let flat = gray_from(32, 32, |_, _| 128);
        assert_eq!(canny(&flat, 5, 25).count(), 0);
    }

    #[test]
    fn detects_a_vertical_step_edge_and_thins_it() {
        // Left half black, right half white: one edge ridge near x == 16.
        let img = gray_from(32, 32, |x, _| if x < 16 { 0 } else { 255 });
        let edges = canny(&img, 5, 25);

        // Away from the blurred top/bottom borders, each row crosses the edge
        // in a narrow band rather than a thick smear.
        for y in 6..26 {
            let hits: Vec<u32> = (0..32).filter(|x| edges.at(*x, y)).collect();
            assert!(!hits.is_empty(), "row {y} found no edge");
            assert!(hits.len() <= 2, "row {y} edge not thinned: {hits:?}");
            for x in &hits {
                assert!((14..=17).contains(x), "row {y} edge at unexpected x={x}");
            }
        }
    }

    #[test]
    fn detects_a_horizontal_step_edge() {
        let img = gray_from(32, 32, |_, y| if y < 16 { 0 } else { 255 });
        let edges = canny(&img, 5, 25);
        for x in 6..26 {
            let hits: Vec<u32> = (0..32).filter(|y| edges.at(x, *y)).collect();
            assert!(!hits.is_empty(), "column {x} found no edge");
            assert!(hits.len() <= 2, "column {x} edge not thinned: {hits:?}");
        }
    }

    #[test]
    fn higher_thresholds_never_add_edges() {
        // A gradient ramp yields weak edges that the upper threshold prunes.
        let img = gray_from(64, 64, |x, y| ((x * 4 + y) % 256) as u8);
        let permissive = canny(&img, 5, 25).count();
        let strict = canny(&img, 80, 220).count();
        assert!(strict <= permissive, "{strict} > {permissive}");
    }

    #[test]
    fn zero_thresholds_do_not_flood_the_image() {
        // A small square on a flat field. Zero-magnitude pixels satisfy
        // `>= 0`, so without excluding them one strong seed flood-fills the
        // whole image through 8-connectivity: this returned 1024/1024.
        let img = gray_from(32, 32, |x, y| {
            if (14..18).contains(&x) && (14..18).contains(&y) {
                255
            } else {
                0
            }
        });

        let sane = canny(&img, 5, 25).count();
        for (low, high) in [(0, 1), (0, 0), (1, 0)] {
            let count = canny(&img, low, high).count();
            assert_eq!(
                count, sane,
                "thresholds ({low}, {high}) found {count} edges, expected {sane}"
            );
            assert!(count < 32 * 32, "({low}, {high}) marked the whole image");
        }
    }

    #[test]
    fn extreme_thresholds_do_not_overflow() {
        // `high <= low` bumps the upper threshold, which overflowed u16 when
        // the caller passed low == u16::MAX.
        let img = gray_from(16, 16, |x, _| if x < 8 { 0 } else { 255 });
        for (low, high) in [(u16::MAX, 0), (u16::MAX, u16::MAX), (u16::MAX - 1, 0)] {
            let map = canny(&img, low, high);
            assert_eq!(map.count(), 0, "({low}, {high}) found impossible edges");
        }
    }

    #[test]
    fn degenerate_images_yield_a_blank_map_instead_of_panicking() {
        let tiny = gray_from(2, 2, |_, _| 255);
        let edges = canny(&tiny, 5, 25);
        assert!(edges.is_empty() || edges.count() == 0);
        assert_eq!((edges.width(), edges.height()), (2, 2));
    }

    #[test]
    fn span_queries_respect_bounds() {
        let mut data = vec![false; 4 * 4];
        // One edge pixel at (x = 2, y = 1) in a 4x4 map.
        data[4 + 2] = true;
        let map = EdgeMap::new(4, 4, data).expect("valid map");

        assert!(map.any_in_row(1, 0, 3));
        assert!(map.any_in_row(1, 2, 2));
        assert!(!map.any_in_row(1, 0, 1));
        assert!(!map.any_in_row(0, 0, 3));
        assert!(map.any_in_column(2, 0, 3));
        assert!(!map.any_in_column(3, 0, 3));

        // Out-of-range queries answer "no edge" rather than panicking.
        assert!(!map.any_in_row(9, 0, 3));
        assert!(!map.any_in_column(9, 0, 3));
        assert!(!map.at(9, 9));

        // A span running past the right/bottom border is clamped to the map
        // rather than rejected, so callers can pass an unclamped search window.
        assert!(map.any_in_row(1, 0, 99));
        assert!(map.any_in_column(2, 0, 99));
    }

    #[test]
    fn ascii_fixtures_round_trip() {
        let map = EdgeMap::from_ascii(&[".#..", "..#.", "...."]);
        assert_eq!((map.width(), map.height()), (4, 3));
        assert_eq!(map.count(), 2);
        assert!(map.at(1, 0) && map.at(2, 1));
        assert!(!map.at(0, 0));
    }
}
