"""Core pure-logic helpers for screen-ruler."""

from __future__ import annotations

import numpy as np
import cv2

from PyQt6.QtGui import QImage

SENSITIVITY_LOW_AT_100 = 5
SENSITIVITY_HIGH_AT_100 = 25
SENSITIVITY_LOW_AT_0 = 80
SENSITIVITY_HIGH_AT_0 = 220

REGION_CLOSE_KERNEL_SIZE = 3
REGION_DILATE_ITERATIONS = 0


def trace_ray(edge_map: np.ndarray, px: int, py: int, dx: int, dy: int) -> int:
    """Trace one ray until the first edge pixel or map boundary."""
    if dx == 0 and dy == 0:
        raise ValueError("trace_ray requires a non-zero direction vector.")

    map_h, map_w = edge_map.shape
    cx, cy = px, py
    dist = 0
    while True:
        nx, ny = cx + dx, cy + dy
        if nx < 0 or nx >= map_w or ny < 0 or ny >= map_h:
            break
        dist += 1
        if edge_map[ny, nx]:
            break
        cx, cy = nx, ny
    return dist


def compute_edge_map(
    qimage: QImage,
    threshold_low: int = 50,
    threshold_high: int = 150,
) -> np.ndarray:
    """Convert a QImage to a boolean Canny edge map."""
    if qimage is None or qimage.isNull() or qimage.width() <= 0 or qimage.height() <= 0:
        raise ValueError("Cannot compute edge map from an empty screenshot.")

    qimage = qimage.convertToFormat(QImage.Format.Format_RGB888)
    width = qimage.width()
    height = qimage.height()
    bytes_per_line = qimage.bytesPerLine()
    ptr = qimage.bits()
    if ptr is None:
        raise ValueError("Cannot access screenshot pixel data.")
    if hasattr(ptr, "setsize"):
        ptr.setsize(bytes_per_line * height)

    raw = np.frombuffer(ptr, dtype=np.uint8, count=bytes_per_line * height).reshape(
        (height, bytes_per_line)
    )
    img = raw[:, : width * 3].reshape((height, width, 3)).copy()

    gray = cv2.cvtColor(img, cv2.COLOR_RGB2GRAY)
    edges = cv2.Canny(gray, threshold_low, threshold_high)
    return edges.astype(bool)


def sensitivity_to_thresholds(sensitivity: float) -> tuple[int, int]:
    """Map user-facing sensitivity (0..100) to Canny thresholds."""
    s = max(0.0, min(100.0, sensitivity))
    low_span = SENSITIVITY_LOW_AT_0 - SENSITIVITY_LOW_AT_100
    high_span = SENSITIVITY_HIGH_AT_0 - SENSITIVITY_HIGH_AT_100
    low = int(round(SENSITIVITY_LOW_AT_100 + (100 - s) * (low_span / 100.0)))
    high = int(round(SENSITIVITY_HIGH_AT_100 + (100 - s) * (high_span / 100.0)))
    if high <= low:
        high = min(255, low + 1)
    return low, high


def thresholds_to_sensitivity(threshold_low: int, threshold_high: int) -> float:
    """Approximate sensitivity (0..100) from Canny thresholds."""
    low_scale = (SENSITIVITY_LOW_AT_0 - SENSITIVITY_LOW_AT_100) / 100.0
    high_scale = (SENSITIVITY_HIGH_AT_0 - SENSITIVITY_HIGH_AT_100) / 100.0
    s_from_low = 100.0 - ((threshold_low - SENSITIVITY_LOW_AT_100) / low_scale)
    s_from_high = 100.0 - ((threshold_high - SENSITIVITY_HIGH_AT_100) / high_scale)
    return max(0.0, min(100.0, (s_from_low + s_from_high) / 2.0))
