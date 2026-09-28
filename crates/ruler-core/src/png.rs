//! PNG encoding for the clipboard.
//!
//! The external clipboard helpers want `image/png` bytes. Encoding goes
//! through the `image` crate's PNG codec, which slint already enables, so it
//! costs nothing and gives real deflate compression.

use ::image::codecs::png::PngEncoder;
use ::image::ImageEncoder;

use crate::image::RgbaImage;

/// Encodes an image as PNG bytes.
pub fn encode(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ::image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("cannot encode PNG: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Rgba;

    fn gradient(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| Rgba([x as u8, y as u8, 128, 255]))
    }

    /// Decodes `bytes` back into RGBA, so the tests assert against what a
    /// clipboard consumer would actually see rather than our own byte layout.
    fn decode(bytes: &[u8]) -> RgbaImage {
        ::image::load_from_memory_with_format(bytes, ::image::ImageFormat::Png)
            .expect("valid PNG")
            .into_rgba8()
    }

    #[test]
    fn output_starts_with_the_png_signature() {
        let bytes = encode(&gradient(4, 4)).expect("encodes");
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
    }

    #[test]
    fn pixels_survive_a_round_trip_unchanged() {
        let image = gradient(7, 3);
        let decoded = decode(&encode(&image).expect("encodes"));

        assert_eq!(decoded.dimensions(), (7, 3));
        assert_eq!(
            decoded.as_raw(),
            image.as_raw(),
            "RGBA data changed in transit"
        );
    }

    #[test]
    fn translucent_pixels_keep_their_alpha() {
        let image = RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 64]));
        let decoded = decode(&encode(&image).expect("encodes"));
        assert_eq!(decoded.get_pixel(1, 1).0, [10, 20, 30, 64]);
    }

    #[test]
    fn a_payload_larger_than_one_deflate_block_round_trips() {
        // Three rows of 40000 px comfortably exceeds the 65535-byte cap on a
        // single deflate block, so the encoder has to split transparently.
        let image = gradient(40_000, 3);
        let decoded = decode(&encode(&image).expect("encodes"));

        assert_eq!(decoded.dimensions(), (40_000, 3));
        assert_eq!(decoded.as_raw(), image.as_raw());
    }

    #[test]
    fn compression_beats_the_raw_pixel_size() {
        // A smooth gradient is highly compressible; this guards against
        // silently regressing to stored (uncompressed) blocks.
        let image = gradient(256, 256);
        let encoded = encode(&image).expect("encodes");
        assert!(
            encoded.len() < image.as_raw().len() / 2,
            "expected real compression, got {} bytes for {} raw",
            encoded.len(),
            image.as_raw().len()
        );
    }
}
