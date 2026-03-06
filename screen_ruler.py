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

from PyQt5.QtWidgets import QApplication, QWidget
from PyQt5.QtCore import Qt, QTimer, QRect
from PyQt5.QtGui import (
    QPainter,
    QColor,
    QPen,
    QFont,
    QCursor,
    QImage,
)

# ---------------------------------------------------------------------------
# Visual constants
# ---------------------------------------------------------------------------

LINE_COLOR = QColor(0, 220, 255)        # cyan crosshair lines
LABEL_FG_COLOR = QColor(255, 255, 60)   # yellow text
LABEL_BG_COLOR = QColor(0, 0, 0, 160)  # semi-transparent black backdrop
LABEL_OFFSET_X = 14                     # pixels right of the cursor
LABEL_OFFSET_Y = 4                      # pixels below the cursor
FONT_NAME = "DejaVu Sans Mono"
FONT_SIZE = 13
CROSSHAIR_WIDTH = 1
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
    qimage = qimage.convertToFormat(QImage.Format_RGB888)
    width = qimage.width()
    height = qimage.height()
    # Qt may add per-row padding; bytesPerLine() is the actual stride.
    bytes_per_line = qimage.bytesPerLine()
    ptr = qimage.bits()
    ptr.setsize(bytes_per_line * height)
    # Read every row at full stride width, then discard the padding bytes.
    raw = np.frombuffer(ptr, dtype=np.uint8).reshape((height, bytes_per_line))
    img = raw[:, : width * 3].reshape((height, width, 3)).copy()

    gray = cv2.cvtColor(img, cv2.COLOR_RGB2GRAY)
    # Pre-blur: cv2.Canny has no internal smoothing step, and raw screen
    # captures contain too much high-frequency noise to use Canny directly.
    blurred = cv2.GaussianBlur(gray, (3, 3), 0)
    edges = cv2.Canny(blurred, threshold_low, threshold_high)
    return edges.astype(bool)


def capture_screen(app: QApplication) -> QImage:
    """
    Grab a screenshot covering all connected monitors and return it as a
    ``QImage`` at the display's physical (device-pixel) resolution.
    """
    # Build the bounding rectangle that encompasses every screen
    all_rect = QRect()
    for screen in app.screens():
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


def _try_set_click_through_x11(window: QWidget) -> None:
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
# Overlay widget
# ---------------------------------------------------------------------------


class ScreenRulerOverlay(QWidget):
    """
    Full-screen transparent overlay that renders the edge-detection crosshair
    and measurement labels in real time.
    """

    def __init__(self, edge_map: np.ndarray, device_pixel_ratio: float) -> None:
        super().__init__()
        self._edge_map = edge_map           # shape (H, W), dtype bool
        self._dpr = device_pixel_ratio
        self._cursor_x: int = -1
        self._cursor_y: int = -1
        self._init_window()
        self._init_timer()
        _try_set_click_through_x11(self)

    # ------------------------------------------------------------------
    # Initialisation
    # ------------------------------------------------------------------

    def _init_window(self) -> None:
        self.setWindowTitle("Screen Ruler")
        self.setWindowFlags(
            Qt.WindowStaysOnTopHint
            | Qt.FramelessWindowHint
            | Qt.Tool
            | Qt.X11BypassWindowManagerHint
        )
        self.setAttribute(Qt.WA_TranslucentBackground, True)
        self.setAttribute(Qt.WA_ShowWithoutActivating, True)

        # Span all monitors
        rect = QRect()
        for screen in QApplication.screens():
            rect = rect.united(screen.geometry())
        self.setGeometry(rect)

    def _init_timer(self) -> None:
        self._timer = QTimer(self)
        self._timer.timeout.connect(self._poll_cursor)
        self._timer.start(TIMER_INTERVAL_MS)

    # ------------------------------------------------------------------
    # Cursor polling
    # ------------------------------------------------------------------

    def _poll_cursor(self) -> None:
        pos = QCursor.pos()
        if pos.x() != self._cursor_x or pos.y() != self._cursor_y:
            self._cursor_x = pos.x()
            self._cursor_y = pos.y()
            self.update()

    # ------------------------------------------------------------------
    # Key handling
    # ------------------------------------------------------------------

    def keyPressEvent(self, event) -> None:  # noqa: N802
        if event.key() in (Qt.Key_Escape, Qt.Key_Q):
            QApplication.quit()

    # ------------------------------------------------------------------
    # Painting
    # ------------------------------------------------------------------

    def paintEvent(self, event) -> None:  # noqa: N802
        if self._cursor_x < 0:
            return

        lx = self._cursor_x   # logical cursor x
        ly = self._cursor_y   # logical cursor y

        # Map logical cursor position → edge-map (physical) coordinates
        map_h, map_w = self._edge_map.shape
        ex = max(0, min(int(lx * self._dpr), map_w - 1))
        ey = max(0, min(int(ly * self._dpr), map_h - 1))

        # Trace the four rays in edge-map pixel space
        d_n = trace_ray(self._edge_map, ex, ey, 0, -1)
        d_s = trace_ray(self._edge_map, ex, ey, 0, 1)
        d_w = trace_ray(self._edge_map, ex, ey, -1, 0)
        d_e = trace_ray(self._edge_map, ex, ey, 1, 0)

        # Convert ray lengths back to logical pixels for drawing
        draw_y_n = ly - int(d_n / self._dpr)
        draw_y_s = ly + int(d_s / self._dpr)
        draw_x_w = lx - int(d_w / self._dpr)
        draw_x_e = lx + int(d_e / self._dpr)

        # Physical-pixel measurements (what the user cares about)
        W = d_w + d_e
        H = d_n + d_s

        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing, False)

        # ---- Crosshair lines ----
        pen = QPen(LINE_COLOR, CROSSHAIR_WIDTH, Qt.SolidLine)
        pen.setCosmetic(True)
        painter.setPen(pen)
        painter.drawLine(lx, draw_y_n, lx, draw_y_s)   # vertical ray
        painter.drawLine(draw_x_w, ly, draw_x_e, ly)   # horizontal ray

        # ---- Measurement labels ----
        font = QFont(FONT_NAME, FONT_SIZE, QFont.Bold)
        painter.setFont(font)

        label_w = f"W: {W} px"
        label_h = f"H: {H} px"

        label_x = lx + LABEL_OFFSET_X
        label_y = ly + LABEL_OFFSET_Y

        fm = painter.fontMetrics()
        line_height = fm.height()
        pad = 3

        # Background rectangles (improve readability on any desktop)
        painter.setPen(Qt.NoPen)
        painter.setBrush(LABEL_BG_COLOR)
        for row, text in enumerate((label_w, label_h)):
            text_rect = fm.boundingRect(text)
            painter.drawRoundedRect(
                label_x - pad,
                label_y + row * (line_height + 2) - pad,
                text_rect.width() + 2 * pad,
                line_height + 2 * pad,
                3,
                3,
            )

        # Foreground text
        painter.setPen(QPen(LABEL_FG_COLOR))
        painter.drawText(label_x, label_y + fm.ascent(), label_w)
        painter.drawText(
            label_x, label_y + line_height + 2 + fm.ascent(), label_h
        )

        painter.end()


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


def main() -> None:
    args = parse_args()

    app = QApplication(sys.argv)
    app.setApplicationName("Screen Ruler")

    # HIDPI: query device pixel ratio from the primary screen
    primary_screen = app.primaryScreen()
    dpr = primary_screen.devicePixelRatio()

    print("Capturing screen…")
    qimage = capture_screen(app)

    print(
        f"Computing edge map  "
        f"(Canny thresholds: {args.threshold_low} / {args.threshold_high})…"
    )
    edge_map = compute_edge_map(
        qimage, args.threshold_low, args.threshold_high
    )
    print(
        f"Edge map: {edge_map.shape[1]}×{edge_map.shape[0]} px  |  "
        f"{edge_map.sum():,} edge pixels found."
    )

    overlay = ScreenRulerOverlay(edge_map, dpr)
    overlay.show()
    overlay.activateWindow()

    sys.exit(app.exec_())


if __name__ == "__main__":
    main()
