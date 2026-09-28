//! Per-monitor screen capture.
//!
//! Each monitor is captured separately rather than as one stitched
//! virtual-desktop bitmap. That is what allows every display to keep its own
//! resolution and scale factor instead of being resampled into a single
//! lowest-common-denominator image.
//!
//! Backends, in the order they are tried:
//!
//! * **Linux, Wayland** — `wlr-screencopy` (wlroots compositors), then the
//!   desktop portal (GNOME, KDE). Never X11: under Wayland that would only see
//!   XWayland clients.
//! * **Linux, X11** — RandR + `GetImage`.
//! * **macOS, Windows** — xcap.
//!
//! The Linux backends are built on crates slint already links, so they add
//! nothing to the build; xcap is only a dependency off Linux.

use crate::geometry::MonitorGeometry;
use image::RgbaImage;

#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(not(target_os = "linux"))]
mod xcap_backend;

/// One monitor's pixels plus the geometry the capture backend reported for it.
pub struct CapturedMonitor {
    pub geometry: MonitorGeometry,
    pub image: RgbaImage,
}

/// Why capture failed, in terms a user can act on.
#[derive(Debug)]
pub enum CaptureError {
    /// The platform reported no usable displays.
    NoMonitors,
    /// Every monitor failed to capture; carries the underlying reasons.
    AllMonitorsFailed(String),
    /// The capture backend itself could not be reached.
    Backend(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::NoMonitors => write!(f, "no displays were reported by the system"),
            CaptureError::AllMonitorsFailed(reason) => {
                write!(f, "no display could be captured: {reason}")
            }
            CaptureError::Backend(reason) => write!(f, "screen capture is unavailable: {reason}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// True when a Wayland session is in use.
///
/// Decides which capture backend to try and which clipboard helper to prefer,
/// so it is defined once here rather than re-derived at each call site.
pub fn is_wayland_session() -> bool {
    names_a_display(std::env::var_os("WAYLAND_DISPLAY").as_deref())
}

/// Whether a `*_DISPLAY` value names a display. Set-but-empty counts as
/// unset, as it does for Wayland clients themselves: wrappers and service
/// units often blank the variable rather than removing it.
fn names_a_display(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// A platform-specific hint shown when capture fails, so the user knows what to
/// grant or install rather than just seeing an error.
pub fn permission_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Grant Screen Recording permission in System Settings > Privacy & Security, \
         then relaunch."
    } else if cfg!(target_os = "windows") {
        "Check that the app is allowed to capture the screen and is not blocked by a \
         protected-content policy."
    } else if is_wayland_session() {
        "On Wayland, allow the screenshot request from the desktop portal when prompted. \
         Unless the compositor supports wlr-screencopy (Sway, Hyprland, river, Wayfire), \
         a portal implementation (xdg-desktop-portal plus a backend such as \
         xdg-desktop-portal-gnome or -kde) must be running."
    } else {
        "Check that the X server allows screen capture for this session."
    }
}

/// Captures every connected monitor, trying each available backend in turn.
///
/// Every backend failure is collected so that, if none succeed, the reported
/// error explains what was actually tried.
#[cfg(target_os = "linux")]
pub fn capture_all() -> Result<Vec<CapturedMonitor>, CaptureError> {
    type Backend = fn() -> Result<Vec<CapturedMonitor>, String>;
    let backends: &[(&str, Backend)] = if is_wayland_session() {
        &[
            ("wlr-screencopy", wayland::capture_all),
            ("desktop portal", portal::capture_all),
        ]
    } else {
        &[("X11", x11::capture_all)]
    };

    let mut attempts = Vec::new();
    for (name, capture) in backends {
        match capture() {
            Ok(monitors) if !monitors.is_empty() => return Ok(finish(monitors)),
            Ok(_) => attempts.push(format!("{name}: no monitors returned")),
            Err(reason) => attempts.push(format!("{name}: {reason}")),
        }
    }
    Err(CaptureError::AllMonitorsFailed(attempts.join("; ")))
}

/// Captures every connected monitor.
#[cfg(not(target_os = "linux"))]
pub fn capture_all() -> Result<Vec<CapturedMonitor>, CaptureError> {
    xcap_backend::capture_all().map(finish)
}

/// Puts monitors in a deterministic top-to-bottom, left-to-right order.
///
/// Stable indices matter for reproducing multi-screen bugs and for keeping
/// annotations attached to the same display across a re-analysis.
fn finish(mut monitors: Vec<CapturedMonitor>) -> Vec<CapturedMonitor> {
    monitors.sort_by_key(|m| (m.geometry.position.1, m.geometry.position.0));
    monitors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, position: (i32, i32)) -> CapturedMonitor {
        CapturedMonitor {
            geometry: MonitorGeometry {
                name: name.to_string(),
                position,
                size: (1, 1),
                scale: 1.0,
                is_primary: false,
            },
            image: RgbaImage::new(1, 1),
        }
    }

    #[test]
    fn monitors_are_ordered_by_row_then_column() {
        let ordered = finish(vec![
            monitor("below", (0, 1080)),
            monitor("right", (1920, 0)),
            monitor("left", (-1920, 0)),
            monitor("origin", (0, 0)),
        ]);
        let names: Vec<_> = ordered.iter().map(|m| m.geometry.name.as_str()).collect();
        assert_eq!(names, ["left", "origin", "right", "below"]);
    }

    #[test]
    fn an_empty_display_variable_is_not_a_session() {
        use std::ffi::OsStr;
        assert!(names_a_display(Some(OsStr::new("wayland-0"))));
        assert!(!names_a_display(Some(OsStr::new(""))));
        assert!(!names_a_display(None));
    }

    #[test]
    fn errors_read_as_sentences_carrying_their_reason() {
        let error = CaptureError::AllMonitorsFailed("wlr-screencopy: unsupported".to_string());
        assert_eq!(
            error.to_string(),
            "no display could be captured: wlr-screencopy: unsupported"
        );
    }
}
