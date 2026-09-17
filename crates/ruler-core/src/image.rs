//! Minimal image buffers.
//!
//! Hand-rolled rather than pulling the `image` crate into the public surface:
//! the app only ever needs an RGBA8 rectangle and a grayscale projection of it.

/// A tightly packed RGBA8 image in *device* pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    width: usize,
    height: usize,
    /// `width * height * 4` bytes, row-major, no padding.
    pixels: Vec<u8>,
}

/// A tightly packed 8-bit grayscale image in *device* pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gray8 {
    width: usize,
    height: usize,
    /// `width * height` bytes, row-major, no padding.
    pixels: Vec<u8>,
}

impl Rgba8 {
    /// Wraps raw RGBA bytes, rejecting buffers whose length disagrees with the
    /// stated dimensions.
    pub fn from_raw(width: usize, height: usize, pixels: Vec<u8>) -> Option<Self> {
        let expected = width.checked_mul(height).and_then(|n| n.checked_mul(4))?;
        if width == 0 || height == 0 || pixels.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns the RGBA pixel at `(x, y)`, or opaque black when out of bounds.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 255];
        }
        let i = (y * self.width + x) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// Rec. 601 luma, matching the weights OpenCV's `COLOR_RGB2GRAY` uses.
    pub fn to_gray(&self) -> Gray8 {
        let mut out = vec![0u8; self.width * self.height];
        for (dst, src) in out.iter_mut().zip(self.pixels.chunks_exact(4)) {
            let r = src[0] as u32;
            let g = src[1] as u32;
            let b = src[2] as u32;
            // Fixed-point 0.299 / 0.587 / 0.114 with rounding.
            *dst = ((r * 19595 + g * 38470 + b * 7471 + 32768) >> 16) as u8;
        }
        Gray8 {
            width: self.width,
            height: self.height,
            pixels: out,
        }
    }

    /// Shrinks by an integer `factor`, averaging each source block.
    ///
    /// Box averaging, not nearest-neighbour: this backs the on-screen backdrop,
    /// where dropped pixels alias badly on text. A factor of 0 or 1 clones.
    pub fn downscale(&self, factor: usize) -> Rgba8 {
        if factor <= 1 {
            return self.clone();
        }
        // Round up: the loops below already clip partial blocks against the
        // source bounds, so flooring here would silently drop the ragged
        // right/bottom edge instead of averaging it.
        let width = self.width.div_ceil(factor).max(1);
        let height = self.height.div_ceil(factor).max(1);
        let mut out = Vec::with_capacity(width * height * 4);

        for y in 0..height {
            for x in 0..width {
                let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
                for dy in 0..factor {
                    let sy = y * factor + dy;
                    if sy >= self.height {
                        break;
                    }
                    for dx in 0..factor {
                        let sx = x * factor + dx;
                        if sx >= self.width {
                            break;
                        }
                        let i = (sy * self.width + sx) * 4;
                        r += self.pixels[i] as u32;
                        g += self.pixels[i + 1] as u32;
                        b += self.pixels[i + 2] as u32;
                        a += self.pixels[i + 3] as u32;
                        n += 1;
                    }
                }
                let n = n.max(1);
                out.push((r / n) as u8);
                out.push((g / n) as u8);
                out.push((b / n) as u8);
                out.push((a / n) as u8);
            }
        }

        Rgba8 {
            width,
            height,
            pixels: out,
        }
    }
}

impl Gray8 {
    /// Constructs a grayscale buffer directly. Used by tests to build fixtures.
    #[allow(dead_code)]
    pub fn from_raw(width: usize, height: usize, pixels: Vec<u8>) -> Option<Self> {
        let expected = width.checked_mul(height)?;
        if width == 0 || height == 0 || pixels.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    /// Reads one pixel. Used by tests to assert on conversion output.
    #[allow(dead_code)]
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> u8 {
        assert!(
            x < self.width,
            "x ({x}) out of bounds for width {}",
            self.width
        );
        self.pixels[y * self.width + x]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, rgba: [u8; 4]) -> Rgba8 {
        let pixels = rgba.iter().copied().cycle().take(width * height * 4).collect();
        Rgba8::from_raw(width, height, pixels).expect("valid buffer")
    }

    #[test]
    fn rejects_dimensions_whose_size_overflows() {
        // These products wrap to exactly 0, which would otherwise match an
        // empty buffer and leave `pixel` indexing past the end of it. Written
        // relative to usize::MAX so they hold on 32- and 64-bit alike.
        assert!(Rgba8::from_raw(usize::MAX / 4 + 1, 1, Vec::new()).is_none());
        assert!(Gray8::from_raw(usize::MAX / 2 + 1, 2, Vec::new()).is_none());
    }

    #[test]
    fn rejects_buffers_that_do_not_match_dimensions() {
        assert!(Rgba8::from_raw(2, 2, vec![0; 15]).is_none());
        assert!(Rgba8::from_raw(2, 2, vec![0; 17]).is_none());
        assert!(Rgba8::from_raw(0, 4, vec![]).is_none());
        assert!(Rgba8::from_raw(2, 2, vec![0; 16]).is_some());
    }

    #[test]
    fn out_of_bounds_pixel_reads_are_opaque_black() {
        let img = solid(2, 2, [10, 20, 30, 40]);
        assert_eq!(img.pixel(0, 0), [10, 20, 30, 40]);
        assert_eq!(img.pixel(2, 0), [0, 0, 0, 255]);
        assert_eq!(img.pixel(0, 2), [0, 0, 0, 255]);
    }

    #[test]
    fn downscaling_averages_each_block() {
        // 2x2 of distinct greys; a factor of 2 collapses it to their mean.
        let pixels = vec![
            0, 0, 0, 255, 100, 100, 100, 255, // row 0
            200, 200, 200, 255, 255, 255, 255, 255, // row 1
        ];
        let image = Rgba8::from_raw(2, 2, pixels).expect("valid buffer");
        let small = image.downscale(2);

        assert_eq!((small.width(), small.height()), (1, 1));
        // (0 + 100 + 200 + 255) / 4 = 138
        assert_eq!(small.pixel(0, 0), [138, 138, 138, 255]);
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
        let small = Rgba8::from_raw(5, 1, pixels)
            .expect("valid buffer")
            .downscale(2);

        assert_eq!((small.width(), small.height()), (3, 1));
        assert_eq!(small.pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(small.pixel(2, 0), [255, 0, 0, 255], "edge column dropped");
    }

    #[test]
    fn downscaling_by_one_or_zero_is_a_no_op() {
        let image = solid(4, 4, [1, 2, 3, 255]);
        assert_eq!(image.downscale(1), image);
        assert_eq!(image.downscale(0), image);
    }

    #[test]
    fn downscaling_a_ragged_size_never_produces_an_empty_image() {
        // 5x3 at factor 2 does not divide evenly; partial blocks still count,
        // so the output rounds up rather than cropping the odd edge.
        let image = solid(5, 3, [10, 20, 30, 255]);
        let small = image.downscale(2);
        assert_eq!((small.width(), small.height()), (3, 2));
        assert_eq!(small.pixel(0, 0), [10, 20, 30, 255]);

        // A factor larger than the image still yields at least one pixel.
        let tiny = image.downscale(99);
        assert_eq!((tiny.width(), tiny.height()), (1, 1));
    }

    #[test]
    #[should_panic(expected = "out of bounds for width")]
    fn gray_reads_past_a_row_end_panic_rather_than_wrapping() {
        // Without the bounds check this quietly returns row 1's first pixel.
        let gray = solid(2, 2, [0, 0, 0, 255]).to_gray();
        let _ = gray.at(2, 0);
    }

    #[test]
    fn grayscale_uses_luma_weights() {
        assert_eq!(solid(1, 1, [255, 255, 255, 255]).to_gray().at(0, 0), 255);
        assert_eq!(solid(1, 1, [0, 0, 0, 255]).to_gray().at(0, 0), 0);
        // 0.299 * 255 = 76.2
        assert_eq!(solid(1, 1, [255, 0, 0, 255]).to_gray().at(0, 0), 76);
        // 0.587 * 255 = 149.7
        assert_eq!(solid(1, 1, [0, 255, 0, 255]).to_gray().at(0, 0), 150);
        // 0.114 * 255 = 29.1
        assert_eq!(solid(1, 1, [0, 0, 255, 255]).to_gray().at(0, 0), 29);
    }
}
