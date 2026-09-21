//! Image buffers and the pixel operations the app needs.
//!
//! The buffers are the `image` crate's. slint already links `image`, so using
//! it costs nothing in binary size and saves hand-rolling (and hand-testing)
//! bounds and overflow checks. Only the operations with app-specific
//! behaviour live here.

pub use ::image::{GrayImage, Rgba, RgbaImage};

/// Returns the RGBA pixel at `(x, y)`, or opaque black when out of bounds.
///
/// The black sentinel is inherited from the hand-rolled buffer this replaced.
/// Whether out-of-range reads should instead clamp, or return `None` and make
/// the caller decide, is an open question, so the behaviour is preserved here
/// rather than changed in passing.
pub fn pixel_or_black(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    image.get_pixel_checked(x, y).map_or([0, 0, 0, 255], |p| p.0)
}

/// Rec. 601 luma, matching the weights OpenCV's `COLOR_RGB2GRAY` uses.
///
/// Deliberately not `image::imageops::grayscale`, which applies Rec. 709
/// (0.2126/0.7152/0.0722). The Python implementation feeds this straight into
/// Canny with thresholds tuned against Rec. 601, so changing the basis would
/// shift every gradient and silently detune edge detection.
pub fn to_gray(image: &RgbaImage) -> GrayImage {
    let mut out = GrayImage::new(image.width(), image.height());
    for (dst, src) in out.pixels_mut().zip(image.pixels()) {
        let [r, g, b, _] = src.0;
        // Fixed-point 0.299 / 0.587 / 0.114 with rounding.
        let luma = (r as u32 * 19595 + g as u32 * 38470 + b as u32 * 7471 + 32768) >> 16;
        dst.0 = [luma as u8];
    }
    out
}

/// Shrinks by an integer `factor`, averaging each source block.
///
/// Box averaging, not nearest-neighbour: this backs the on-screen backdrop,
/// where dropped pixels alias badly on text. A factor of 0 or 1 clones.
pub fn downscale(image: &RgbaImage, factor: u32) -> RgbaImage {
    if factor <= 1 {
        return image.clone();
    }
    // Round up: partial blocks are clipped against the source bounds below, so
    // flooring would silently drop the ragged right/bottom edge.
    let width = image.width().div_ceil(factor).max(1);
    let height = image.height().div_ceil(factor).max(1);
    let mut out = RgbaImage::new(width, height);

    for (x, y, dst) in out.enumerate_pixels_mut() {
        let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
        let y1 = (y + 1).saturating_mul(factor).min(image.height());
        let x1 = (x + 1).saturating_mul(factor).min(image.width());
        for sy in y.saturating_mul(factor)..y1 {
            for sx in x.saturating_mul(factor)..x1 {
                let [pr, pg, pb, pa] = image.get_pixel(sx, sy).0;
                r += pr as u32;
                g += pg as u32;
                b += pb as u32;
                a += pa as u32;
                n += 1;
            }
        }
        let n = n.max(1);
        dst.0 = [(r / n) as u8, (g / n) as u8, (b / n) as u8, (a / n) as u8];
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(rgba))
    }

    #[test]
    fn out_of_bounds_pixel_reads_are_opaque_black() {
        let img = solid(2, 2, [10, 20, 30, 40]);
        assert_eq!(pixel_or_black(&img, 0, 0), [10, 20, 30, 40]);
        assert_eq!(pixel_or_black(&img, 2, 0), [0, 0, 0, 255]);
        assert_eq!(pixel_or_black(&img, 0, 2), [0, 0, 0, 255]);
    }

    #[test]
    fn downscaling_averages_each_block() {
        // 2x2 of distinct greys; a factor of 2 collapses it to their mean.
        let pixels = vec![
            0, 0, 0, 255, 100, 100, 100, 255, // row 0
            200, 200, 200, 255, 255, 255, 255, 255, // row 1
        ];
        let image = RgbaImage::from_raw(2, 2, pixels).expect("valid buffer");
        let small = downscale(&image, 2);

        assert_eq!(small.dimensions(), (1, 1));
        // (0 + 100 + 200 + 255) / 4 = 138
        assert_eq!(small.get_pixel(0, 0).0, [138, 138, 138, 255]);
    }

    #[test]
    fn downscaling_keeps_the_ragged_edge_column() {
        // 5 wide at factor 2: the last column is its own partial block. If the
        // output width floored to 2 instead of 3, this column would vanish.
        let mut pixels = Vec::new();
        for x in 0..5 {
            let red = if x == 4 { 255 } else { 0 };
            pixels.extend_from_slice(&[red, 0, 0, 255]);
        }
        let small = downscale(&RgbaImage::from_raw(5, 1, pixels).expect("valid buffer"), 2);

        assert_eq!(small.dimensions(), (3, 1));
        assert_eq!(small.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(small.get_pixel(2, 0).0, [255, 0, 0, 255], "edge column dropped");
    }

    #[test]
    fn downscaling_by_one_or_zero_is_a_no_op() {
        let image = solid(4, 4, [1, 2, 3, 255]);
        assert_eq!(downscale(&image, 1), image);
        assert_eq!(downscale(&image, 0), image);
    }

    #[test]
    fn downscaling_a_ragged_size_never_produces_an_empty_image() {
        // 5x3 at factor 2 does not divide evenly; partial blocks still count,
        // so the output rounds up rather than cropping the odd edge.
        let image = solid(5, 3, [10, 20, 30, 255]);
        let small = downscale(&image, 2);
        assert_eq!(small.dimensions(), (3, 2));
        assert_eq!(small.get_pixel(0, 0).0, [10, 20, 30, 255]);

        // A factor larger than the image still yields at least one pixel.
        assert_eq!(downscale(&image, 99).dimensions(), (1, 1));
    }

    #[test]
    fn grayscale_uses_rec_601_not_the_crate_default() {
        let luma = |rgb: [u8; 3]| {
            to_gray(&solid(1, 1, [rgb[0], rgb[1], rgb[2], 255]))
                .get_pixel(0, 0)
                .0[0]
        };
        assert_eq!(luma([255, 255, 255]), 255);
        assert_eq!(luma([0, 0, 0]), 0);
        // Rec. 601: 0.299 / 0.587 / 0.114. Rec. 709 would give 54 / 182 / 18.
        assert_eq!(luma([255, 0, 0]), 76);
        assert_eq!(luma([0, 255, 0]), 150);
        assert_eq!(luma([0, 0, 255]), 29);
    }
}
