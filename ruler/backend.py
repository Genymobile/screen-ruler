"""QML backend QObject for screen-ruler."""

from __future__ import annotations

import shutil
import subprocess

import cv2
import numpy as np

from PyQt6.QtCore import QObject, QTimer, pyqtProperty, pyqtSignal, pyqtSlot
from PyQt6.QtGui import QClipboard, QCursor, QGuiApplication, QImage

from .core import (
    REGION_CLOSE_KERNEL_SIZE,
    REGION_DILATE_ITERATIONS,
    compute_edge_map,
    sensitivity_to_thresholds,
    thresholds_to_sensitivity,
    trace_ray,
)
from .overlay import create_debug_edge_overlay_source
from .platform import is_wayland_session

SENSITIVITY_RECOMPUTE_DEBOUNCE_MS = 30


class RulerBackend(QObject):
    """Expose cursor position and measurements to QML."""

    dataChanged = pyqtSignal()
    staticChanged = pyqtSignal()
    controlsChanged = pyqtSignal()
    edgeMapPreviewRequested = pyqtSignal()
    annotationsChanged = pyqtSignal()

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
        self._sensitivity = thresholds_to_sensitivity(threshold_low, threshold_high)
        self._always_show_debug_overlay = always_show_debug_overlay
        self._screenshot_source = screenshot_source
        self._debug_overlay_source = debug_overlay_source
        self._overlay_version = 0
        self._is_wayland = is_wayland_session()
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
        self._annotations: list[dict] = []
        self._redo_stack: list[dict] = []

        self._recompute_timer = QTimer(self)
        self._recompute_timer.setSingleShot(True)
        self._recompute_timer.timeout.connect(self._recompute_edge_map)
        self._recompute_regions()

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

    def poll(self) -> None:
        self._update_measurement(force=False)

    def _update_measurement(self, force: bool) -> None:
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
        self._threshold_low, self._threshold_high = sensitivity_to_thresholds(clamped)
        self.controlsChanged.emit()
        self._recompute_timer.start(SENSITIVITY_RECOMPUTE_DEBOUNCE_MS)

    @pyqtSlot(float, float, float, result="QVariant")
    def snapPointToNearestEdge(
        self,
        local_x: float,
        local_y: float,
        max_distance_px: float = 5.0,
    ) -> dict[str, float | bool]:
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

        band_y = max(1, int(np.ceil(self._dpr_y)))
        band_x = max(1, int(np.ceil(self._dpr_x)))
        row_min = max(0, ey - band_y)
        row_max = min(map_h - 1, ey + band_y)
        col_min = max(0, ex - band_x)
        col_max = min(map_w - 1, ex + band_x)

        best_x_delta = None
        best_x_map = None
        for mx in range(min_x, max_x + 1):
            if not np.any(self._edge_map[row_min : row_max + 1, mx]):
                continue
            delta = abs(mx - ex)
            if best_x_delta is None or delta < best_x_delta:
                best_x_delta = delta
                best_x_map = mx

        best_y_delta = None
        best_y_map = None
        for my in range(min_y, max_y + 1):
            if not np.any(self._edge_map[my, col_min : col_max + 1]):
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
        edge_u8 = self._edge_map.astype(np.uint8)
        kernel = np.ones((REGION_CLOSE_KERNEL_SIZE, REGION_CLOSE_KERNEL_SIZE), dtype=np.uint8)
        closed = cv2.morphologyEx(edge_u8, cv2.MORPH_CLOSE, kernel)
        barriers = cv2.dilate(closed, kernel, iterations=REGION_DILATE_ITERATIONS)
        free_space = (barriers == 0).astype(np.uint8)
        _, labels, stats, _ = cv2.connectedComponentsWithStats(free_space, connectivity=4)
        self._region_labels = labels
        self._region_stats = stats

    def _region_label_near(self, ex: int, ey: int, max_radius: int = 3) -> int:
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
            window = self._region_labels[min_y : max_y + 1, min_x : max_x + 1]
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
            source = create_debug_edge_overlay_source(self._edge_map)
            if source:
                self._overlay_version += 1
                self._debug_overlay_source = f"{source}?v={self._overlay_version}"
                self.controlsChanged.emit()
            self._update_measurement(force=True)
            self.edgeMapPreviewRequested.emit()
        except Exception:
            pass

    # ------------------------------------------------------------------
    # Annotation model
    # ------------------------------------------------------------------

    @pyqtProperty("QVariantList", notify=annotationsChanged)
    def annotations(self) -> list[dict]:
        return list(self._annotations)

    @pyqtProperty(int, notify=annotationsChanged)
    def annotationCount(self) -> int:
        return len(self._annotations)

    @pyqtSlot("QVariantMap")
    def addAnnotation(self, data: dict) -> None:
        self._annotations.append(dict(data))
        self._redo_stack.clear()
        self.annotationsChanged.emit()

    @pyqtSlot()
    def removeLastAnnotation(self) -> None:
        if self._annotations:
            self._redo_stack.append(self._annotations.pop())
            self.annotationsChanged.emit()

    @pyqtSlot()
    def redoAnnotation(self) -> None:
        if self._redo_stack:
            self._annotations.append(self._redo_stack.pop())
            self.annotationsChanged.emit()

    @pyqtSlot()
    def clearAnnotations(self) -> None:
        changed = bool(self._annotations) or bool(self._redo_stack)
        self._annotations.clear()
        self._redo_stack.clear()
        if changed:
            self.annotationsChanged.emit()

    @pyqtSlot(str)
    def copyTextToClipboardAndQuit(self, text: str) -> None:
        text = str(text)

        clipboard = QGuiApplication.clipboard()
        if clipboard is not None:
            clipboard.setText(text, QClipboard.Mode.Clipboard)
            if clipboard.supportsSelection():
                clipboard.setText(text, QClipboard.Mode.Selection)

        if self._is_wayland and shutil.which("wl-copy"):
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
