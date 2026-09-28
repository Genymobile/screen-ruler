//! Clipboard access.
//!
//! On Windows and macOS the system takes ownership of the copied data, so a
//! plain `arboard` call survives the process exiting. X11 and Wayland do not
//! work that way: the *source application* owns the selection, and unless a
//! clipboard manager steps in (GNOME and KDE have one; wlroots compositors
//! and bare X11 window managers usually do not) the clipboard empties the
//! moment it dies. Because this tool's whole point is "measure, copy, quit",
//! it prefers an external helper on Linux (`wl-copy`, `xclip`, `xsel`), since
//! those fork a daemon that keeps the selection alive.

#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

use crate::image::RgbaImage;

/// Copies text to the clipboard.
pub fn copy_text(text: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if let Some(result) = linux::copy(linux::Payload::Text, text.as_bytes()) {
        return result;
    }

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())
}

/// Copies an image to the clipboard.
pub fn copy_image(image: &RgbaImage) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        // An encoding failure means no helper could succeed; report it rather
        // than falling through to arboard, which would hit the same problem.
        let png = crate::png::encode(image)?;
        if let Some(result) = linux::copy(linux::Payload::Png, &png) {
            return result;
        }
    }

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Borrowed(image.as_raw()),
        })
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    /// How long a helper gets to fork its daemon and exit. They normally take
    /// a few milliseconds; one still running after this is assumed to be
    /// serving the selection in the foreground.
    const FORK_TIMEOUT: Duration = Duration::from_secs(2);

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(super) enum Payload {
        Text,
        Png,
    }

    /// One clipboard helper: the program to run and the arguments it needs.
    pub(super) type Helper = (&'static str, &'static [&'static str]);

    /// The helpers that talk to this session's own clipboard, most preferred
    /// first.
    ///
    /// Only the session's native helpers: under Wayland `xclip` would reach
    /// XWayland's clipboard rather than the compositor's, and arboard (the
    /// fallback) already speaks the compositor's protocol directly.
    pub(super) fn helpers(payload: Payload, wayland: bool) -> &'static [Helper] {
        match (payload, wayland) {
            (Payload::Text, true) => &[("wl-copy", &[])],
            (Payload::Png, true) => &[("wl-copy", &["--type", "image/png"])],
            (Payload::Text, false) => &[
                ("xclip", &["-selection", "clipboard"]),
                ("xsel", &["--clipboard", "--input"]),
            ],
            (Payload::Png, false) => &[("xclip", &["-selection", "clipboard", "-t", "image/png"])],
        }
    }

    /// Feeds `bytes` to the first installed helper for this session.
    ///
    /// `None` when none is installed, so the caller can fall back to arboard.
    pub(super) fn copy(payload: Payload, bytes: &[u8]) -> Option<Result<(), String>> {
        helpers(payload, crate::capture::is_wayland_session())
            .iter()
            .find_map(|(program, args)| pipe_to(program, args, bytes, FORK_TIMEOUT))
    }

    /// Feeds `bytes` to a helper on stdin and waits for it to fork.
    ///
    /// Returns `None` when the program is not installed, and `Some(Err(..))`
    /// when it exists but failed, so a missing helper falls through to the
    /// next one while a real failure (no display, say) is reported instead of
    /// silently losing the copy.
    pub(super) fn pipe_to(
        program: &str,
        args: &[&str],
        bytes: &[u8],
        timeout: Duration,
    ) -> Option<Result<(), String>> {
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            // Not piped: the forked daemon inherits it, so reading it would
            // block until the clipboard is next replaced.
            .stderr(Stdio::null())
            .spawn();

        let mut child = match child {
            Ok(child) => child,
            // Not installed: let the caller try the next helper.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => return Some(Err(format!("{program}: {e}"))),
        };

        // Dropping stdin at the end of this statement closes it, which is
        // what tells the helper the payload is complete.
        let written = child
            .stdin
            .take()
            .ok_or_else(|| format!("{program}: no stdin"))
            .and_then(|mut stdin| {
                stdin
                    .write_all(bytes)
                    .map_err(|e| format!("{program}: {e}"))
            });
        if let Err(e) = written {
            let _ = child.kill();
            let _ = child.wait();
            return Some(Err(e));
        }

        // The helpers fork a daemon to serve the selection and exit at once,
        // so their exit status says whether the copy took.
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Some(Ok(())),
                Ok(Some(status)) => return Some(Err(format!("{program} failed ({status})"))),
                Ok(None) if Instant::now() >= deadline => return Some(Ok(())),
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Some(Err(format!("{program}: {e}"))),
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux::*;
    use std::time::{Duration, Instant};

    const SHORT: Duration = Duration::from_millis(200);

    #[test]
    fn wayland_sessions_only_use_wl_copy() {
        for payload in [Payload::Text, Payload::Png] {
            let programs: Vec<_> = helpers(payload, true).iter().map(|h| h.0).collect();
            assert_eq!(programs, ["wl-copy"], "{payload:?}");
        }
    }

    #[test]
    fn x11_sessions_prefer_xclip() {
        assert_eq!(helpers(Payload::Text, false)[0].0, "xclip");
        assert!(helpers(Payload::Text, false).iter().any(|h| h.0 == "xsel"));
        // xsel cannot declare a MIME type, so it is no use for images.
        assert!(helpers(Payload::Png, false).iter().all(|h| h.0 != "xsel"));
    }

    #[test]
    fn image_helpers_declare_the_png_type() {
        for wayland in [true, false] {
            for (_, args) in helpers(Payload::Png, wayland) {
                assert!(args.contains(&"image/png"), "{args:?}");
            }
        }
    }

    #[test]
    fn a_missing_helper_falls_through() {
        assert!(pipe_to("screen-ruler-no-such-helper", &[], b"x", SHORT).is_none());
    }

    #[test]
    fn a_helper_that_exits_cleanly_succeeds() {
        let result = pipe_to("sh", &["-c", "cat > /dev/null"], b"payload", SHORT);
        assert_eq!(result, Some(Ok(())));
    }

    #[test]
    fn a_failing_helper_is_reported() {
        let result = pipe_to("sh", &["-c", "cat > /dev/null; exit 3"], b"payload", SHORT);
        assert!(
            matches!(result, Some(Err(ref e)) if e.contains("sh failed")),
            "{result:?}"
        );
    }

    #[test]
    fn a_helper_that_stays_in_the_foreground_does_not_block() {
        let start = Instant::now();
        let result = pipe_to("sh", &["-c", "cat > /dev/null; sleep 5"], b"payload", SHORT);
        assert_eq!(result, Some(Ok(())));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_whole_payload_reaches_the_helper() {
        // Larger than a pipe buffer, and checked by the helper itself.
        let payload = vec![b'a'; 1 << 20];
        let script = format!("test \"$(wc -c)\" -eq {}", payload.len());
        assert_eq!(
            pipe_to("sh", &["-c", &script], &payload, SHORT),
            Some(Ok(()))
        );
    }
}
