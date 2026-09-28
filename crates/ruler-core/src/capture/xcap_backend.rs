//! macOS and Windows capture through xcap (Quartz and DXGI respectively).
//!
//! Linux does not use this: xcap's Linux backend links libwayland and
//! PipeWire at build time and crops XWayland's layout on Wayland, while the
//! native backends beside this file need neither.

use crate::geometry::MonitorGeometry;

use super::{CaptureError, CapturedMonitor};

/// Captures every monitor. A monitor that fails is skipped rather than
/// aborting the run, so one flaky output does not take down the whole tool.
pub(super) fn capture_all() -> Result<Vec<CapturedMonitor>, CaptureError> {
    let monitors = xcap::Monitor::all().map_err(|e| CaptureError::Backend(e.to_string()))?;
    if monitors.is_empty() {
        return Err(CaptureError::NoMonitors);
    }

    let mut captured = Vec::new();
    let mut first_error = None;
    for monitor in monitors {
        match capture_one(monitor) {
            Ok(monitor) => captured.push(monitor),
            Err(reason) => {
                first_error.get_or_insert(reason);
            }
        }
    }

    if captured.is_empty() {
        return Err(CaptureError::AllMonitorsFailed(
            first_error.unwrap_or_else(|| "unknown reason".to_string()),
        ));
    }
    Ok(captured)
}

/// Captures a single monitor and packages it with its reported geometry.
fn capture_one(monitor: xcap::Monitor) -> Result<CapturedMonitor, String> {
    let name = monitor
        .name()
        .or_else(|_| monitor.friendly_name())
        .unwrap_or_else(|_| "unknown".to_string());

    let describe = |what: &str, e: xcap::XCapError| format!("{name}: {what}: {e}");

    let position = (
        monitor.x().map_err(|e| describe("x", e))?,
        monitor.y().map_err(|e| describe("y", e))?,
    );
    let scale = monitor.scale_factor().unwrap_or(1.0);
    let is_primary = monitor.is_primary().unwrap_or(false);

    let image = monitor
        .capture_image()
        .map_err(|e| describe("capture", e))?;

    Ok(CapturedMonitor {
        geometry: MonitorGeometry {
            name,
            position,
            // The captured bitmap is the authority on pixel extent: the
            // reported width/height can disagree with it under scaling, and
            // the edge map has to line up with the pixels we actually have.
            size: (image.width(), image.height()),
            scale,
            is_primary,
        },
        image,
    })
}
