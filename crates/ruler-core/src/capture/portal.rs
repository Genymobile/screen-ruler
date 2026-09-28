//! Wayland capture through the desktop portal (`org.freedesktop.portal.Screenshot`).
//!
//! The fallback for compositors without `wlr-screencopy`, i.e. GNOME and KDE.
//! The portal hands back one PNG of the whole desktop, so it is cut back into
//! per-monitor images using the compositor's own output layout.
//!
//! One request covers every monitor. (xcap instead issues a request per
//! monitor and crops each result by XWayland's view of the layout, which costs
//! a full-desktop screenshot per display and breaks without XWayland.)

use std::collections::HashMap;
use std::path::PathBuf;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::geometry::MonitorGeometry;
use crate::image::RgbaImage;

use super::wayland::{self, OutputLayout};
use super::CapturedMonitor;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";

/// How far the image's horizontal and vertical density may disagree before
/// the layout is judged not to describe the image. Covers rounding of
/// fractional logical sizes; anything larger means cropping would cut the
/// wrong pixels.
const DENSITY_TOLERANCE: f32 = 0.02;

/// Captures every output with a single portal screenshot.
pub(super) fn capture_all() -> Result<Vec<CapturedMonitor>, String> {
    // Read the layout first: it is cheap and fails fast when there is no
    // Wayland display, before the user is shown any portal prompt.
    let layout = wayland::output_layout()?;
    let path = request_screenshot()?;
    let bytes = std::fs::read(&path);
    // The portal writes a fresh file for this request; the app is its only
    // consumer, so do not leave it behind.
    let _ = std::fs::remove_file(&path);
    let bytes = bytes.map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let desktop = ::image::load_from_memory_with_format(&bytes, ::image::ImageFormat::Png)
        .map_err(|e| format!("cannot decode the portal screenshot: {e}"))?
        .into_rgba8();
    split(&desktop, &layout)
}

/// Asks the portal for a non-interactive screenshot and returns its file.
fn request_screenshot() -> Result<PathBuf, String> {
    let describe = |e: zbus::Error| format!("desktop portal: {e}");
    let connection = Connection::session().map_err(describe)?;

    // The portal answers on a Request object whose path is derived from our
    // bus name and a token we choose. Subscribing before calling avoids
    // missing a response that arrives immediately.
    let token = format!("screen_ruler_{}", std::process::id());
    let sender = connection
        .unique_name()
        .ok_or("desktop portal: no unique bus name")?
        .trim_start_matches(':')
        .replace('.', "_");
    let request_path = format!("{PORTAL_PATH}/request/{sender}/{token}");
    let request = Proxy::new(
        &connection,
        PORTAL,
        request_path.as_str(),
        "org.freedesktop.portal.Request",
    )
    .map_err(describe)?;
    let mut responses = request.receive_signal("Response").map_err(describe)?;

    let screenshot = Proxy::new(
        &connection,
        PORTAL,
        PORTAL_PATH,
        "org.freedesktop.portal.Screenshot",
    )
    .map_err(describe)?;
    let options = HashMap::from([
        ("handle_token", Value::from(token.as_str())),
        // No area picker: the app wants the whole desktop, every time.
        ("interactive", Value::from(false)),
    ]);
    let _: OwnedObjectPath = screenshot
        .call("Screenshot", &("", options))
        .map_err(describe)?;

    let response = responses
        .next()
        .ok_or("desktop portal: closed without responding")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) =
        response.body().deserialize().map_err(describe)?;
    match code {
        0 => {}
        1 => return Err("desktop portal: the screenshot was declined".to_string()),
        _ => return Err("desktop portal: the screenshot failed".to_string()),
    }

    let uri = results
        .get("uri")
        .and_then(|v| v.downcast_ref::<&str>().ok())
        .ok_or("desktop portal: response carried no file")?;
    file_uri_to_path(uri).ok_or_else(|| format!("desktop portal: unusable file URI {uri}"))
}

/// Decodes a `file://` URI into a path. `None` for any other scheme or a
/// malformed escape.
fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Skip an authority (`file://host/path`); only the path matters locally.
    let path = &rest[rest.find('/')?..];

    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }

    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(decoded)))
}

/// Cuts a whole-desktop screenshot into one image per output.
///
/// The screenshot spans the outputs' logical bounding box at a single
/// density — compositors render the stitched image at one scale even when
/// monitors differ — so each output's crop is its logical rectangle times that
/// density, which also becomes its scale.
fn split(desktop: &RgbaImage, layout: &[OutputLayout]) -> Result<Vec<CapturedMonitor>, String> {
    let (bx, by, bw, bh) = logical_bounds(layout).ok_or("compositor reported no usable outputs")?;

    let density = desktop.width() as f32 / bw as f32;
    let vertical = desktop.height() as f32 / bh as f32;
    if density <= 0.0 || (density - vertical).abs() > density * DENSITY_TOLERANCE {
        return Err(format!(
            "the {}x{} screenshot does not match the {bw}x{bh} logical desktop",
            desktop.width(),
            desktop.height()
        ));
    }

    let to_device = |logical: i32| (logical as f32 * density).round() as i64;
    let mut monitors = Vec::new();
    for output in layout {
        // Round both edges rather than origin and extent, so neighbouring
        // outputs share a boundary instead of gaining or losing a pixel.
        let clamp = |v: i64, max: u32| v.clamp(0, max as i64) as u32;
        let x0 = clamp(to_device(output.position.0 - bx), desktop.width());
        let y0 = clamp(to_device(output.position.1 - by), desktop.height());
        let x1 = clamp(
            to_device(output.position.0 + output.size.0 - bx),
            desktop.width(),
        );
        let y1 = clamp(
            to_device(output.position.1 + output.size.1 - by),
            desktop.height(),
        );
        if x1 <= x0 || y1 <= y0 {
            continue;
        }

        let image = ::image::imageops::crop_imm(desktop, x0, y0, x1 - x0, y1 - y0).to_image();
        monitors.push(CapturedMonitor {
            geometry: MonitorGeometry {
                name: output.name.clone(),
                position: (
                    to_device(output.position.0) as i32,
                    to_device(output.position.1) as i32,
                ),
                size: (image.width(), image.height()),
                scale: density,
                is_primary: false,
            },
            image,
        });
    }

    if monitors.is_empty() {
        return Err("no output overlaps the portal screenshot".to_string());
    }
    Ok(monitors)
}

/// Bounding box `(x, y, width, height)` of every output with a positive
/// size, in logical px.
fn logical_bounds(layout: &[OutputLayout]) -> Option<(i32, i32, i32, i32)> {
    let mut usable = layout.iter().filter(|o| o.size.0 > 0 && o.size.1 > 0);
    let first = usable.next()?;
    let init = (
        first.position.0,
        first.position.1,
        first.position.0 + first.size.0,
        first.position.1 + first.size.1,
    );
    let (x0, y0, x1, y1) = usable.fold(init, |(x0, y0, x1, y1), o| {
        (
            x0.min(o.position.0),
            y0.min(o.position.1),
            x1.max(o.position.0 + o.size.0),
            y1.max(o.position.1 + o.size.1),
        )
    });
    Some((x0, y0, x1 - x0, y1 - y0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Rgba;

    fn output(name: &str, position: (i32, i32), size: (i32, i32)) -> OutputLayout {
        OutputLayout {
            name: name.to_string(),
            position,
            size,
        }
    }

    /// A desktop image whose red channel encodes the column, so crops can be
    /// checked by where they start.
    fn desktop(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, _| Rgba([x as u8, 0, 0, 255]))
    }

    #[test]
    fn side_by_side_outputs_are_split_at_their_boundary() {
        let layout = [
            output("A", (0, 0), (100, 50)),
            output("B", (100, 0), (100, 50)),
        ];
        let monitors = split(&desktop(200, 50), &layout).expect("split");

        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0].geometry.size, (100, 50));
        assert_eq!(monitors[1].geometry.size, (100, 50));
        assert_eq!(monitors[1].geometry.position, (100, 0));
        assert_eq!(monitors[1].image.get_pixel(0, 0).0[0], 100);
        assert_eq!(monitors[0].geometry.scale, 1.0);
    }

    #[test]
    fn a_hidpi_screenshot_scales_every_crop() {
        // Logical 100x50 + 100x50, rendered at 2x.
        let layout = [
            output("A", (0, 0), (100, 50)),
            output("B", (100, 0), (100, 50)),
        ];
        let monitors = split(&desktop(400, 100), &layout).expect("split");

        assert_eq!(monitors[1].geometry.size, (200, 100));
        assert_eq!(monitors[1].geometry.position, (200, 0));
        assert_eq!(monitors[1].geometry.scale, 2.0);
        assert_eq!(monitors[1].image.get_pixel(0, 0).0[0], 200);
    }

    #[test]
    fn a_layout_not_starting_at_the_origin_is_offset() {
        // An output left of the primary puts the bounding box at negative x.
        let layout = [
            output("L", (-100, 0), (100, 50)),
            output("P", (0, 0), (100, 50)),
        ];
        let monitors = split(&desktop(200, 50), &layout).expect("split");

        assert_eq!(monitors[0].image.get_pixel(0, 0).0[0], 0);
        assert_eq!(monitors[1].image.get_pixel(0, 0).0[0], 100);
        assert_eq!(monitors[0].geometry.position, (-100, 0));
    }

    #[test]
    fn fractional_densities_share_boundaries() {
        // 1.5x: 3 logical px per output would be 4.5 device px each.
        let layout = [output("A", (0, 0), (3, 2)), output("B", (3, 0), (3, 2))];
        let monitors = split(&desktop(9, 3), &layout).expect("split");

        let widths: u32 = monitors.iter().map(|m| m.geometry.size.0).sum();
        assert_eq!(widths, 9, "no column dropped or duplicated");
    }

    #[test]
    fn an_image_that_does_not_match_the_layout_is_rejected() {
        // Width says 1x, height says 2x.
        let layout = [output("A", (0, 0), (100, 50))];
        assert!(split(&desktop(100, 100), &layout).is_err());
    }

    #[test]
    fn empty_outputs_are_ignored() {
        let layout = [
            output("A", (0, 0), (100, 50)),
            output("ghost", (0, 0), (0, 0)),
        ];
        let monitors = split(&desktop(100, 50), &layout).expect("split");
        assert_eq!(monitors.len(), 1);
        assert!(split(&desktop(100, 50), &[output("ghost", (0, 0), (0, 0))]).is_err());
    }

    #[test]
    fn file_uris_are_percent_decoded() {
        assert_eq!(
            file_uri_to_path("file:///home/u/Pictures/Screenshot%20from%202026.png"),
            Some(PathBuf::from("/home/u/Pictures/Screenshot from 2026.png"))
        );
        assert_eq!(
            file_uri_to_path("file://localhost/tmp/a.png"),
            Some(PathBuf::from("/tmp/a.png"))
        );
        assert_eq!(file_uri_to_path("https://example.com/a.png"), None);
        assert_eq!(file_uri_to_path("file:///tmp/bad%zz"), None);
        assert_eq!(file_uri_to_path("file:///tmp/truncated%2"), None);
    }
}
