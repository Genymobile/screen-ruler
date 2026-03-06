#!/usr/bin/env python3
"""
Smart Edge-Detection Screen Ruler
===================================
Captures a one-time screenshot at launch, builds a binary edge map using
Canny edge detection, then shows a transparent full-screen overlay that
tracks the mouse cursor and projects four rays (N/S/E/W) until each ray
hits an edge pixel (or the screen boundary).  The sum of the East + West
ray lengths is displayed as the horizontal width W, and North + South as
the vertical height H.

Usage
-----
    python screen_ruler.py [--threshold-low N] [--threshold-high N]

Controls
--------
    Escape / Q : quit
"""

import sys
import os
import argparse
import atexit
import shutil
import subprocess
import tempfile

import numpy as np

try:
    import cv2
except ImportError:
    print(
        "Error: opencv-python-headless is required.\n"
        "Install with:  pip install opencv-python-headless",
        file=sys.stderr,
    )
    sys.exit(1)

from PyQt6.QtGui import QGuiApplication, QCursor, QImage, QClipboard
from PyQt6.QtCore import (
    Qt,
    QTimer,
    QRect,
    QUrl,
    QObject,
    QEventLoop,
    pyqtSignal,
    pyqtProperty,
    pyqtSlot,
)
from PyQt6.QtQml import QQmlApplicationEngine

# ---------------------------------------------------------------------------
# Timing constant
# ---------------------------------------------------------------------------

TIMER_INTERVAL_MS = 16                  # ≈ 60 FPS
CAPTURE_SETTLE_MS = 250                 # keep latest frame, then snapshot after settle window

# ---------------------------------------------------------------------------
# Pure-logic helpers (module-level so they can be unit-tested without a display)
# ---------------------------------------------------------------------------


def trace_ray(
    edge_map: np.ndarray, px: int, py: int, dx: int, dy: int
) -> int:
    """
    Trace a single ray on *edge_map* starting at pixel ``(px, py)`` and
    stepping by ``(dx, dy)`` each iteration.

    Returns the number of steps taken until either:
      * an edge pixel is reached (that pixel is included in the count), or
      * the ray walks off the edge-map boundary.

    Parameters
    ----------
    edge_map : 2-D boolean ndarray, shape ``(H, W)``
    px, py   : starting column / row (in edge-map pixel coordinates)
    dx, dy   : step direction; each should be -1, 0, or +1

    Returns
    -------
    int
        Ray length in edge-map pixels (≥ 0).
    """
    map_h, map_w = edge_map.shape
    cx, cy = px, py
    dist = 0
    while True:
        nx, ny = cx + dx, cy + dy
        if nx < 0 or nx >= map_w or ny < 0 or ny >= map_h:
            break            # hit the screen boundary
        dist += 1
        if edge_map[ny, nx]:
            break            # hit an edge pixel
        cx, cy = nx, ny
    return dist


def compute_edge_map(
    qimage: QImage,
    threshold_low: int = 50,
    threshold_high: int = 150,
) -> np.ndarray:
    """
    Convert a ``QImage`` to a boolean edge map using Canny edge detection.

    Why is Gaussian blur applied before ``cv2.Canny``?
    ---------------------------------------------------
    Two distinct reasons:

    1. **OpenCV's Canny does not blur internally.**
       The original 1986 Canny paper specifies Gaussian smoothing as its first
       step, but OpenCV's ``cv2.Canny`` skips that step and goes straight to
       the Sobel gradient computation.  Pre-smoothing is left to the caller,
       which is the standard usage pattern recommended by the OpenCV docs.

    2. **Screen captures are inherently noisy.**
       A desktop screenshot contains many sources of high-frequency variation
       that are *not* meaningful UI edges: sub-pixel font anti-aliasing, texture
       gradients in wallpapers, JPEG/PNG compression ringing, and icon detail.
       Without the blur, Canny fires on all of these, producing an edge map so
       dense that every crosshair ray stops within one or two pixels of the
       cursor — rendering the ruler useless.  Even raising the Canny thresholds
       cannot fully suppress this noise because the thresholds gate hysteresis
       propagation, not local gradient spikes caused by individual noisy pixels.

    A small kernel (3×3) is intentionally used so that genuine sharp UI
    boundaries (button outlines, panel separators) are preserved while
    single-pixel noise is smoothed away.

    Parameters
    ----------
    qimage          : source image (any Qt format; converted internally)
    threshold_low   : lower hysteresis threshold for ``cv2.Canny``
    threshold_high  : upper hysteresis threshold for ``cv2.Canny``

    Returns
    -------
    np.ndarray of dtype bool, shape ``(H, W)``
    """
    if qimage is None or qimage.isNull() or qimage.width() <= 0 or qimage.height() <= 0:
        raise ValueError("Cannot compute edge map from an empty screenshot.")

    qimage = qimage.convertToFormat(QImage.Format.Format_RGB888)
    width = qimage.width()
    height = qimage.height()
    # Qt may add per-row padding; bytesPerLine() is the actual stride.
    bytes_per_line = qimage.bytesPerLine()
    ptr = qimage.bits()
    if ptr is None:
        raise ValueError("Cannot access screenshot pixel data.")
    if hasattr(ptr, "setsize"):
        ptr.setsize(bytes_per_line * height)
    # Read every row at full stride width, then discard the padding bytes.
    raw = np.frombuffer(
        ptr,
        dtype=np.uint8,
        count=bytes_per_line * height,
    ).reshape((height, bytes_per_line))
    img = raw[:, : width * 3].reshape((height, width, 3)).copy()

    gray = cv2.cvtColor(img, cv2.COLOR_RGB2GRAY)
    # Pre-blur: cv2.Canny has no internal smoothing step, and raw screen
    # captures contain too much high-frequency noise to use Canny directly.
    blurred = cv2.GaussianBlur(gray, (3, 3), 0)
    edges = cv2.Canny(blurred, threshold_low, threshold_high)
    return edges.astype(bool)


def capture_screen(app: QGuiApplication) -> QImage:
    """
    Grab a screenshot covering all connected monitors and return it as a
    ``QImage`` at the display's physical (device-pixel) resolution.
    """
    screens = app.screens()
    if not screens:
        return QImage()

    # Build the bounding rectangle that encompasses every screen
    all_rect = QRect()
    for screen in screens:
        all_rect = all_rect.united(screen.geometry())

    if all_rect.width() <= 0 or all_rect.height() <= 0:
        return QImage()

    session_type = os.environ.get("XDG_SESSION_TYPE", "").strip().lower()

    if session_type == "wayland":
        image = _capture_screen_qt_native(app)
        if not image.isNull() and image.width() > 0 and image.height() > 0:
            return image

    primary = app.primaryScreen()
    if primary is not None:
        pixmap = primary.grabWindow(
            0,
            all_rect.x(),
            all_rect.y(),
            all_rect.width(),
            all_rect.height(),
        )
        image = pixmap.toImage()
        if not image.isNull() and image.width() > 0 and image.height() > 0:
            return image

    return _capture_screen_external(all_rect)


def _capture_screen_qt_native(
    app: QGuiApplication,
    timeout_ms: int = 12000,
) -> QImage:
    """Try Qt Multimedia native screen capture (portal-based on Wayland)."""
    try:
        from PyQt6.QtMultimedia import QScreenCapture, QMediaCaptureSession, QVideoSink
    except Exception:
        return QImage()

    screen = app.primaryScreen()
    if screen is None:
        return QImage()

    capture = QScreenCapture()
    session = QMediaCaptureSession()
    sink = QVideoSink()

    session.setScreenCapture(capture)
    session.setVideoOutput(sink)
    capture.setScreen(screen)

    result: dict[str, QImage] = {
        "image": QImage(),
        "latest": QImage(),
    }
    loop = QEventLoop()
    timer = QTimer()
    timer.setSingleShot(True)
    settle_timer = QTimer()
    settle_timer.setSingleShot(True)

    def finish() -> None:
        if loop.isRunning():
            loop.quit()

    def on_frame(frame) -> None:
        if not frame.isValid():
            return
        image = frame.toImage()
        if image.isNull() or image.width() <= 0 or image.height() <= 0:
            return
        result["latest"] = image
        if not settle_timer.isActive():
            settle_timer.start(CAPTURE_SETTLE_MS)

    def on_settled() -> None:
        latest = result["latest"]
        if latest.isNull() or latest.width() <= 0 or latest.height() <= 0:
            return
        result["image"] = latest
        finish()

    sink.videoFrameChanged.connect(on_frame)
    capture.errorOccurred.connect(lambda *_: finish())
    timer.timeout.connect(finish)
    settle_timer.timeout.connect(on_settled)

    timer.start(timeout_ms)
    capture.start()
    loop.exec()
    capture.stop()

    return result["image"]


def _qt_native_capture_supported() -> bool:
    try:
        from PyQt6.QtMultimedia import QScreenCapture, QMediaCaptureSession, QVideoSink
    except Exception:
        return False
    return all((QScreenCapture, QMediaCaptureSession, QVideoSink))


def _capture_screen_external(all_rect: QRect) -> QImage:
    """Try an external screenshot tool based on the active Linux session type."""
    session_type = os.environ.get("XDG_SESSION_TYPE", "").strip().lower()
    command = None

    if session_type == "wayland":
        grim = shutil.which("grim")
        if grim:
            geometry = (
                f"{all_rect.x()},{all_rect.y()} "
                f"{all_rect.width()}x{all_rect.height()}"
            )
            command = [grim, "-g", geometry, "-t", "png", "-"]
    elif session_type == "x11":
        imagemagick_import = shutil.which("import")
        if imagemagick_import:
            command = [imagemagick_import, "-window", "root", "png:-"]

    if not command:
        return QImage()

    try:
        result = subprocess.run(
            command,
            check=True,
            capture_output=True,
            timeout=5,
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, OSError):
        return QImage()

    if not result.stdout:
        return QImage()

    image = QImage.fromData(result.stdout, "PNG")
    if image.isNull() or image.width() <= 0 or image.height() <= 0:
        return QImage()
    return image


def _capture_tool_hint() -> str | None:
    """Return a user-facing hint about optional external screenshot tools."""
    session_type = os.environ.get("XDG_SESSION_TYPE", "").strip().lower()
    if session_type == "wayland":
        if _qt_native_capture_supported():
            return (
                "Tip: when prompted, allow desktop screen-capture authorization "
                "for this app."
            )
        if not shutil.which("grim"):
            return "Tip: install 'grim' to enable Wayland fallback screenshot capture."
    if session_type == "x11" and not shutil.which("import"):
        return "Tip: install ImageMagick ('import') to enable X11 fallback screenshot capture."
    return None


def _edge_map_to_qimage(edge_map: np.ndarray) -> QImage:
    """Convert a boolean edge map to a displayable grayscale ``QImage``."""
    if edge_map.ndim != 2 or edge_map.shape[0] <= 0 or edge_map.shape[1] <= 0:
        raise ValueError("Edge map must be a non-empty 2-D array.")

    grayscale = (edge_map.astype(np.uint8) * 255).copy(order="C")
    height, width = grayscale.shape
    image = QImage(
        grayscale.data,
        width,
        height,
        width,
        QImage.Format.Format_Grayscale8,
    )
    return image.copy()


def _create_temp_image_source(image: QImage, prefix: str) -> str | None:
    """Persist a QImage as a temp PNG and return a file URL for QML."""
    if image.isNull() or image.width() <= 0 or image.height() <= 0:
        return None

    fd, path = tempfile.mkstemp(prefix=prefix, suffix=".png")
    os.close(fd)

    if not image.save(path, "PNG"):
        try:
            os.unlink(path)
        except OSError:
            pass
        return None

    def _cleanup() -> None:
        try:
            os.unlink(path)
        except OSError:
            pass

    atexit.register(_cleanup)
    return QUrl.fromLocalFile(path).toString()


def _create_debug_edge_overlay_source(edge_map: np.ndarray) -> str | None:
    """Persist the edge map as a temp PNG and return a file URL for QML."""
    image = _edge_map_to_qimage(edge_map)
    return _create_temp_image_source(image, "screen-ruler-edge-")


def _create_screenshot_overlay_source(qimage: QImage) -> str | None:
    """Persist the captured screenshot as a temp PNG and return a file URL."""
    return _create_temp_image_source(qimage, "screen-ruler-shot-")


# ---------------------------------------------------------------------------
# QML backend: exposes ruler data as Qt properties for QML bindings
# ---------------------------------------------------------------------------


class RulerBackend(QObject):
    """
    QObject that exposes cursor position and measurement data to QML.

    All properties notify via a single ``dataChanged`` signal so that
    QML ``Connections`` handlers and property bindings are re-evaluated
    on every cursor move.
    """

    dataChanged = pyqtSignal()
    staticChanged = pyqtSignal()

    def __init__(
        self,
        edge_map: np.ndarray,
        dpr_x: float,
        dpr_y: float,
        virtual_x: int,
        virtual_y: int,
        virtual_w: int,
        virtual_h: int,
        screenshot_source: str = "",
        debug_overlay_source: str = "",
    ) -> None:
        super().__init__()
        self._edge_map = edge_map
        self._dpr_x = dpr_x
        self._dpr_y = dpr_y
        self._vx = virtual_x
        self._vy = virtual_y
        self._vw = virtual_w
        self._vh = virtual_h
        self._screenshot_source = screenshot_source
        self._debug_overlay_source = debug_overlay_source
        self._is_wayland = os.environ.get("XDG_SESSION_TYPE", "").strip().lower() == "wayland"
        self._cx: int = -1
        self._cy: int = -1
        self._d_n: int = 0
        self._d_s: int = 0
        self._d_w: int = 0
        self._d_e: int = 0
        self._W: int = 0
        self._H: int = 0

    # ------------------------------------------------------------------
    # Properties read by QML
    # ------------------------------------------------------------------

    @pyqtProperty(int, notify=dataChanged)
    def cursorX(self) -> int:
        return self._cx

    @pyqtProperty(int, notify=dataChanged)
    def cursorY(self) -> int:
        return self._cy

    @pyqtProperty(int, notify=dataChanged)
    def northEnd(self) -> int:
        return self._cy - self._d_n if self._cy >= 0 else 0

    @pyqtProperty(int, notify=dataChanged)
    def southEnd(self) -> int:
        return self._cy + self._d_s if self._cy >= 0 else 0

    @pyqtProperty(int, notify=dataChanged)
    def westEnd(self) -> int:
        return self._cx - self._d_w if self._cx >= 0 else 0

    @pyqtProperty(int, notify=dataChanged)
    def eastEnd(self) -> int:
        return self._cx + self._d_e if self._cx >= 0 else 0

    @pyqtProperty(int, notify=dataChanged)
    def widthPx(self) -> int:
        return self._W

    @pyqtProperty(int, notify=dataChanged)
    def heightPx(self) -> int:
        return self._H

    @pyqtProperty(int, notify=staticChanged)
    def virtualDesktopX(self) -> int:
        return self._vx

    @pyqtProperty(int, notify=staticChanged)
    def virtualDesktopY(self) -> int:
        return self._vy

    @pyqtProperty(int, notify=staticChanged)
    def virtualDesktopWidth(self) -> int:
        return self._vw

    @pyqtProperty(int, notify=staticChanged)
    def virtualDesktopHeight(self) -> int:
        return self._vh

    @pyqtProperty(bool, notify=staticChanged)
    def debugOverlayEnabled(self) -> bool:
        return bool(self._debug_overlay_source)

    @pyqtProperty(str, notify=staticChanged)
    def debugOverlaySource(self) -> str:
        return self._debug_overlay_source

    @pyqtProperty(bool, notify=staticChanged)
    def screenshotAvailable(self) -> bool:
        return bool(self._screenshot_source)

    @pyqtProperty(str, notify=staticChanged)
    def screenshotSource(self) -> str:
        return self._screenshot_source

    @pyqtProperty(bool, notify=staticChanged)
    def isWaylandSession(self) -> bool:
        return self._is_wayland

    # ------------------------------------------------------------------
    # Cursor polling (called by QTimer at ~60 Hz)
    # ------------------------------------------------------------------

    def poll(self) -> None:
        """Re-trace rays from the current cursor position and emit dataChanged."""
        pos = QCursor.pos()
        global_x, global_y = pos.x(), pos.y()
        lx = global_x - self._vx
        ly = global_y - self._vy
        if lx == self._cx and ly == self._cy:
            return

        map_h, map_w = self._edge_map.shape
        ex = max(0, min(int(lx * self._dpr_x), map_w - 1))
        ey = max(0, min(int(ly * self._dpr_y), map_h - 1))

        d_n = trace_ray(self._edge_map, ex, ey, 0, -1)
        d_s = trace_ray(self._edge_map, ex, ey, 0, 1)
        d_w = trace_ray(self._edge_map, ex, ey, -1, 0)
        d_e = trace_ray(self._edge_map, ex, ey, 1, 0)

        self._cx = lx
        self._cy = ly
        self._d_n = int(d_n / self._dpr_y)
        self._d_s = int(d_s / self._dpr_y)
        self._d_w = int(d_w / self._dpr_x)
        self._d_e = int(d_e / self._dpr_x)
        self._W = self._d_w + self._d_e
        self._H = self._d_n + self._d_s

        self.dataChanged.emit()

    @pyqtSlot()
    def copySizeToClipboardAndQuit(self) -> None:
        self.poll()
        text = f"{self._W} × {self._H}"
        clipboard = QGuiApplication.clipboard()
        if clipboard is not None:
            clipboard.setText(text, QClipboard.Mode.Clipboard)
            try:
                clipboard.setText(text, QClipboard.Mode.Selection)
            except Exception:
                pass

        if (
            os.environ.get("XDG_SESSION_TYPE", "").strip().lower() == "wayland"
            and shutil.which("wl-copy")
        ):
            try:
                subprocess.run(
                    ["wl-copy"],
                    input=text.encode("utf-8"),
                    check=True,
                    timeout=1,
                )
            except Exception:
                pass

        QTimer.singleShot(120, QGuiApplication.quit)


# ---------------------------------------------------------------------------
# CLI entry point
# ---------------------------------------------------------------------------


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Smart Edge-Detection Screen Ruler — "
            "move the mouse to measure distances between UI edges."
        )
    )
    parser.add_argument(
        "--threshold-low",
        type=int,
        default=50,
        metavar="N",
        help="Lower Canny edge-detection threshold (default: 50)",
    )
    parser.add_argument(
        "--threshold-high",
        type=int,
        default=150,
        metavar="N",
        help="Upper Canny edge-detection threshold (default: 150)",
    )
    parser.add_argument(
        "--debug-edge-overlay",
        action="store_true",
        help=(
            "Show the captured Canny edge map as a 30%-opacity overlay "
            "for alignment debugging"
        ),
    )
    return parser.parse_args()


def _find_qml() -> str:
    """Return the absolute path to screen_ruler.qml.

    Works both when running as a plain Python script and inside a
    PyInstaller one-file bundle (where data files land in ``sys._MEIPASS``).
    """
    if getattr(sys, "frozen", False) and hasattr(sys, "_MEIPASS"):
        base = getattr(sys, "_MEIPASS")
    else:
        base = os.path.dirname(os.path.abspath(__file__))
    return os.path.join(base, "screen_ruler.qml")


def main() -> None:
    args = parse_args()

    app = QGuiApplication(sys.argv)
    app.setApplicationName("Screen Ruler")

    # Compute virtual desktop bounds (union of all screen geometries)
    all_rect = QRect()
    for screen in app.screens():
        all_rect = all_rect.united(screen.geometry())

    print("Capturing screen…")
    qimage = capture_screen(app)

    print(
        f"Computing edge map  "
        f"(Canny thresholds: {args.threshold_low} / {args.threshold_high})…"
    )
    try:
        edge_map = compute_edge_map(
            qimage, args.threshold_low, args.threshold_high
        )
    except ValueError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        print(
            "Screenshot capture may be unavailable in this desktop/session "
            "environment.",
            file=sys.stderr,
        )
        hint = _capture_tool_hint()
        if hint:
            print(hint, file=sys.stderr)
        sys.exit(1)
    print(
        f"Edge map: {edge_map.shape[1]}×{edge_map.shape[0]} px  |  "
        f"{edge_map.sum():,} edge pixels found."
    )

    if all_rect.width() <= 0 or all_rect.height() <= 0:
        dpr_x = 1.0
        dpr_y = 1.0
    else:
        dpr_x = edge_map.shape[1] / all_rect.width()
        dpr_y = edge_map.shape[0] / all_rect.height()

    debug_overlay_source = ""
    screenshot_source = _create_screenshot_overlay_source(qimage) or ""
    if not screenshot_source:
        print(
            "Warning: failed to create screenshot background image.",
            file=sys.stderr,
        )

    if args.debug_edge_overlay:
        debug_overlay_source = _create_debug_edge_overlay_source(edge_map) or ""
        if not debug_overlay_source:
            print(
                "Warning: failed to create debug edge overlay image.",
                file=sys.stderr,
            )

    ruler = RulerBackend(
        edge_map,
        dpr_x,
        dpr_y,
        all_rect.x(),
        all_rect.y(),
        all_rect.width(),
        all_rect.height(),
        screenshot_source,
        debug_overlay_source,
    )

    engine = QQmlApplicationEngine()
    engine.rootContext().setContextProperty("ruler", ruler)
    engine.load(QUrl.fromLocalFile(_find_qml()))

    if not engine.rootObjects():
        print("Error: failed to load QML UI", file=sys.stderr)
        sys.exit(1)

    # Start the cursor-polling timer
    timer = QTimer()
    timer.timeout.connect(ruler.poll)
    timer.start(TIMER_INTERVAL_MS)

    sys.exit(app.exec())


if __name__ == "__main__":
    main()
