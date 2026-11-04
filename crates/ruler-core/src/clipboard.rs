//! Clipboard access.
//!
//! On Windows and macOS the system takes ownership of the copied data, so a
//! plain `arboard` call survives the process exiting. X11 and Wayland do not
//! work that way: the *source application* owns the selection, and unless a
//! clipboard manager steps in (GNOME and KDE have one; wlroots compositors
//! and bare X11 window managers usually do not) the clipboard empties the
//! moment it dies. Because this tool's whole point is "measure, copy, quit",
//! it prefers an external helper on Linux (`wl-copy`, `xclip`, `xsel`), since
//! those fork a daemon that keeps the selection alive. Without one, a copy
//! reports [`Copied::MayNotLast`], so the user can be warned.

#[cfg(target_os = "linux")]
use std::borrow::Cow;
#[cfg(target_os = "linux")]
use std::ffi::OsStr;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

use image::RgbaImage;

/// Whether a copy can be expected to outlive screen-ruler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Copied {
    /// The system, or a helper's daemon, keeps the copy.
    Lasting,
    /// Linux without the session's helper (wl-copy on Wayland; on X11 xclip,
    /// or xsel for text only), where arboard serves the copy from this
    /// process. Unless a clipboard manager takes it, it goes when screen-ruler
    /// quits, or on X11 as soon as the copy returns.
    MayNotLast,
}

/// What a copy puts on the clipboard, which decides the helpers that can
/// take it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    Text,
    /// An image, handed to the helpers as PNG.
    Png,
}

/// What to install so that copies of `payload` outlive screen-ruler, after
/// a [`Copied::MayNotLast`]: the package with this session's helper.
pub fn install_hint(payload: Payload) -> &'static str {
    install_hint_for(payload, crate::capture::is_wayland_session())
}

/// [`install_hint`] for a Wayland session or not, as the copies pick their
/// helpers.
fn install_hint_for(payload: Payload, wayland: bool) -> &'static str {
    match (payload, wayland) {
        (_, true) => "install wl-clipboard",
        (Payload::Text, false) => "install xclip or xsel",
        // xsel cannot declare a MIME type, so it cannot copy an image.
        (Payload::Png, false) => "install xclip",
    }
}

/// Copies text to the clipboard, and says whether the copy can be expected
/// to outlive screen-ruler.
pub fn copy_text(text: &str) -> Result<Copied, String> {
    #[cfg(target_os = "linux")]
    if let Some(result) = linux::copy(Payload::Text, || Ok(Cow::Borrowed(text.as_bytes()))) {
        return result.map(|()| Copied::Lasting);
    }

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())?;
    Ok(if cfg!(target_os = "linux") {
        Copied::MayNotLast
    } else {
        Copied::Lasting
    })
}

/// Copies an image to the clipboard, and says whether the copy can be
/// expected to outlive screen-ruler, as [`copy_text`] does.
pub fn copy_image(image: &RgbaImage) -> Result<Copied, String> {
    #[cfg(target_os = "linux")]
    // An encoding failure is reported rather than falling through to
    // arboard: a helper exists, so the copy belongs to it.
    if let Some(result) = linux::copy(Payload::Png, || linux::encode_png(image).map(Cow::Owned)) {
        return result.map(|()| Copied::Lasting);
    }

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Borrowed(image.as_raw()),
        })
        .map_err(|e| e.to_string())?;
    Ok(if cfg!(target_os = "linux") {
        Copied::MayNotLast
    } else {
        Copied::Lasting
    })
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;

    /// Encodes an image as the `image/png` bytes the helpers want.
    pub(super) fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| format!("cannot encode PNG: {e}"))?;
        Ok(out)
    }

    /// How long a helper gets, from the last write that got through, to read
    /// more or to fork its daemon and exit. They normally take a few
    /// milliseconds; one still running this long after the whole payload is
    /// assumed to be serving the selection in the foreground.
    const FORK_TIMEOUT: Duration = Duration::from_secs(2);

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

    /// Copies through the first helper installed for this session.
    ///
    /// `None` when none is installed, so the caller can fall back to arboard.
    /// The payload is only built once a helper is known to exist, so an image
    /// is not PNG-encoded just to be handed to arboard as raw pixels.
    pub(super) fn copy<'a>(
        payload: Payload,
        bytes: impl FnOnce() -> Result<Cow<'a, [u8]>, String>,
    ) -> Option<Result<(), String>> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let wayland = crate::capture::is_wayland_session();
        let (program, args) = first_installed(helpers(payload, wayland), &path)?;
        match bytes() {
            Ok(bytes) => pipe_to(program, args, &bytes, FORK_TIMEOUT),
            Err(e) => Some(Err(e)),
        }
    }

    /// The first helper with an executable on `path` (a `PATH`-style list).
    pub(super) fn first_installed(helpers: &[Helper], path: &OsStr) -> Option<Helper> {
        helpers.iter().copied().find(|(program, _)| {
            std::env::split_paths(path).any(|dir| {
                std::fs::metadata(dir.join(program))
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
        })
    }

    /// Feeds `bytes` to a helper on stdin and waits for it to fork. The wait
    /// ends when the helper exits, or once it has gone `timeout` without
    /// reading more of the payload. A helper still running then has failed
    /// if it has not read the whole payload. If it has, the copy succeeded:
    /// the helper is serving the selection in the foreground, so it is left
    /// running.
    ///
    /// Returns `None` when the program is not installed, and `Some(Err(..))`
    /// when it exists but failed, so a failure (no display, say) is reported
    /// instead of silently losing the copy.
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
            // Vanished since the PATH check: let the caller fall back.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => return Some(Err(format!("{program}: {e}"))),
        };
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Some(Err(format!("{program}: no stdin")));
        };

        // Non-blocking, so a helper that stops reading cannot hold us past
        // the deadline. (Killing it would not reliably unblock a blocking
        // write: anything it spawned may still hold the pipe open.)
        if let Err(e) = set_nonblocking(&stdin) {
            let _ = child.kill();
            let _ = child.wait();
            return Some(Err(format!("{program}: {e}")));
        }

        // Pushed back by every write that makes progress, so a large payload
        // has as long as it needs, and only a stall times out.
        let mut deadline = Instant::now() + timeout;
        let mut remaining = bytes;
        let mut stdin = Some(stdin);
        let mut write_error = None;
        loop {
            let mut progressed = false;
            if let Some(pipe) = stdin.as_mut() {
                match pipe.write(remaining) {
                    Ok(written) => {
                        remaining = &remaining[written..];
                        progressed = written > 0;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => write_error = Some(e),
                }
                // Closing stdin is what tells the helper the payload is
                // complete; after a write error there is nothing more to say.
                if remaining.is_empty() || write_error.is_some() {
                    stdin = None;
                }
            }
            if progressed {
                deadline = Instant::now() + timeout;
            }

            match child.try_wait() {
                // The helpers fork a daemon to serve the selection and exit
                // at once, so their exit status says whether the copy took.
                Ok(Some(status)) if !status.success() => {
                    return Some(Err(format!("{program} failed ({status})")));
                }
                Ok(Some(_)) => {
                    return Some(match write_error {
                        Some(e) => Err(format!("{program}: {e}")),
                        None if !remaining.is_empty() => Err(format!(
                            "{program} exited before reading the clipboard data"
                        )),
                        None => Ok(()),
                    });
                }
                // Straight back to writing while the helper keeps reading;
                // a pause only while nothing moves.
                Ok(None) if Instant::now() < deadline => {
                    if !progressed {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                // Still running with the whole payload delivered: it is
                // serving the selection in the foreground.
                Ok(None) if stdin.is_none() && write_error.is_none() => return Some(Ok(())),
                Ok(None) => {
                    drop(stdin);
                    let _ = child.kill();
                    let _ = child.wait();
                    return Some(Err(match write_error {
                        Some(e) => format!("{program}: {e}"),
                        None => format!("{program} stopped reading the clipboard data"),
                    }));
                }
                Err(e) => {
                    drop(stdin);
                    let _ = child.kill();
                    let _ = child.wait();
                    return Some(Err(format!("{program}: {e}")));
                }
            }
        }
    }

    fn set_nonblocking(pipe: &std::process::ChildStdin) -> std::io::Result<()> {
        let fd = pipe.as_raw_fd();
        // SAFETY: `fd` is a valid descriptor owned by `pipe` for this call;
        // F_GETFL/F_SETFL only change its status flags.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux::*;
    use super::{install_hint_for, Payload};
    use image::{Rgba, RgbaImage};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    const SHORT: Duration = Duration::from_millis(200);

    #[test]
    fn png_encoding_round_trips_every_channel() {
        // Translucent on purpose: the helpers must receive RGBA, not RGB.
        let image = RgbaImage::from_fn(7, 3, |x, y| Rgba([x as u8, y as u8, 128, 64]));
        let bytes = encode_png(&image).expect("encodes");
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .expect("valid PNG")
            .into_rgba8();
        assert_eq!(decoded, image);
    }

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
    fn the_install_hint_names_the_package_with_the_sessions_helper() {
        assert_eq!(
            install_hint_for(Payload::Text, true),
            "install wl-clipboard"
        );
        assert_eq!(
            install_hint_for(Payload::Text, false),
            "install xclip or xsel"
        );
    }

    #[test]
    fn the_install_hint_for_an_image_names_only_a_helper_that_copies_images() {
        assert_eq!(install_hint_for(Payload::Png, true), "install wl-clipboard");
        assert_eq!(install_hint_for(Payload::Png, false), "install xclip");
    }

    #[test]
    fn a_helper_missing_at_spawn_falls_through() {
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
    fn a_helper_that_stops_reading_times_out_instead_of_hanging() {
        // Far larger than a pipe buffer, to a helper that never reads it.
        let payload = vec![0u8; 8 << 20];
        let start = Instant::now();
        let result = pipe_to("sh", &["-c", "sleep 30"], &payload, SHORT);
        assert!(
            matches!(result, Some(Err(ref e)) if e.contains("stopped reading")),
            "{result:?}"
        );
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn only_installed_helpers_are_picked() {
        let dir = std::env::temp_dir().join(format!("screen-ruler-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let helpers = helpers(Payload::Text, false);

        let path = std::env::join_paths([&dir]).expect("path");
        assert_eq!(
            first_installed(helpers, &path),
            None,
            "nothing installed yet"
        );

        // A non-executable file does not count; an executable one does.
        let xsel = dir.join("xsel");
        std::fs::write(&xsel, "").expect("write");
        assert_eq!(first_installed(helpers, &path), None);
        std::fs::set_permissions(&xsel, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert_eq!(first_installed(helpers, &path).map(|h| h.0), Some("xsel"));

        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn a_helper_that_reads_slowly_but_never_stalls_gets_the_whole_payload() {
        // A 64 KiB block every 100 ms: never 200 ms without progress, ~600 ms in all.
        let payload = vec![0u8; 6 * 65536];
        let script = "for i in 1 2 3 4 5 6 7; do dd bs=65536 count=1 status=none of=/dev/null; sleep 0.1; done";
        assert_eq!(
            pipe_to("sh", &["-c", script], &payload, SHORT),
            Some(Ok(()))
        );
    }
}
