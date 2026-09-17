//! Colour sampling and formatting for the eyedropper mode.

use std::collections::HashMap;

use crate::image::Rgba8;

/// A sampled colour with the representations the UI shows and copies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Hue in degrees, 0..359.
    pub h: u16,
    /// Saturation as a percentage, 0..100.
    pub s: u8,
    /// Lightness as a percentage, 0..100.
    pub l: u8,
}

impl Sample {
    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn rgb(&self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }

    pub fn hsl(&self) -> String {
        format!("hsl({}, {}%, {}%)", self.h, self.s, self.l)
    }

    /// All three notations, in the order the readout bubble stacks them.
    pub fn notations(&self) -> [String; 3] {
        [self.hex(), self.rgb(), self.hsl()]
    }

    /// The three notations on one line, as copied to the clipboard.
    pub fn summary(&self) -> String {
        self.notations().join(" ")
    }

    /// Builds a sample from RGB, deriving the HSL representation.
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        let (h, s, l) = rgb_to_hsl(r, g, b);
        Self { r, g, b, h, s, l }
    }
}

/// Caches the circular Gaussian kernels used for averaged sampling.
///
/// The radius only changes when the user drags the slider, so recomputing the
/// offsets on every cursor move would be waste.
#[derive(Default)]
pub struct KernelCache {
    kernels: HashMap<usize, Vec<(isize, isize, f32)>>,
}

impl KernelCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `(dx, dy, weight)` triples covering a disc of `radius` pixels.
    fn kernel(&mut self, radius: usize) -> &[(isize, isize, f32)] {
        self.kernels.entry(radius).or_insert_with(|| {
            if radius == 0 {
                return vec![(0, 0, 1.0)];
            }
            let r = radius as isize;
            let radius_sq = (radius * radius) as f32;
            let sigma = (radius as f32 / 2.0).max(0.5);
            let inv_two_sigma_sq = 1.0 / (2.0 * sigma * sigma);
            let mut kernel = Vec::new();
            for dy in -r..=r {
                for dx in -r..=r {
                    let dist_sq = (dx * dx + dy * dy) as f32;
                    // Circular, not square: a square kernel would bias the
                    // average toward the corners of the sampled area.
                    if dist_sq > radius_sq {
                        continue;
                    }
                    kernel.push((dx, dy, (-dist_sq * inv_two_sigma_sq).exp()));
                }
            }
            kernel
        })
    }

    /// Samples `image` at `(x, y)`, averaging over a Gaussian-weighted disc.
    ///
    /// A `radius` of 0 reads the single pixel under the cursor. Neighbours that
    /// fall outside the image are skipped rather than clamped, so sampling near
    /// a screen border does not smear the edge pixel across the average.
    pub fn sample(&mut self, image: &Rgba8, x: usize, y: usize, radius: usize) -> Sample {
        if radius == 0 || image.width() == 0 || image.height() == 0 {
            let [r, g, b, _] = image.pixel(x, y);
            return Sample::from_rgb(r, g, b);
        }

        // Neighbours outside the image are skipped, so a disc wider than the
        // image adds nothing. Clamping keeps the cached kernel sized by the
        // image rather than by whatever the caller passed, and keeps the
        // `radius * radius` below from overflowing.
        let radius = radius.min(image.width().max(image.height()));

        let (w, h) = (image.width(), image.height());
        let (mut sum_r, mut sum_g, mut sum_b, mut sum_w) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);

        for &(dx, dy, weight) in self.kernel(radius) {
            // Checked rather than `x as isize`, which wraps a large
            // out-of-range coordinate to a negative offset and then samples
            // real pixels at the opposite edge.
            let (Some(px), Some(py)) = (x.checked_add_signed(dx), y.checked_add_signed(dy))
            else {
                continue;
            };
            if px >= w || py >= h {
                continue;
            }
            let [r, g, b, _] = image.pixel(px, py);
            sum_r += weight * r as f32;
            sum_g += weight * g as f32;
            sum_b += weight * b as f32;
            sum_w += weight;
        }

        if sum_w <= 0.0 {
            let [r, g, b, _] = image.pixel(x, y);
            return Sample::from_rgb(r, g, b);
        }

        Sample::from_rgb(
            (sum_r / sum_w).round().clamp(0.0, 255.0) as u8,
            (sum_g / sum_w).round().clamp(0.0, 255.0) as u8,
            (sum_b / sum_w).round().clamp(0.0, 255.0) as u8,
        )
    }
}

/// Converts RGB to HSL, returning `(hue degrees, saturation %, lightness %)`.
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (u16, u8, u8) {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;

    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let lightness = (max + min) / 2.0;

    if (max - min).abs() < f32::EPSILON {
        // Achromatic: hue is undefined, reported as 0.
        return (0, 0, (lightness * 100.0).round() as u8);
    }

    let delta = max - min;
    let saturation = if lightness > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };

    let hue = if max == rf {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if max == gf {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };
    let hue = hue.rem_euclid(360.0);

    (
        hue.round() as u16 % 360,
        (saturation * 100.0).round() as u8,
        (lightness * 100.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, rgb: [u8; 3]) -> Rgba8 {
        let mut pixels = Vec::with_capacity(width * height * 4);
        for _ in 0..width * height {
            pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        Rgba8::from_raw(width, height, pixels).expect("valid buffer")
    }

    #[test]
    fn formats_match_the_expected_css_notations() {
        let sample = Sample::from_rgb(230, 25, 94);
        assert_eq!(sample.hex(), "#E6195E");
        assert_eq!(sample.rgb(), "rgb(230, 25, 94)");
        assert!(sample.hsl().starts_with("hsl(340, 80%, 50%)"), "{}", sample.hsl());
    }

    #[test]
    fn hsl_conversion_covers_primaries_and_greys() {
        assert_eq!(rgb_to_hsl(255, 0, 0), (0, 100, 50));
        assert_eq!(rgb_to_hsl(0, 255, 0), (120, 100, 50));
        assert_eq!(rgb_to_hsl(0, 0, 255), (240, 100, 50));
        assert_eq!(rgb_to_hsl(255, 255, 255), (0, 0, 100));
        assert_eq!(rgb_to_hsl(0, 0, 0), (0, 0, 0));
        assert_eq!(rgb_to_hsl(128, 128, 128), (0, 0, 50));
    }

    #[test]
    fn hue_stays_within_range_for_magenta_wraparound() {
        let (h, _, _) = rgb_to_hsl(255, 0, 128);
        assert!(h < 360, "hue {h} out of range");
    }

    #[test]
    fn zero_radius_reads_a_single_pixel() {
        let mut pixels = Vec::new();
        for y in 0..2u8 {
            for x in 0..2u8 {
                pixels.extend_from_slice(&[x * 100, y * 100, 0, 255]);
            }
        }
        let image = Rgba8::from_raw(2, 2, pixels).expect("valid buffer");
        let mut cache = KernelCache::new();

        let sample = cache.sample(&image, 1, 1, 0);
        assert_eq!((sample.r, sample.g, sample.b), (100, 100, 0));
    }

    #[test]
    fn a_radius_wider_than_the_image_is_clamped() {
        let image = solid(8, 8, [30, 60, 90]);
        let mut cache = KernelCache::new();

        // Must not overflow, allocate without bound, or degrade into the
        // single-pixel fallback: it still averages the whole image.
        let sample = cache.sample(&image, 4, 4, usize::MAX);
        assert_eq!((sample.r, sample.g, sample.b), (30, 60, 90));

        // The cache is keyed by the clamped radius, not the caller's number.
        assert!(cache.kernels.contains_key(&8), "clamped kernel not cached");
        assert!(
            !cache.kernels.contains_key(&usize::MAX),
            "cached a kernel for the unclamped radius"
        );
    }

    #[test]
    fn kernel_weights_follow_the_gaussian() {
        let mut cache = KernelCache::new();
        let kernel = cache.kernel(2).to_vec();
        let weight = |dx: isize, dy: isize| {
            kernel
                .iter()
                .find(|(kx, ky, _)| *kx == dx && *ky == dy)
                .map(|(_, _, w)| *w)
                .expect("offset missing from kernel")
        };

        // sigma is radius/2, so the weights fall off as exp(-dist_sq / 2).
        // Pinned as literals: a flat kernel or a different sigma must fail.
        assert!((weight(0, 0) - 1.0).abs() < 1e-5, "centre must be 1.0");
        assert!((weight(1, 0) - 0.606_531).abs() < 1e-5);
        assert!((weight(1, 1) - 0.367_879).abs() < 1e-5);
        assert!((weight(0, 2) - 0.135_335).abs() < 1e-5);
    }

    #[test]
    fn out_of_image_neighbours_are_skipped_not_clamped() {
        // `white_cols` leftmost columns white, the rest black.
        fn striped(width: usize, white_cols: usize) -> Rgba8 {
            let mut pixels = Vec::new();
            for _ in 0..16 {
                for x in 0..width {
                    let v = if x < white_cols { 255 } else { 0 };
                    pixels.extend_from_slice(&[v, v, v, 255]);
                }
            }
            Rgba8::from_raw(width, 16, pixels).expect("valid buffer")
        }

        let mut cache = KernelCache::new();

        // Sampling the left edge: the four columns left of x=0 do not exist.
        let edge = cache.sample(&striped(16, 1), 0, 8, 4);

        // The same neighbourhood with those columns actually present,
        // replicating column 0 - exactly what clamping would have read. If
        // `sample` clamped rather than skipping, these two would agree.
        let padded = cache.sample(&striped(20, 5), 4, 8, 4);

        assert_ne!(
            (edge.r, edge.g, edge.b),
            (padded.r, padded.g, padded.b),
            "out-of-image neighbours were clamped into the average, not skipped"
        );
    }

    #[test]
    fn averaging_a_solid_image_returns_that_colour() {
        let image = solid(16, 16, [30, 60, 90]);
        let mut cache = KernelCache::new();
        for radius in [0, 1, 4, 7] {
            let sample = cache.sample(&image, 8, 8, radius);
            assert_eq!(
                (sample.r, sample.g, sample.b),
                (30, 60, 90),
                "radius {radius} skewed a solid colour"
            );
        }
    }

    #[test]
    fn averaging_blends_a_split_image_toward_the_centre() {
        // Left half black, right half white; sampling the seam should land
        // between the two, and the single-pixel read should not.
        let mut pixels = Vec::new();
        for _ in 0..32 {
            for x in 0..32 {
                let v = if x < 16 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let image = Rgba8::from_raw(32, 32, pixels).expect("valid buffer");
        let mut cache = KernelCache::new();

        assert_eq!(cache.sample(&image, 16, 16, 0).r, 255);
        let blended = cache.sample(&image, 16, 16, 8).r;
        assert!((60..=200).contains(&blended), "unexpected blend {blended}");
    }

    #[test]
    fn sampling_near_a_border_skips_missing_neighbours() {
        let image = solid(8, 8, [200, 100, 50]);
        let mut cache = KernelCache::new();
        let sample = cache.sample(&image, 0, 0, 6);
        assert_eq!((sample.r, sample.g, sample.b), (200, 100, 50));
    }

    #[test]
    fn out_of_bounds_sample_falls_back_to_opaque_black() {
        let image = solid(4, 4, [10, 20, 30]);
        let mut cache = KernelCache::new();
        let sample = cache.sample(&image, 99, 99, 0);
        assert_eq!((sample.r, sample.g, sample.b), (0, 0, 0));
    }

    #[test]
    fn a_wildly_out_of_range_centre_does_not_wrap_into_the_image() {
        // `x as isize` turned usize::MAX into -1, so a positive kernel offset
        // landed back on real pixels at the left edge. The existing
        // out-of-bounds test uses radius 0 and takes the early-return path,
        // so it never reached this.
        let image = solid(4, 4, [255, 255, 255]);
        let mut cache = KernelCache::new();

        let sample = cache.sample(&image, usize::MAX, 1, 2);
        assert_eq!((sample.r, sample.g, sample.b), (0, 0, 0));

        let sample = cache.sample(&image, 1, usize::MAX, 2);
        assert_eq!((sample.r, sample.g, sample.b), (0, 0, 0));
    }

    #[test]
    fn kernel_is_circular_and_cached() {
        let mut cache = KernelCache::new();
        let len = cache.kernel(4).len();
        // A 9x9 square would be 81 entries; a disc of radius 4 is smaller.
        assert!(len < 81, "kernel is not circular: {len}");
        assert!(len > 40, "kernel is suspiciously small: {len}");
        assert_eq!(cache.kernel(4).len(), len, "cached kernel changed");
        assert_eq!(cache.kernel(0), &[(0, 0, 1.0)]);
    }
}
