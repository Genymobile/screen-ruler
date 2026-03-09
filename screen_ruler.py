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
    python screen_ruler.py [--threshold-low N] [--threshold-high N] [--debug-edge-overlay]

Controls
--------
    Escape / Q : quit
    Left click : copy current "W × H px" to clipboard and quit
    Top slider : live sensitivity tuning (recomputes edge map)
                 and shows a short edge-map preview animation
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
SENSITIVITY_RECOMPUTE_DEBOUNCE_MS = 30
REGION_CLOSE_KERNEL_SIZE = 3            # close tiny gaps before connected components
REGION_DILATE_ITERATIONS = 0            # keep container bounds closer to raw edges

SENSITIVITY_LOW_AT_100 = 5
SENSITIVITY_HIGH_AT_100 = 25
SENSITIVITY_LOW_AT_0 = 80
SENSITIVITY_HIGH_AT_0 = 220

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
    if dx == 0 and dy == 0:
        raise ValueError("trace_ray requires a non-zero direction vector.")

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

     The current pipeline applies Canny directly to grayscale pixels so subtle
     UI transitions are preserved as candidate edges.

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
    edges = cv2.Canny(gray, threshold_low, threshold_high)
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


def _sensitivity_to_thresholds(sensitivity: float) -> tuple[int, int]:
    """Map user-facing sensitivity (0..100) to Canny thresholds."""
    s = max(0.0, min(100.0, sensitivity))
    low_span = SENSITIVITY_LOW_AT_0 - SENSITIVITY_LOW_AT_100
    high_span = SENSITIVITY_HIGH_AT_0 - SENSITIVITY_HIGH_AT_100
    low = int(round(SENSITIVITY_LOW_AT_100 + (100 - s) * (low_span / 100.0)))
    high = int(round(SENSITIVITY_HIGH_AT_100 + (100 - s) * (high_span / 100.0)))
    if high <= low:
        high = min(255, low + 1)
    return low, high


def _thresholds_to_sensitivity(threshold_low: int, threshold_high: int) -> float:
    """Approximate sensitivity (0..100) from Canny thresholds."""
    low_scale = (SENSITIVITY_LOW_AT_0 - SENSITIVITY_LOW_AT_100) / 100.0
    high_scale = (SENSITIVITY_HIGH_AT_0 - SENSITIVITY_HIGH_AT_100) / 100.0
    s_from_low = 100.0 - ((threshold_low - SENSITIVITY_LOW_AT_100) / low_scale)
    s_from_high = 100.0 - ((threshold_high - SENSITIVITY_HIGH_AT_100) / high_scale)
    return max(0.0, min(100.0, (s_from_low + s_from_high) / 2.0))


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
    controlsChanged = pyqtSignal()
    edgeMapPreviewRequested = pyqtSignal()

    def __init__(
        self,
        edge_map: np.ndarray,
        dpr_x: float,
        dpr_y: float,
        virtual_x: int,
        virtual_y: int,
        virtual_w: int,
        virtual_h: int,
        source_image: QImage,
        threshold_low: int,
        threshold_high: int,
        always_show_debug_overlay: bool,
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
        self._source_image = source_image
        self._threshold_low = threshold_low
        self._threshold_high = threshold_high
        self._sensitivity = _thresholds_to_sensitivity(threshold_low, threshold_high)
        self._always_show_debug_overlay = always_show_debug_overlay
        self._screenshot_source = screenshot_source
        self._debug_overlay_source = debug_overlay_source
        self._overlay_version = 0
        self._is_wayland = os.environ.get("XDG_SESSION_TYPE", "").strip().lower() == "wayland"
        self._cx: int = -1
        self._cy: int = -1
        self._d_n: int = 0
        self._d_s: int = 0
        self._d_w: int = 0
        self._d_e: int = 0
        self._W: int = 0
        self._H: int = 0
        self._region_labels: np.ndarray | None = None
        self._region_stats: np.ndarray | None = None

        self._recompute_timer = QTimer(self)
        self._recompute_timer.setSingleShot(True)
        self._recompute_timer.timeout.connect(self._recompute_edge_map)
        self._recompute_regions()

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

    @pyqtProperty(bool, notify=controlsChanged)
    def debugOverlayEnabled(self) -> bool:
        return self._always_show_debug_overlay and bool(self._debug_overlay_source)

    @pyqtProperty(str, notify=controlsChanged)
    def debugOverlaySource(self) -> str:
        return self._debug_overlay_source

    @pyqtProperty(float, notify=controlsChanged)
    def sensitivity(self) -> float:
        return self._sensitivity

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
        self._update_measurement(force=False)

    def _update_measurement(self, force: bool) -> None:
        """Re-trace rays from the current cursor position and emit dataChanged."""
        pos = QCursor.pos()
        global_x, global_y = pos.x(), pos.y()
        lx = global_x - self._vx
        ly = global_y - self._vy
        if not force and lx == self._cx and ly == self._cy:
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
        self._d_n = int(round(d_n / self._dpr_y))
        self._d_s = int(round(d_s / self._dpr_y))
        self._d_w = int(round(d_w / self._dpr_x))
        self._d_e = int(round(d_e / self._dpr_x))
        self._W = int(round((d_w + d_e) / self._dpr_x))
        self._H = int(round((d_n + d_s) / self._dpr_y))

        self.dataChanged.emit()

    @pyqtSlot(float)
    def setSensitivity(self, sensitivity: float) -> None:
        clamped = max(0.0, min(100.0, float(sensitivity)))
        if abs(clamped - self._sensitivity) < 0.01:
            return
        self._sensitivity = clamped
        self._threshold_low, self._threshold_high = _sensitivity_to_thresholds(clamped)
        self.controlsChanged.emit()
        self._recompute_timer.start(SENSITIVITY_RECOMPUTE_DEBOUNCE_MS)

    @pyqtSlot(float, float, float, result="QVariant")
    def snapPointToNearestEdge(
        self,
        local_x: float,
        local_y: float,
        max_distance_px: float = 5.0,
    ) -> dict[str, float | bool]:
        """Snap a local point to nearby horizontal/vertical edge lines.

        The search is line-oriented: X snapping looks for edge presence in
        nearby columns around the current row band, and Y snapping looks for
        edge presence in nearby rows around the current column band.
        Parameters are in QML-local pixels (not edge-map/device pixels).
        """
        map_h, map_w = self._edge_map.shape
        x = float(local_x)
        y = float(local_y)
        max_dist = max(0.0, float(max_distance_px))
        if max_dist <= 0.0:
            return {"x": x, "y": y, "snapped": False}

        ex = max(0, min(int(round(x * self._dpr_x)), map_w - 1))
        ey = max(0, min(int(round(y * self._dpr_y)), map_h - 1))

        search_rx = max(1, int(np.ceil(max_dist * self._dpr_x)))
        search_ry = max(1, int(np.ceil(max_dist * self._dpr_y)))
        min_x = max(0, ex - search_rx)
        max_x = min(map_w - 1, ex + search_rx)
        min_y = max(0, ey - search_ry)
        max_y = min(map_h - 1, ey + search_ry)

        # Search along the local row/column (with a tiny orthogonal band) so
        # snapping behaves like "snap to nearby vertical/horizontal line".
        # This is more stable near corners than selecting x/y from arbitrary
        # edge pixels in a 2-D window.
        band_y = max(1, int(np.ceil(self._dpr_y)))
        band_x = max(1, int(np.ceil(self._dpr_x)))
        row_min = max(0, ey - band_y)
        row_max = min(map_h - 1, ey + band_y)
        col_min = max(0, ex - band_x)
        col_max = min(map_w - 1, ex + band_x)

        best_x_delta = None
        best_x_map = None
        for mx in range(min_x, max_x + 1):
            if not np.any(self._edge_map[row_min:row_max + 1, mx]):
                continue
            delta = abs(mx - ex)
            if best_x_delta is None or delta < best_x_delta:
                best_x_delta = delta
                best_x_map = mx

        best_y_delta = None
        best_y_map = None
        for my in range(min_y, max_y + 1):
            if not np.any(self._edge_map[my, col_min:col_max + 1]):
                continue
            delta = abs(my - ey)
            if best_y_delta is None or delta < best_y_delta:
                best_y_delta = delta
                best_y_map = my

        snapped_x = x if best_x_map is None else (best_x_map / self._dpr_x)
        snapped_y = y if best_y_map is None else (best_y_map / self._dpr_y)
        snapped = best_x_map is not None or best_y_map is not None
        return {"x": snapped_x, "y": snapped_y, "snapped": snapped}

    def _recompute_regions(self) -> None:
        """Build connected-component labels for non-edge free-space regions."""
        edge_u8 = self._edge_map.astype(np.uint8)
        kernel = np.ones((REGION_CLOSE_KERNEL_SIZE, REGION_CLOSE_KERNEL_SIZE), dtype=np.uint8)
        closed = cv2.morphologyEx(edge_u8, cv2.MORPH_CLOSE, kernel)
        barriers = cv2.dilate(closed, kernel, iterations=REGION_DILATE_ITERATIONS)
        free_space = (barriers == 0).astype(np.uint8)
        _, labels, stats, _ = cv2.connectedComponentsWithStats(
            free_space,
            connectivity=4,
        )
        self._region_labels = labels
        self._region_stats = stats

    def _region_label_near(self, ex: int, ey: int, max_radius: int = 3) -> int:
        """Return a non-zero free-space region label near the map point."""
        if self._region_labels is None or self._region_stats is None:
            return 0

        map_h, map_w = self._region_labels.shape
        label = int(self._region_labels[ey, ex])
        if label > 0:
            return label

        best_label = 0
        best_dist2 = None
        best_area = None
        for radius in range(1, max_radius + 1):
            min_x = max(0, ex - radius)
            max_x = min(map_w - 1, ex + radius)
            min_y = max(0, ey - radius)
            max_y = min(map_h - 1, ey + radius)
            window = self._region_labels[min_y:max_y + 1, min_x:max_x + 1]
            ys, xs = np.where(window > 0)
            if ys.size == 0:
                continue

            for index in range(ys.size):
                my = int(min_y + ys[index])
                mx = int(min_x + xs[index])
                cand_label = int(self._region_labels[my, mx])
                if cand_label <= 0:
                    continue
                dx = mx - ex
                dy = my - ey
                dist2 = dx * dx + dy * dy
                area = int(self._region_stats[cand_label, cv2.CC_STAT_AREA])
                if (
                    best_dist2 is None
                    or dist2 < best_dist2
                    or (dist2 == best_dist2 and (best_area is None or area < best_area))
                ):
                    best_dist2 = dist2
                    best_area = area
                    best_label = cand_label

            if best_label > 0:
                return best_label

        return best_label

    @pyqtSlot(float, float, result="QVariant")
    def detectContainerAtPoint(self, local_x: float, local_y: float) -> dict[str, float | bool]:
        """Detect a container-like free-space region around a local point."""
        if self._region_labels is None or self._region_stats is None:
            return {
                "available": False,
                "x": 0.0,
                "y": 0.0,
                "width": 0.0,
                "height": 0.0,
            }

        map_h, map_w = self._edge_map.shape
        ex = max(0, min(int(round(float(local_x) * self._dpr_x)), map_w - 1))
        ey = max(0, min(int(round(float(local_y) * self._dpr_y)), map_h - 1))

        label = self._region_label_near(ex, ey)
        if label <= 0 or label >= int(self._region_stats.shape[0]):
            return {
                "available": False,
                "x": 0.0,
                "y": 0.0,
                "width": 0.0,
                "height": 0.0,
            }

        x_map, y_map, w_map, h_map, area = self._region_stats[label]
        if w_map <= 1 or h_map <= 1:
            return {
                "available": False,
                "x": 0.0,
                "y": 0.0,
                "width": 0.0,
                "height": 0.0,
            }

        # Avoid selecting the giant outside region when edges do not bound
        # an interior container around the pointer.
        if area >= int(map_w * map_h * 0.98):
            return {
                "available": False,
                "x": 0.0,
                "y": 0.0,
                "width": 0.0,
                "height": 0.0,
            }

        return {
            "available": True,
            "x": float(x_map) / self._dpr_x,
            "y": float(y_map) / self._dpr_y,
            "width": float(w_map) / self._dpr_x,
            "height": float(h_map) / self._dpr_y,
        }

    def _recompute_edge_map(self) -> None:
        try:
            self._edge_map = compute_edge_map(
                self._source_image,
                self._threshold_low,
                self._threshold_high,
            )
            self._recompute_regions()
            source = _create_debug_edge_overlay_source(self._edge_map)
            if source:
                self._overlay_version += 1
                self._debug_overlay_source = f"{source}?v={self._overlay_version}"
                self.controlsChanged.emit()
            self._update_measurement(force=True)
            self.edgeMapPreviewRequested.emit()
        except Exception:
            pass

    @pyqtSlot(str)
    def copyTextToClipboardAndQuit(self, text: str) -> None:
        text = str(text)

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
        default=16,
        metavar="N",
        help="Lower Canny edge-detection threshold (default: 16)",
    )
    parser.add_argument(
        "--threshold-high",
        type=int,
        default=54,
        metavar="N",
        help="Upper Canny edge-detection threshold (default: 54)",
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

    # Keep Qt Quick Controls in sync with the app accent palette.
    os.environ["QT_QUICK_CONTROLS_UNIVERSAL_ACCENT"] = "#E6195E"

    try:
        from PyQt6.QtQuickControls2 import QQuickStyle
        QQuickStyle.setStyle("Universal")
    except Exception:
        os.environ.setdefault("QT_QUICK_CONTROLS_STYLE", "Universal")

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

    debug_overlay_source = _create_debug_edge_overlay_source(edge_map) or ""
    screenshot_source = _create_screenshot_overlay_source(qimage) or ""
    if not screenshot_source:
        print(
            "Warning: failed to create screenshot background image.",
            file=sys.stderr,
        )
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
        qimage,
        args.threshold_low,
        args.threshold_high,
        args.debug_edge_overlay,
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
