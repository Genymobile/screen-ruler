//! X11 capture: RandR for the monitor layout, `GetImage` on the root window
//! for the pixels.
//!
//! Uses x11rb's pure-Rust connection, so it needs no libxcb at build time.

use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, ImageOrder, Screen, Window};
use x11rb::rust_connection::RustConnection;

use crate::geometry::MonitorGeometry;
use crate::image::RgbaImage;

use super::CapturedMonitor;

/// The DPI X11 treats as scale 1.
const BASE_DPI: f32 = 96.0;

/// How the server lays out pixels in a `ZPixmap` image.
#[derive(Clone, Copy, Debug)]
struct PixelFormat {
    bits_per_pixel: u8,
    /// Each row is padded to a multiple of this many bits.
    scanline_pad: u8,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    lsb_first: bool,
}

/// A monitor's rectangle on the root window, in device pixels.
struct Area {
    name: String,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    is_primary: bool,
}

/// Captures every monitor RandR reports.
pub(super) fn capture_all() -> Result<Vec<CapturedMonitor>, String> {
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|e| format!("cannot connect to the X server: {e}"))?;
    let screen = &conn.setup().roots[screen_num];
    let format = pixel_format(&conn, screen)?;
    let scale = xft_scale(&conn);

    let mut captured = Vec::new();
    let mut first_error = None;
    for area in areas(&conn, screen) {
        match capture_area(&conn, screen.root, &area, format, scale) {
            Ok(monitor) => captured.push(monitor),
            Err(reason) => {
                first_error.get_or_insert(reason);
            }
        }
    }

    if captured.is_empty() {
        return Err(first_error.unwrap_or_else(|| "no monitor could be captured".to_string()));
    }
    Ok(captured)
}

/// The monitors to capture: RandR's list, or the whole root window when
/// RandR is unavailable (e.g. a bare Xvfb), so capture still works there.
fn areas(conn: &RustConnection, screen: &Screen) -> Vec<Area> {
    let monitors = conn
        .randr_get_monitors(screen.root, true)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.monitors)
        .unwrap_or_default();

    if monitors.is_empty() {
        return vec![Area {
            name: "screen".to_string(),
            x: 0,
            y: 0,
            width: screen.width_in_pixels,
            height: screen.height_in_pixels,
            is_primary: true,
        }];
    }

    monitors
        .into_iter()
        .map(|m| Area {
            name: conn
                .get_atom_name(m.name)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| String::from_utf8_lossy(&reply.name).into_owned())
                .unwrap_or_else(|| "unknown".to_string()),
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
            is_primary: m.primary,
        })
        .collect()
}

fn capture_area(
    conn: &RustConnection,
    root: Window,
    area: &Area,
    format: PixelFormat,
    scale: f32,
) -> Result<CapturedMonitor, String> {
    let name = &area.name;
    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            root,
            area.x,
            area.y,
            area.width,
            area.height,
            !0,
        )
        .map_err(|e| format!("{name}: {e}"))?
        .reply()
        .map_err(|e| format!("{name}: capture: {e}"))?;

    let image =
        to_rgba(&reply.data, area.width as u32, area.height as u32, format).ok_or_else(|| {
            format!(
                "{name}: unsupported {}-bit pixel format",
                format.bits_per_pixel
            )
        })?;

    Ok(CapturedMonitor {
        geometry: MonitorGeometry {
            name: name.clone(),
            position: (area.x as i32, area.y as i32),
            size: (image.width(), image.height()),
            scale,
            is_primary: area.is_primary,
        },
        image,
    })
}

/// Looks up how the root window's pixels are encoded.
fn pixel_format(conn: &RustConnection, screen: &Screen) -> Result<PixelFormat, String> {
    let setup = conn.setup();
    let visual = screen
        .allowed_depths
        .iter()
        .flat_map(|d| d.visuals.iter())
        .find(|v| v.visual_id == screen.root_visual)
        .ok_or("the root window's visual is not advertised")?;
    let pixmap = setup
        .pixmap_formats
        .iter()
        .find(|f| f.depth == screen.root_depth)
        .ok_or("the root window's depth has no pixmap format")?;

    Ok(PixelFormat {
        bits_per_pixel: pixmap.bits_per_pixel,
        scanline_pad: pixmap.scanline_pad,
        red_mask: visual.red_mask,
        green_mask: visual.green_mask,
        blue_mask: visual.blue_mask,
        lsb_first: setup.image_byte_order == ImageOrder::LSB_FIRST,
    })
}

/// Scale from the `Xft.dpi` resource, which is how X11 desktops express
/// HiDPI. 1.0 when it is unset or unreadable.
fn xft_scale(conn: &RustConnection) -> f32 {
    x11rb::resource_manager::new_from_default(conn)
        .ok()
        .and_then(|db| db.get_value::<u32>("Xft.dpi", "").ok().flatten())
        .map(|dpi| dpi as f32 / BASE_DPI)
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(1.0)
}

/// Decodes a `ZPixmap` image of 16, 24 or 32 bits per pixel into RGBA.
///
/// Channels are extracted by the visual's masks rather than assumed to be
/// BGRX, and widened to 8 bits when narrower (e.g. 16-bit 5-6-5).
fn to_rgba(data: &[u8], width: u32, height: u32, format: PixelFormat) -> Option<RgbaImage> {
    let bytes_per_pixel = match format.bits_per_pixel {
        16 | 24 | 32 => format.bits_per_pixel as usize / 8,
        _ => return None,
    };
    let pad = (format.scanline_pad as usize).max(8);
    let row_bits = width as usize * format.bits_per_pixel as usize;
    let stride = row_bits.div_ceil(pad) * pad / 8;

    let channels = [format.red_mask, format.green_mask, format.blue_mask].map(Channel::new);
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for row in 0..height as usize {
        let start = row * stride;
        let line = data.get(start..start + width as usize * bytes_per_pixel)?;
        for pixel in line.chunks_exact(bytes_per_pixel) {
            let value = pixel.iter().enumerate().fold(0u32, |acc, (i, &byte)| {
                let shift = if format.lsb_first {
                    i
                } else {
                    bytes_per_pixel - 1 - i
                };
                acc | (byte as u32) << (8 * shift)
            });
            for channel in &channels {
                out.push(channel.extract(value));
            }
            // The root window has no meaningful alpha.
            out.push(255);
        }
    }

    RgbaImage::from_raw(width, height, out)
}

/// One colour channel's position within a pixel value.
struct Channel {
    mask: u32,
    shift: u32,
    max: u32,
}

impl Channel {
    fn new(mask: u32) -> Self {
        let shift = mask.trailing_zeros().min(31);
        Channel {
            mask,
            shift,
            max: mask.checked_shr(shift).unwrap_or(0),
        }
    }

    fn extract(&self, value: u32) -> u8 {
        if self.max == 0 {
            return 0;
        }
        let raw = (value & self.mask) >> self.shift;
        ((raw * 255 + self.max / 2) / self.max) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BGRX: PixelFormat = PixelFormat {
        bits_per_pixel: 32,
        scanline_pad: 32,
        red_mask: 0xff0000,
        green_mask: 0x00ff00,
        blue_mask: 0x0000ff,
        lsb_first: true,
    };

    #[test]
    fn the_common_24_bit_depth_decodes_as_bgrx() {
        // Memory order B, G, R, X on a little-endian server.
        let image = to_rgba(&[10, 20, 30, 0], 1, 1, BGRX).expect("decoded");
        assert_eq!(image.get_pixel(0, 0).0, [30, 20, 10, 255]);
    }

    #[test]
    fn big_endian_servers_reverse_the_byte_order() {
        let format = PixelFormat {
            lsb_first: false,
            ..BGRX
        };
        let image = to_rgba(&[0, 30, 20, 10], 1, 1, format).expect("decoded");
        assert_eq!(image.get_pixel(0, 0).0, [30, 20, 10, 255]);
    }

    #[test]
    fn sixteen_bit_channels_are_widened_to_full_range() {
        let rgb565 = PixelFormat {
            bits_per_pixel: 16,
            scanline_pad: 32,
            red_mask: 0xf800,
            green_mask: 0x07e0,
            blue_mask: 0x001f,
            lsb_first: true,
        };
        // Pure white and pure red.
        let image = to_rgba(&[0xff, 0xff, 0x00, 0xf8], 2, 1, rgb565).expect("decoded");
        assert_eq!(image.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn rows_are_padded_to_the_scanline_pad() {
        // 24 bpp, one pixel per row, rows padded to 32 bits.
        let format = PixelFormat {
            bits_per_pixel: 24,
            ..BGRX
        };
        let data = [10, 20, 30, 99, 40, 50, 60, 99];
        let image = to_rgba(&data, 1, 2, format).expect("decoded");
        assert_eq!(image.get_pixel(0, 1).0, [60, 50, 40, 255]);
    }

    #[test]
    fn short_buffers_and_odd_depths_are_rejected() {
        assert!(to_rgba(&[0; 4], 2, 2, BGRX).is_none());
        let format = PixelFormat {
            bits_per_pixel: 8,
            ..BGRX
        };
        assert!(to_rgba(&[0; 4], 1, 1, format).is_none());
    }

    #[test]
    fn an_empty_mask_reads_as_zero() {
        let format = PixelFormat {
            blue_mask: 0,
            ..BGRX
        };
        let image = to_rgba(&[10, 20, 30, 0], 1, 1, format).expect("decoded");
        assert_eq!(image.get_pixel(0, 0).0[2], 0);
    }
}
