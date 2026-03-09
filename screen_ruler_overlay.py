"""Overlay image serialization helpers for QML sources."""

from __future__ import annotations

import atexit
import os
import tempfile

import numpy as np
from PyQt6.QtCore import QUrl
from PyQt6.QtGui import QImage


def edge_map_to_qimage(edge_map: np.ndarray) -> QImage:
    """Convert a boolean edge map to a grayscale QImage."""
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


def create_temp_image_source(image: QImage, prefix: str) -> str | None:
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


def create_debug_edge_overlay_source(edge_map: np.ndarray) -> str | None:
    """Persist an edge map PNG and return a file URL."""
    image = edge_map_to_qimage(edge_map)
    return create_temp_image_source(image, "screen-ruler-edge-")


def create_screenshot_overlay_source(qimage: QImage) -> str | None:
    """Persist a screenshot PNG and return a file URL."""
    return create_temp_image_source(qimage, "screen-ruler-shot-")
