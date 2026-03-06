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

from PyQt6.QtGui import QGuiApplication, QCursor, QImage
from PyQt6.QtCore import Qt, QTimer, QRect, QUrl, QObject, pyqtSignal, pyqtProperty
from PyQt6.QtQml import QQmlApplicationEngine

# ---------------------------------------------------------------------------
# Timing constant
# ---------------------------------------------------------------------------

TIMER_INTERVAL_MS = 16                  # ≈ 60 FPS

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

    primary = app.primaryScreen()
    pixmap = primary.grabWindow(
        0,
        all_rect.x(),
        all_rect.y(),
        all_rect.width(),
        all_rect.height(),
    )
    return pixmap.toImage()


# ---------------------------------------------------------------------------
# Optional X11 click-through support
# ---------------------------------------------------------------------------


def _try_set_click_through_x11(window) -> None:
    """
    Attempt to make *window* fully click-through on X11/Linux by
    setting an empty ``ShapeInput`` region via the XFixes extension.
    Silently ignored on non-X11 platforms or if the libraries are absent.
    """
    try:
        import ctypes
        import ctypes.util

        libx11_name = ctypes.util.find_library("X11")
        libxfixes_name = ctypes.util.find_library("Xfixes")
        if not libx11_name or not libxfixes_name:
            return

        x11 = ctypes.cdll.LoadLibrary(libx11_name)
        xfixes = ctypes.cdll.LoadLibrary(libxfixes_name)

        x11.XOpenDisplay.restype = ctypes.c_void_p
        xfixes.XFixesCreateRegion.restype = ctypes.c_ulong
        xfixes.XFixesSetWindowShapeRegion.argtypes = [
            ctypes.c_void_p,  # display
            ctypes.c_ulong,   # window id
            ctypes.c_int,     # shape_kind  (ShapeInput = 2)
            ctypes.c_int,     # x_off
            ctypes.c_int,     # y_off
            ctypes.c_ulong,   # region
        ]

        display = x11.XOpenDisplay(None)
        if not display:
            return

        win_id = int(window.winId())
        region = xfixes.XFixesCreateRegion(display, None, 0)
        xfixes.XFixesSetWindowShapeRegion(display, win_id, 2, 0, 0, region)
        xfixes.XFixesDestroyRegion(display, region)
        x11.XFlush(display)
        x11.XCloseDisplay(display)
    except Exception:
        pass  # click-through is a best-effort feature


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

    def __init__(
        self,
        edge_map: np.ndarray,
        dpr_x: float,
        dpr_y: float,
        virtual_x: int,
        virtual_y: int,
        virtual_w: int,
        virtual_h: int,
    ) -> None:
        super().__init__()
        self._edge_map = edge_map
        self._dpr_x = dpr_x
        self._dpr_y = dpr_y
        self._vx = virtual_x
        self._vy = virtual_y
        self._vw = virtual_w
        self._vh = virtual_h
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

    @pyqtProperty(int, notify=dataChanged)
    def virtualDesktopX(self) -> int:
        return self._vx

    @pyqtProperty(int, notify=dataChanged)
    def virtualDesktopY(self) -> int:
        return self._vy

    @pyqtProperty(int, notify=dataChanged)
    def virtualDesktopWidth(self) -> int:
        return self._vw

    @pyqtProperty(int, notify=dataChanged)
    def virtualDesktopHeight(self) -> int:
        return self._vh

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

    ruler = RulerBackend(
        edge_map,
        dpr_x,
        dpr_y,
        all_rect.x(),
        all_rect.y(),
        all_rect.width(),
        all_rect.height(),
    )

    engine = QQmlApplicationEngine()
    engine.rootContext().setContextProperty("ruler", ruler)
    engine.load(QUrl.fromLocalFile(_find_qml()))

    if not engine.rootObjects():
        print("Error: failed to load QML UI", file=sys.stderr)
        sys.exit(1)

    # Apply X11 click-through after the QQuickWindow is created
    root_window = engine.rootObjects()[0]
    _try_set_click_through_x11(root_window)

    # Start the cursor-polling timer
    timer = QTimer()
    timer.timeout.connect(ruler.poll)
    timer.start(TIMER_INTERVAL_MS)

    sys.exit(app.exec())


if __name__ == "__main__":
    main()
