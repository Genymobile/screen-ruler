"""QML backend QObject for screen-ruler."""

from __future__ import annotations

import math
import shutil
import subprocess

import cv2
import numpy as np

from PyQt6.QtCore import QBuffer, QByteArray, QIODevice, QObject, QPointF, QRect, QRectF, Qt, QTimer, pyqtProperty, pyqtSignal, pyqtSlot
from PyQt6.QtGui import QColor, QClipboard, QCursor, QFont, QFontMetrics, QGuiApplication, QImage, QPainter, QPen

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

    @staticmethod
    def _format_number(value: object) -> str:
        try:
            num = float(value)
        except (TypeError, ValueError):
            return "?"
        if num.is_integer():
            return str(int(num))
        return f"{num:.2f}".rstrip("0").rstrip(".")

    @staticmethod
    def _annotation_mode_title(mode: object) -> str:
        labels = {
            0: "Crosshair",
            1: "Rectangle",
            2: "Rectangle",
            3: "Rectangle",
            4: "Color",
        }
        try:
            mode_key = int(float(mode))
        except (TypeError, ValueError):
            return str(mode)
        return labels.get(mode_key, str(mode))

    def _shrink_rect_to_content_map(
        self,
        left: int,
        top: int,
        right: int,
        bottom: int,
        min_size: int = 5,
    ) -> tuple[int, int, int, int]:
        map_h, map_w = self._edge_map.shape
        clamped_left = max(0, min(left, map_w - 1))
        clamped_top = max(0, min(top, map_h - 1))
        clamped_right = max(0, min(right, map_w - 1))
        clamped_bottom = max(0, min(bottom, map_h - 1))

        if clamped_right < clamped_left:
            clamped_left, clamped_right = clamped_right, clamped_left
        if clamped_bottom < clamped_top:
            clamped_top, clamped_bottom = clamped_bottom, clamped_top

        width = clamped_right - clamped_left
        height = clamped_bottom - clamped_top
        if width < min_size or height < min_size:
            return clamped_left, clamped_top, clamped_right, clamped_bottom

        new_top = clamped_top
        for row in range(clamped_top, clamped_bottom + 1):
            if np.any(self._edge_map[row, clamped_left : clamped_right + 1]):
                new_top = row
                break

        new_bottom = clamped_bottom
        for row in range(clamped_bottom, new_top - 1, -1):
            if np.any(self._edge_map[row, clamped_left : clamped_right + 1]):
                new_bottom = row
                break

        new_left = clamped_left
        for col in range(clamped_left, clamped_right + 1):
            if np.any(self._edge_map[new_top : new_bottom + 1, col]):
                new_left = col
                break

        new_right = clamped_right
        for col in range(clamped_right, new_left - 1, -1):
            if np.any(self._edge_map[new_top : new_bottom + 1, col]):
                new_right = col
                break

        return new_left, new_top, new_right, new_bottom

    @staticmethod
    def _format_coordinate(value: object) -> str:
        try:
            return str(int(round(float(value))))
        except (TypeError, ValueError):
            return "?"

    @pyqtSlot(result=str)
    def annotationsToMarkdown(self) -> str:
        if not self._annotations:
            return ""

        lines: list[str] = []
        for annotation in self._annotations:
            mode = self._annotation_mode_title(annotation.get("mode"))
            measurement = str(annotation.get("text", "")).strip()
            if not measurement:
                width = self._format_number(annotation.get("width"))
                height = self._format_number(annotation.get("height"))
                measurement = f"{width} × {height} px"
            x = self._format_coordinate(annotation.get("cursorX", annotation.get("x")))
            y = self._format_coordinate(annotation.get("cursorY", annotation.get("y")))
            lines.append(f"- {mode} @ ({x}, {y}): {measurement}")
        return "\n".join(lines)

    def _sample_weighted_color(
        self,
        center_x: int,
        center_y: int,
        radius_px: int,
        map_w: int,
        map_h: int,
    ) -> tuple[int, int, int]:
        if radius_px <= 0:
            color = self._source_image.pixelColor(center_x, center_y)
            return int(color.red()), int(color.green()), int(color.blue())

        min_x = max(0, center_x - radius_px)
        max_x = min(map_w - 1, center_x + radius_px)
        min_y = max(0, center_y - radius_px)
        max_y = min(map_h - 1, center_y + radius_px)
        radius_sq = float(radius_px * radius_px)
        sigma = max(0.5, radius_px / 2.0)
        inv_two_sigma_sq = 1.0 / (2.0 * sigma * sigma)

        sum_w = 0.0
        sum_r = 0.0
        sum_g = 0.0
        sum_b = 0.0
        for py in range(min_y, max_y + 1):
            dy = float(py - center_y)
            for px in range(min_x, max_x + 1):
                dx = float(px - center_x)
                dist_sq = dx * dx + dy * dy
                if dist_sq > radius_sq:
                    continue
                weight = math.exp(-dist_sq * inv_two_sigma_sq)
                color = self._source_image.pixelColor(px, py)
                sum_w += weight
                sum_r += weight * float(color.red())
                sum_g += weight * float(color.green())
                sum_b += weight * float(color.blue())

        if sum_w <= 0.0:
            color = self._source_image.pixelColor(center_x, center_y)
            return int(color.red()), int(color.green()), int(color.blue())

        return (
            int(round(sum_r / sum_w)),
            int(round(sum_g / sum_w)),
            int(round(sum_b / sum_w)),
        )

    @pyqtSlot(float, float, float, result="QVariant")
    def sampleColorAtPoint(
        self, local_x: float, local_y: float, local_radius: float = 0.0
    ) -> dict[str, str | float | bool]:
        if self._source_image.isNull():
            return {
                "available": False,
                "x": float(local_x),
                "y": float(local_y),
                "hex": "",
                "rgb": "",
                "hsl": "",
                "r": 0,
                "g": 0,
                "b": 0,
                "h": 0,
                "s": 0,
                "l": 0,
                "sampleRadius": 0.0,
            }

        map_h, map_w = self._edge_map.shape
        if map_w <= 0 or map_h <= 0:
            return {
                "available": False,
                "x": float(local_x),
                "y": float(local_y),
                "hex": "",
                "rgb": "",
                "hsl": "",
                "r": 0,
                "g": 0,
                "b": 0,
                "h": 0,
                "s": 0,
                "l": 0,
                "sampleRadius": 0.0,
            }

        ex = max(0, min(int(round(float(local_x) * self._dpr_x)), map_w - 1))
        ey = max(0, min(int(round(float(local_y) * self._dpr_y)), map_h - 1))
        avg_dpr = max(1e-6, (self._dpr_x + self._dpr_y) / 2.0)
        radius_map = max(0, int(round(float(local_radius) * avg_dpr)))
        sampled_radius_local = float(radius_map) / avg_dpr
        r, g, b = self._sample_weighted_color(ex, ey, radius_map, map_w, map_h)
        color = QColor(r, g, b)
        hex_value = f"#{r:02X}{g:02X}{b:02X}"
        rgb_value = f"rgb({r}, {g}, {b})"
        hue = int(color.hslHue())
        if hue < 0:
            hue = 0
        saturation = int(round(color.hslSaturation() * 100 / 255))
        lightness = int(round(color.lightness() * 100 / 255))
        hsl_value = f"hsl({hue}, {saturation}%, {lightness}%)"
        return {
            "available": True,
            "x": float(ex) / self._dpr_x,
            "y": float(ey) / self._dpr_y,
            "hex": hex_value,
            "rgb": rgb_value,
            "hsl": hsl_value,
            "r": r,
            "g": g,
            "b": b,
            "h": hue,
            "s": saturation,
            "l": lightness,
            "sampleRadius": sampled_radius_local,
        }

    @pyqtSlot(float, float, float, float, result="QVariant")
    def shrinkRectToContent(
        self,
        local_x: float,
        local_y: float,
        local_width: float,
        local_height: float,
    ) -> dict[str, float | bool]:
        map_h, map_w = self._edge_map.shape
        x0 = int(round(float(local_x) * self._dpr_x))
        y0 = int(round(float(local_y) * self._dpr_y))
        x1 = int(round((float(local_x) + float(local_width)) * self._dpr_x))
        y1 = int(round((float(local_y) + float(local_height)) * self._dpr_y))

        left = min(x0, x1)
        right = max(x0, x1)
        top = min(y0, y1)
        bottom = max(y0, y1)

        if map_w <= 0 or map_h <= 0:
            return {
                "available": False,
                "x": 0.0,
                "y": 0.0,
                "width": 0.0,
                "height": 0.0,
            }

        new_left, new_top, new_right, new_bottom = self._shrink_rect_to_content_map(
            left, top, right, bottom
        )
        return {
            "available": True,
            "x": float(new_left) / self._dpr_x,
            "y": float(new_top) / self._dpr_y,
            "width": float(new_right - new_left) / self._dpr_x,
            "height": float(new_bottom - new_top) / self._dpr_y,
        }

    def _copy_text_to_clipboard(self, text: str) -> None:
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

    def _copy_image_to_clipboard(self, image: QImage) -> None:
        if image.isNull():
            return

        clipboard = QGuiApplication.clipboard()
        if clipboard is not None:
            clipboard.setImage(image, QClipboard.Mode.Clipboard)
            if clipboard.supportsSelection():
                clipboard.setImage(image, QClipboard.Mode.Selection)

        if self._is_wayland and shutil.which("wl-copy"):
            try:
                payload = QByteArray()
                buffer = QBuffer(payload)
                buffer.open(QIODevice.OpenModeFlag.WriteOnly)
                image.save(buffer, "PNG")
                subprocess.run(
                    ["wl-copy", "--type", "image/png"],
                    input=bytes(payload),
                    check=True,
                    timeout=5,
                )
            except Exception:
                pass

    @staticmethod
    def _to_float(value: object, default: float = 0.0) -> float:
        try:
            return float(value)
        except (TypeError, ValueError):
            return default

    def _local_to_image_x(self, value: object) -> float:
        return self._to_float(value) * self._dpr_x

    def _local_to_image_y(self, value: object) -> float:
        return self._to_float(value) * self._dpr_y

    @staticmethod
    def _resolve_floating_panel_position(
        anchor_x: float,
        anchor_y: float,
        box_width: float,
        box_height: float,
        offset_x: float,
        offset_y: float,
        canvas_width: float,
        canvas_height: float,
        margin: float = 2.0,
    ) -> tuple[float, float]:
        desired_x = anchor_x + offset_x
        desired_y = anchor_y + offset_y

        if desired_x + box_width <= canvas_width - margin:
            resolved_x = desired_x
        else:
            resolved_x = max(margin, anchor_x - offset_x - box_width)

        if desired_y + box_height <= canvas_height - margin:
            resolved_y = desired_y
        else:
            resolved_y = max(margin, anchor_y - offset_y - box_height)

        return resolved_x, resolved_y

    def _draw_annotation_measurement_label(
        self,
        painter: QPainter,
        annotation: dict,
        crop_left: int,
        crop_top: int,
    ) -> None:
        label_base_margin = 14
        label_offset_y = 4
        label_shadow_offset = 2
        label_horizontal_padding = 18
        label_vertical_padding = 12
        corner_radius = 5
        panel_background = QColor("#1A1A1A")
        panel_shadow = QColor(0, 0, 0, int(0.22 * 255))
        text_color = QColor("#FFFFFF")
        panel_background.setAlphaF(0.9)

        mode = int(self._to_float(annotation.get("mode"), 0))
        if mode == 0:
            anchor_x = self._local_to_image_x(annotation.get("cursorX"))
            anchor_y = self._local_to_image_y(annotation.get("cursorY"))
        else:
            anchor_x = self._local_to_image_x(annotation.get("x"))
            anchor_y = self._local_to_image_y(annotation.get("y"))
        text_value = str(annotation.get("text", "")).strip()
        if not text_value:
            text_value = (
                f"{self._format_number(annotation.get('width'))} × "
                f"{self._format_number(annotation.get('height'))} px"
            )

        font = QFont("DejaVu Sans Mono")
        font.setBold(True)
        font_scale = max(1.0, (self._dpr_x + self._dpr_y) / 2.0)
        font.setPixelSize(max(1, int(round(13 * font_scale))))
        painter.setFont(font)
        metrics = QFontMetrics(font)
        text_width = metrics.horizontalAdvance(text_value)
        text_height = metrics.height()
        box_width = text_width + int(label_horizontal_padding * self._dpr_x)
        box_height = text_height + int(label_vertical_padding * self._dpr_y)
        label_abs_x, label_abs_y = self._resolve_floating_panel_position(
            anchor_x=anchor_x,
            anchor_y=anchor_y,
            box_width=box_width,
            box_height=box_height,
            offset_x=label_base_margin * self._dpr_x,
            offset_y=label_offset_y * self._dpr_y,
            canvas_width=self._source_image.width(),
            canvas_height=self._source_image.height(),
        )
        label_x = label_abs_x - crop_left
        label_y = label_abs_y - crop_top
        shadow_x = label_x + label_shadow_offset * self._dpr_x
        shadow_y = label_y + label_shadow_offset * self._dpr_y

        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(panel_shadow)
        painter.drawRoundedRect(
            QRectF(shadow_x, shadow_y, box_width, box_height),
            corner_radius,
            corner_radius,
        )

        painter.setBrush(panel_background)
        painter.drawRoundedRect(
            QRectF(label_x, label_y, box_width, box_height),
            corner_radius,
            corner_radius,
        )

        painter.setPen(text_color)
        text_x = label_x + (box_width - text_width) / 2
        text_baseline = label_y + (box_height + metrics.ascent() - metrics.descent()) / 2
        painter.drawText(QPointF(text_x, text_baseline), text_value)

    def _draw_annotation_color_bubble(
        self,
        painter: QPainter,
        annotation: dict,
        crop_left: int,
        crop_top: int,
    ) -> None:
        panel_background = QColor("#1A1A1A")
        panel_background.setAlphaF(0.9)
        panel_shadow = QColor(0, 0, 0, int(0.22 * 255))
        text_color = QColor("#FFFFFF")

        scale = max(1.0, (self._dpr_x + self._dpr_y) / 2.0)
        offset_x = (14 + 8) * self._dpr_x
        offset_y = (4 + 6) * self._dpr_y
        shadow_offset = 2 * scale
        horizontal_padding = 18 * scale
        vertical_padding = 12 * scale
        row_spacing = 10 * scale
        line_spacing = 1 * scale
        swatch_size = 16 * scale
        corner_radius = 5 * scale

        anchor_x = self._local_to_image_x(annotation.get("x"))
        anchor_y = self._local_to_image_y(annotation.get("y"))
        color_hex = str(annotation.get("colorHex", "#000000")).strip() or "#000000"
        color_rgb = str(annotation.get("colorRgb", "rgb(0, 0, 0)")).strip() or "rgb(0, 0, 0)"
        color_hsl = str(annotation.get("colorHsl", "hsl(0, 0%, 0%)")).strip() or "hsl(0, 0%, 0%)"
        lines = [color_hex, color_rgb, color_hsl]

        font = QFont("DejaVu Sans Mono")
        font.setBold(True)
        font.setPixelSize(max(1, int(round(13 * scale))))
        painter.setFont(font)
        metrics = QFontMetrics(font)

        text_col_width = max(metrics.horizontalAdvance(line) for line in lines)
        line_height = metrics.height()
        text_col_height = line_height * len(lines) + line_spacing * (len(lines) - 1)
        row_height = max(swatch_size, text_col_height)
        row_width = swatch_size + row_spacing + text_col_width
        box_width = row_width + horizontal_padding
        box_height = row_height + vertical_padding

        bubble_abs_x, bubble_abs_y = self._resolve_floating_panel_position(
            anchor_x=anchor_x,
            anchor_y=anchor_y,
            box_width=box_width,
            box_height=box_height,
            offset_x=offset_x,
            offset_y=offset_y,
            canvas_width=self._source_image.width(),
            canvas_height=self._source_image.height(),
        )
        bubble_x = bubble_abs_x - crop_left
        bubble_y = bubble_abs_y - crop_top

        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(panel_shadow)
        painter.drawRoundedRect(
            QRectF(
                bubble_x + shadow_offset,
                bubble_y + shadow_offset,
                box_width,
                box_height,
            ),
            corner_radius,
            corner_radius,
        )

        painter.setBrush(panel_background)
        painter.drawRoundedRect(
            QRectF(bubble_x, bubble_y, box_width, box_height),
            corner_radius,
            corner_radius,
        )

        row_left = bubble_x + (box_width - row_width) / 2
        row_top = bubble_y + (box_height - row_height) / 2

        swatch_color = QColor(color_hex)
        if not swatch_color.isValid():
            swatch_color = QColor("#000000")
        painter.fillRect(QRectF(row_left, row_top, swatch_size, swatch_size), swatch_color)
        painter.setPen(QPen(text_color))
        painter.setBrush(Qt.BrushStyle.NoBrush)
        painter.drawRect(QRectF(row_left, row_top, swatch_size, swatch_size))

        painter.setPen(text_color)
        text_x = row_left + swatch_size + row_spacing
        text_start_y = row_top + (row_height - text_col_height) / 2
        for index, line in enumerate(lines):
            baseline_y = (
                text_start_y
                + index * (line_height + line_spacing)
                + metrics.ascent()
            )
            painter.drawText(QPointF(text_x, baseline_y), line)

    def _draw_annotation_overlay(
        self,
        painter: QPainter,
        annotation: dict,
        crop_left: int,
        crop_top: int,
    ) -> None:
        accent = QColor("#E6195E")
        pen = QPen(accent)
        pen.setWidth(1)
        painter.setPen(pen)
        painter.setBrush(Qt.BrushStyle.NoBrush)

        mode = int(self._to_float(annotation.get("mode"), 0))
        if mode == 0:
            cx = self._local_to_image_x(annotation.get("cursorX")) - crop_left
            cy = self._local_to_image_y(annotation.get("cursorY")) - crop_top
            north = self._local_to_image_y(annotation.get("northEnd")) - crop_top
            south = self._local_to_image_y(annotation.get("southEnd")) - crop_top
            west = self._local_to_image_x(annotation.get("westEnd")) - crop_left
            east = self._local_to_image_x(annotation.get("eastEnd")) - crop_left
            tick = 5 * ((self._dpr_x + self._dpr_y) / 2.0)

            painter.drawLine(QPointF(cx, north), QPointF(cx, south))
            painter.drawLine(QPointF(west, cy), QPointF(east, cy))
            painter.drawLine(QPointF(cx - tick, north), QPointF(cx + tick, north))
            painter.drawLine(QPointF(cx - tick, south), QPointF(cx + tick, south))
            painter.drawLine(QPointF(west, cy - tick), QPointF(west, cy + tick))
            painter.drawLine(QPointF(east, cy - tick), QPointF(east, cy + tick))
        elif mode == 4:
            x = self._local_to_image_x(annotation.get("x")) - crop_left
            y = self._local_to_image_y(annotation.get("y")) - crop_top
            avg_dpr = max(1e-6, (self._dpr_x + self._dpr_y) / 2.0)
            radius = max(0.8, self._to_float(annotation.get("sampleRadius", 0.0)) * avg_dpr)
            marker_arm = max(3.0, 5.0 * ((self._dpr_x + self._dpr_y) / 2.0))
            marker_gap = max(1.0, 2.0 * ((self._dpr_x + self._dpr_y) / 2.0))
            painter.drawEllipse(QPointF(x, y), radius, radius)
            painter.drawLine(QPointF(x - marker_arm, y), QPointF(x - marker_gap, y))
            painter.drawLine(QPointF(x + marker_gap, y), QPointF(x + marker_arm, y))
            painter.drawLine(QPointF(x, y - marker_arm), QPointF(x, y - marker_gap))
            painter.drawLine(QPointF(x, y + marker_gap), QPointF(x, y + marker_arm))
            painter.fillRect(QRectF(x - 1.0, y - 1.0, 3.0, 3.0), accent)
            self._draw_annotation_color_bubble(painter, annotation, crop_left, crop_top)
        else:
            x = self._local_to_image_x(annotation.get("x")) - crop_left
            y = self._local_to_image_y(annotation.get("y")) - crop_top
            w = max(1.0, self._local_to_image_x(annotation.get("width", 0)))
            h = max(1.0, self._local_to_image_y(annotation.get("height", 0)))
            painter.drawRect(QRectF(x, y, w, h))

        if mode != 4:
            self._draw_annotation_measurement_label(painter, annotation, crop_left, crop_top)

    def buildCompositeImageForRegion(
        self,
        local_x: float,
        local_y: float,
        local_width: float,
        local_height: float,
    ) -> QImage:
        if self._source_image.isNull():
            return QImage()

        crop_left = int(round(local_x * self._dpr_x))
        crop_top = int(round(local_y * self._dpr_y))
        crop_width = int(round(local_width * self._dpr_x))
        crop_height = int(round(local_height * self._dpr_y))
        if crop_width <= 0 or crop_height <= 0:
            return QImage()

        bounds = QRect(0, 0, self._source_image.width(), self._source_image.height())
        crop = QRect(crop_left, crop_top, crop_width, crop_height).intersected(bounds)
        if crop.isEmpty():
            return QImage()

        output = self._source_image.copy(crop)
        if output.isNull():
            return QImage()

        painter = QPainter(output)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        for annotation in self._annotations:
            self._draw_annotation_overlay(
                painter,
                annotation,
                crop.left(),
                crop.top(),
            )
        painter.end()
        return output

    @pyqtSlot(float, float, float, float, result=bool)
    def copyCompositeRegionToClipboard(
        self,
        local_x: float,
        local_y: float,
        local_width: float,
        local_height: float,
    ) -> bool:
        composite = self.buildCompositeImageForRegion(
            local_x, local_y, local_width, local_height
        )
        if composite.isNull():
            return False
        self._copy_image_to_clipboard(composite)
        return True

    @pyqtSlot()
    def copyAnnotationsMarkdownToClipboard(self) -> None:
        self._copy_text_to_clipboard(self.annotationsToMarkdown())

    @pyqtSlot(str)
    def copyTextToClipboardAndQuit(self, text: str) -> None:
        self._copy_text_to_clipboard(text)
        QTimer.singleShot(120, QGuiApplication.quit)
