"""
Unit tests for the pure-logic components of screen_ruler.py.

These tests do NOT require a display or a running QApplication — they only
exercise the module-level helper functions ``trace_ray`` and
``compute_edge_map`` which have no Qt dependencies at runtime.
"""

import numpy as np
import pytest

# Import only the pure-logic symbols; avoid triggering any Qt initialisation.
from screen_ruler import trace_ray, compute_edge_map, _capture_screen_external
from screen_ruler import _edge_map_to_qimage
from screen_ruler import RulerBackend
from ruler.platform import is_wayland_session, session_type


# ---------------------------------------------------------------------------
# trace_ray
# ---------------------------------------------------------------------------


class TestTraceRay:
    """Tests for the ray-tracing helper."""

    def _empty(self, h: int = 20, w: int = 20) -> np.ndarray:
        """Return an all-False edge map of the given size."""
        return np.zeros((h, w), dtype=bool)

    def _with_edge_at(self, row: int, col: int, h: int = 20, w: int = 20) -> np.ndarray:
        em = self._empty(h, w)
        em[row, col] = True
        return em

    # -- boundary behaviour --------------------------------------------------

    def test_empty_map_east_reaches_boundary(self):
        """With no edges the ray must travel to the map boundary."""
        em = self._empty(10, 10)
        # Starting at column 0, heading east (+x) → 9 steps to column 9
        assert trace_ray(em, 0, 5, 1, 0) == 9

    def test_empty_map_west_reaches_boundary(self):
        em = self._empty(10, 10)
        assert trace_ray(em, 9, 5, -1, 0) == 9

    def test_empty_map_north_reaches_boundary(self):
        em = self._empty(10, 10)
        assert trace_ray(em, 5, 9, 0, -1) == 9

    def test_empty_map_south_reaches_boundary(self):
        em = self._empty(10, 10)
        assert trace_ray(em, 5, 0, 0, 1) == 9

    def test_start_on_boundary_returns_zero(self):
        """A ray fired from the map boundary should immediately return 0."""
        em = self._empty(10, 10)
        assert trace_ray(em, 9, 5, 1, 0) == 0   # eastern edge, heading east

    # -- edge detection ------------------------------------------------------

    def test_ray_stops_at_edge_pixel(self):
        """Ray must stop (and include) the first edge pixel it encounters."""
        em = self._with_edge_at(row=5, col=7)
        # Start at (0, 5) heading east — edge is at col 7, distance = 7
        assert trace_ray(em, 0, 5, 1, 0) == 7

    def test_ray_stops_at_nearest_edge(self):
        """When multiple edges exist the ray must stop at the closest one."""
        em = self._empty()
        em[5, 4] = True
        em[5, 10] = True
        # From (0, 5) heading east, nearest edge is at col 4
        assert trace_ray(em, 0, 5, 1, 0) == 4

    def test_ray_one_step_from_edge(self):
        em = self._with_edge_at(row=5, col=1)
        assert trace_ray(em, 0, 5, 1, 0) == 1

    def test_edge_at_starting_column_not_detected(self):
        """
        The algorithm starts by examining the *next* pixel, not the starting
        pixel itself.  An edge at the start should be ignored and the ray
        should continue until it finds another edge or hits the boundary.
        """
        em = self._empty(10, 10)
        em[5, 0] = True   # edge at start column
        # No other edges east → ray travels to the boundary (9 steps)
        assert trace_ray(em, 0, 5, 1, 0) == 9

    # -- diagonal / combined directions (sanity) -----------------------------

    def test_non_stationary_cardinal_rays_return_non_negative_distance(self):
        """Verify valid non-stationary cardinal directions reach a boundary
        and produce a non-negative distance."""
        em = self._empty(5, 5)
        for dx, dy in [(1, 0), (-1, 0), (0, 1), (0, -1)]:
            result = trace_ray(em, 2, 2, dx, dy)
            assert result >= 0

    def test_stationary_direction_raises_value_error(self):
        """Direction (0, 0) is invalid and must raise instead of hanging."""
        em = self._empty(5, 5)
        with pytest.raises(ValueError, match="non-zero direction"):
            trace_ray(em, 2, 2, 0, 0)

    # -- measurement correctness ---------------------------------------------

    def test_W_equals_sum_of_east_west_rays(self):
        """W should equal d_west + d_east measured from the same starting point."""
        em = self._empty(10, 20)
        em[5, 3] = True   # west boundary: 6 columns away from col 9
        em[5, 14] = True  # east boundary: 5 columns away from col 9
        d_w = trace_ray(em, 9, 5, -1, 0)
        d_e = trace_ray(em, 9, 5, 1, 0)
        assert d_w == 6
        assert d_e == 5
        assert d_w + d_e == 11

    def test_H_equals_sum_of_north_south_rays(self):
        em = self._empty(20, 10)
        em[3, 5] = True   # north boundary: row 3, from row 9 → distance 6
        em[15, 5] = True  # south boundary: row 15, from row 9 → distance 6
        d_n = trace_ray(em, 5, 9, 0, -1)
        d_s = trace_ray(em, 5, 9, 0, 1)
        assert d_n == 6
        assert d_s == 6
        assert d_n + d_s == 12


# ---------------------------------------------------------------------------
# compute_edge_map
# ---------------------------------------------------------------------------


class TestComputeEdgeMap:
    """Tests for the Canny-based edge-map builder."""

    def _make_qimage(self, width: int, height: int, rgb: tuple) -> "QImage":
        from PyQt6.QtGui import QImage
        img = QImage(width, height, QImage.Format.Format_RGB888)
        img.fill(0)
        from PyQt6.QtGui import QColor
        color = QColor(*rgb)
        for y in range(height):
            for x in range(width):
                img.setPixelColor(x, y, color)
        return img

    def test_solid_colour_produces_no_edges(self):
        """A uniformly coloured image should yield no edges at all."""
        from PyQt6.QtGui import QImage
        img = self._make_qimage(50, 50, (128, 64, 200))
        edge_map = compute_edge_map(img)
        assert edge_map.dtype == bool
        assert edge_map.shape == (50, 50)
        assert not edge_map.any(), "Solid image must have zero edge pixels"

    def test_output_shape_matches_input(self):
        """Edge map must have the same spatial dimensions as the source image."""
        from PyQt6.QtGui import QImage
        img = self._make_qimage(80, 60, (0, 0, 0))
        edge_map = compute_edge_map(img)
        assert edge_map.shape == (60, 80)

    def test_hard_edge_is_detected(self):
        """
        A half-black / half-white image has a crisp vertical edge.
        The edge map must contain some True pixels along that boundary.
        """
        from PyQt6.QtGui import QImage, QColor
        w, h = 100, 100
        img = QImage(w, h, QImage.Format.Format_RGB888)
        img.fill(0)
        white = QColor(255, 255, 255)
        for y in range(h):
            for x in range(w):
                if x >= w // 2:
                    img.setPixelColor(x, y, white)
        edge_map = compute_edge_map(img, threshold_low=30, threshold_high=80)
        assert edge_map.any(), "Hard vertical edge must be detected"

    def test_returns_bool_array(self):
        """Return type must always be a boolean numpy array."""
        from PyQt6.QtGui import QImage
        img = self._make_qimage(10, 10, (100, 100, 100))
        edge_map = compute_edge_map(img)
        assert edge_map.dtype == bool

    def test_custom_thresholds_accepted(self):
        """Function must not raise when non-default thresholds are supplied."""
        from PyQt6.QtGui import QImage
        img = self._make_qimage(20, 20, (0, 0, 0))
        # Should not raise
        edge_map = compute_edge_map(img, threshold_low=10, threshold_high=200)
        assert edge_map.shape == (20, 20)

    def test_empty_qimage_raises_value_error(self):
        """A null/empty QImage should fail with a clear ValueError."""
        from PyQt6.QtGui import QImage

        with pytest.raises(ValueError, match="empty screenshot"):
            compute_edge_map(QImage())


class TestExternalCaptureFallback:
    """Tests for Wayland/X11-specific external screenshot fallbacks."""

    def _png_bytes(self) -> bytes:
        from PyQt6.QtCore import QBuffer, QByteArray, QIODevice
        from PyQt6.QtGui import QImage

        image = QImage(4, 3, QImage.Format.Format_RGB32)
        image.fill(0xFF112233)

        byte_array = QByteArray()
        buffer = QBuffer(byte_array)
        buffer.open(QIODevice.OpenModeFlag.WriteOnly)
        image.save(buffer, "PNG")
        return bytes(byte_array)

    def test_wayland_uses_grim_when_available(self, monkeypatch):
        import screen_ruler
        from PyQt6.QtCore import QRect

        calls = []

        monkeypatch.setenv("XDG_SESSION_TYPE", "wayland")
        monkeypatch.setattr(
            "screen_ruler.shutil.which",
            lambda name: "/usr/bin/grim" if name == "grim" else None,
        )

        png = self._png_bytes()

        class Result:
            stdout = png

        def fake_run(command, check, capture_output, timeout):
            calls.append(command)
            return Result()

        monkeypatch.setattr("screen_ruler.subprocess.run", fake_run)

        image = _capture_screen_external(QRect(10, 20, 100, 60))
        assert not image.isNull()
        assert calls
        assert calls[0][0] == "/usr/bin/grim"

    def test_x11_returns_empty_without_import_tool(self, monkeypatch):
        from PyQt6.QtCore import QRect

        monkeypatch.setenv("XDG_SESSION_TYPE", "x11")
        monkeypatch.setattr("screen_ruler.shutil.which", lambda _: None)

        image = _capture_screen_external(QRect(0, 0, 50, 50))
        assert image.isNull()


class TestPlatformSessionHelper:
    """Tests for platform/session detection helpers."""

    def test_session_type_reads_environment_when_no_qt_app(self, monkeypatch):
        monkeypatch.setenv("XDG_SESSION_TYPE", "wayland")
        assert session_type() == "wayland"

    def test_is_wayland_session_false_on_x11_environment(self, monkeypatch):
        monkeypatch.setenv("XDG_SESSION_TYPE", "x11")
        assert is_wayland_session() is False
        assert session_type() == "x11"


class TestDebugEdgeOverlay:
    """Tests for debug overlay conversion helpers."""

    def test_edge_map_to_qimage_dimensions(self):
        edge_map = np.zeros((12, 34), dtype=bool)
        edge_map[5, 10] = True

        image = _edge_map_to_qimage(edge_map)

        assert not image.isNull()
        assert image.width() == 34
        assert image.height() == 12

    def test_edge_map_to_qimage_rejects_invalid_shape(self):
        with pytest.raises(ValueError, match="non-empty 2-D"):
            _edge_map_to_qimage(np.array([], dtype=bool))

    def test_external_failure_returns_empty_image(self, monkeypatch):
        import screen_ruler
        from PyQt6.QtCore import QRect

        monkeypatch.setenv("XDG_SESSION_TYPE", "wayland")
        monkeypatch.setattr("screen_ruler.shutil.which", lambda name: "/usr/bin/grim")

        def fake_run(command, check, capture_output, timeout):
            raise screen_ruler.subprocess.CalledProcessError(1, command)

        monkeypatch.setattr("screen_ruler.subprocess.run", fake_run)

        image = _capture_screen_external(QRect(0, 0, 50, 50))
        assert image.isNull()


class TestSnapPointToNearestEdge:
    """Tests for line/corner snapping in RulerBackend."""

    def _backend(self, edge_map: np.ndarray) -> RulerBackend:
        from PyQt6.QtGui import QImage

        h, w = edge_map.shape
        source_image = QImage(1, 1, QImage.Format.Format_RGB32)
        return RulerBackend(
            edge_map=edge_map,
            dpr_x=1.0,
            dpr_y=1.0,
            virtual_x=0,
            virtual_y=0,
            virtual_w=w,
            virtual_h=h,
            source_image=source_image,
            threshold_low=50,
            threshold_high=150,
            always_show_debug_overlay=False,
        )

    def test_corner_point_snaps_both_axes(self):
        """A nearby corner pixel should snap both x and y coordinates."""
        edge_map = np.zeros((12, 12), dtype=bool)
        edge_map[4, 6] = True
        backend = self._backend(edge_map)

        result = backend.snapPointToNearestEdge(5.2, 5.2, 2.0)

        assert result["snapped"] is True
        assert result["x"] == 6.0
        assert result["y"] == 4.0

    def test_horizontal_edge_snaps_y_coordinate(self):
        """A horizontal edge near the cursor should pull y to that edge."""
        edge_map = np.zeros((12, 12), dtype=bool)
        edge_map[4, :] = True
        backend = self._backend(edge_map)

        result = backend.snapPointToNearestEdge(6.0, 5.2, 2.0)

        assert result["snapped"] is True
        assert result["x"] == 6.0
        assert result["y"] == 4.0

    def test_vertical_edge_snaps_x_coordinate(self):
        """A vertical edge near the cursor should pull x to that edge."""
        edge_map = np.zeros((12, 12), dtype=bool)
        edge_map[:, 7] = True
        backend = self._backend(edge_map)

        result = backend.snapPointToNearestEdge(5.2, 6.0, 2.0)

        assert result["snapped"] is True
        assert result["x"] == 7.0
        assert result["y"] == 6.0

    def test_no_edge_within_radius_returns_original_point(self):
        """If no edge is close enough, the point should remain unchanged."""
        edge_map = np.zeros((12, 12), dtype=bool)
        edge_map[10, 10] = True
        backend = self._backend(edge_map)

        result = backend.snapPointToNearestEdge(2.0, 2.0, 1.0)

        assert result["snapped"] is False
        assert result["x"] == 2.0
        assert result["y"] == 2.0


class TestDetectContainerAtPoint:
    """Tests for experimental container detection from the edge map."""

    def _backend(self, edge_map: np.ndarray) -> RulerBackend:
        from PyQt6.QtGui import QImage

        h, w = edge_map.shape
        source_image = QImage(1, 1, QImage.Format.Format_RGB32)
        return RulerBackend(
            edge_map=edge_map,
            dpr_x=1.0,
            dpr_y=1.0,
            virtual_x=0,
            virtual_y=0,
            virtual_w=w,
            virtual_h=h,
            source_image=source_image,
            threshold_low=50,
            threshold_high=150,
            always_show_debug_overlay=False,
        )

    def test_prefers_nearby_small_enclosed_zone(self):
        """Near an internal divider, detect the nearby small enclosed region."""
        edge_map = np.zeros((30, 30), dtype=bool)

        # Outer frame
        edge_map[2:28, 2] = True
        edge_map[2:28, 27] = True
        edge_map[2, 2:28] = True
        edge_map[27, 2:28] = True

        # Internal vertical divider creates a smaller right-side zone
        edge_map[3:27, 20] = True

        backend = self._backend(edge_map)

        # Pointer sits on the divider line area: nearest enclosed zone should
        # be the right-side smaller region.
        result = backend.detectContainerAtPoint(20.0, 15.0)

        assert result["available"] is True
        assert result["x"] >= 20.0
        assert result["width"] < 10.0


class TestAnnotationModel:
    """Tests for the session-mode persistent annotation store."""

    def _backend(self) -> RulerBackend:
        from PyQt6.QtGui import QImage

        edge_map = np.zeros((20, 20), dtype=bool)
        source_image = QImage(1, 1, QImage.Format.Format_RGB32)
        return RulerBackend(
            edge_map=edge_map,
            dpr_x=1.0,
            dpr_y=1.0,
            virtual_x=0,
            virtual_y=0,
            virtual_w=20,
            virtual_h=20,
            source_image=source_image,
            threshold_low=50,
            threshold_high=150,
            always_show_debug_overlay=False,
        )

    def _sample(self, **overrides) -> dict:
        base = {
            "mode": 0,
            "x": 10.0,
            "y": 20.0,
            "width": 100.0,
            "height": 50.0,
            "text": "100 × 50 px",
            "cursorX": 10.0,
            "cursorY": 20.0,
            "northEnd": 5.0,
            "southEnd": 70.0,
            "westEnd": 0.0,
            "eastEnd": 110.0,
        }
        base.update(overrides)
        return base

    def test_initial_annotations_empty(self):
        backend = self._backend()
        assert backend.annotations == []
        assert backend.annotationCount == 0

    def test_add_annotation_increases_count(self):
        backend = self._backend()
        backend.addAnnotation(self._sample())
        assert backend.annotationCount == 1

    def test_add_annotation_stores_data(self):
        backend = self._backend()
        data = self._sample(text="42 × 8 px", mode=1)
        backend.addAnnotation(data)
        stored = backend.annotations[0]
        assert stored["text"] == "42 × 8 px"
        assert stored["mode"] == 1

    def test_add_multiple_annotations(self):
        backend = self._backend()
        backend.addAnnotation(self._sample(text="A"))
        backend.addAnnotation(self._sample(text="B"))
        backend.addAnnotation(self._sample(text="C"))
        assert backend.annotationCount == 3
        texts = [a["text"] for a in backend.annotations]
        assert texts == ["A", "B", "C"]

    def test_remove_last_annotation_decreases_count(self):
        backend = self._backend()
        backend.addAnnotation(self._sample())
        backend.addAnnotation(self._sample())
        backend.removeLastAnnotation()
        assert backend.annotationCount == 1

    def test_remove_last_annotation_removes_correct_item(self):
        backend = self._backend()
        backend.addAnnotation(self._sample(text="first"))
        backend.addAnnotation(self._sample(text="second"))
        backend.removeLastAnnotation()
        assert backend.annotations[0]["text"] == "first"

    def test_remove_last_on_empty_is_safe(self):
        backend = self._backend()
        backend.removeLastAnnotation()  # must not raise
        assert backend.annotationCount == 0

    def test_clear_annotations_empties_store(self):
        backend = self._backend()
        backend.addAnnotation(self._sample())
        backend.addAnnotation(self._sample())
        backend.clearAnnotations()
        assert backend.annotations == []
        assert backend.annotationCount == 0

    def test_clear_annotations_on_empty_is_safe(self):
        backend = self._backend()
        backend.clearAnnotations()  # must not raise

    def test_annotations_returns_copy(self):
        """Mutating the returned list must not affect internal state."""
        backend = self._backend()
        backend.addAnnotation(self._sample())
        copy = backend.annotations
        copy.clear()
        assert backend.annotationCount == 1

    def test_annotationsChanged_emitted_on_add(self):
        backend = self._backend()
        received = []
        backend.annotationsChanged.connect(lambda: received.append(1))
        backend.addAnnotation(self._sample())
        assert len(received) == 1

    def test_annotationsChanged_emitted_on_remove(self):
        backend = self._backend()
        backend.addAnnotation(self._sample())
        received = []
        backend.annotationsChanged.connect(lambda: received.append(1))
        backend.removeLastAnnotation()
        assert len(received) == 1

    def test_annotationsChanged_not_emitted_on_remove_when_empty(self):
        backend = self._backend()
        received = []
        backend.annotationsChanged.connect(lambda: received.append(1))
        backend.removeLastAnnotation()
        assert len(received) == 0

    def test_annotationsChanged_emitted_on_clear(self):
        backend = self._backend()
        backend.addAnnotation(self._sample())
        received = []
        backend.annotationsChanged.connect(lambda: received.append(1))
        backend.clearAnnotations()
        assert len(received) == 1

    def test_annotationsChanged_not_emitted_on_clear_when_empty(self):
        backend = self._backend()
        received = []
        backend.annotationsChanged.connect(lambda: received.append(1))
        backend.clearAnnotations()
        assert len(received) == 0

    def test_redo_restores_undone_annotation(self):
        backend = self._backend()
        backend.addAnnotation(self._sample(text="A"))
        backend.removeLastAnnotation()
        backend.redoAnnotation()
        assert backend.annotationCount == 1
        assert backend.annotations[0]["text"] == "A"

    def test_redo_on_empty_stack_is_safe(self):
        backend = self._backend()
        backend.redoAnnotation()  # must not raise
        assert backend.annotationCount == 0

    def test_undo_redo_sequence(self):
        backend = self._backend()
        backend.addAnnotation(self._sample(text="A"))
        backend.addAnnotation(self._sample(text="B"))
        backend.removeLastAnnotation()  # undo B
        backend.removeLastAnnotation()  # undo A
        backend.redoAnnotation()        # redo A
        backend.redoAnnotation()        # redo B
        texts = [a["text"] for a in backend.annotations]
        assert texts == ["A", "B"]

    def test_add_clears_redo_stack(self):
        """Adding a new annotation must discard the redo history."""
        backend = self._backend()
        backend.addAnnotation(self._sample(text="A"))
        backend.removeLastAnnotation()  # undo → redo stack has A
        backend.addAnnotation(self._sample(text="B"))  # should clear redo
        backend.redoAnnotation()  # nothing to redo
        assert backend.annotationCount == 1
        assert backend.annotations[0]["text"] == "B"

    def test_annotationsChanged_emitted_on_redo(self):
        backend = self._backend()
        backend.addAnnotation(self._sample())
        backend.removeLastAnnotation()
        received = []
        backend.annotationsChanged.connect(lambda: received.append(1))
        backend.redoAnnotation()
        assert len(received) == 1

    def test_annotationsChanged_not_emitted_on_redo_when_empty(self):
        backend = self._backend()
        received = []
        backend.annotationsChanged.connect(lambda: received.append(1))
        backend.redoAnnotation()
        assert len(received) == 0

    def test_annotations_to_markdown_empty_when_no_annotations(self):
        backend = self._backend()
        assert backend.annotationsToMarkdown() == ""

    def test_annotations_to_markdown_uses_human_readable_format(self):
        backend = self._backend()
        backend.addAnnotation(
            self._sample(
                text="48 × 32 px",
                mode=0,
                cursorX=412,
                cursorY=230,
            )
        )
        backend.addAnnotation(
            self._sample(
                text="320 × 200 px",
                mode=2,
                cursorX=100,
                cursorY=80,
            )
        )
        markdown = backend.annotationsToMarkdown()
        assert markdown == (
            "- Crosshair @ (412, 230): 48 × 32 px\n"
            "- Container @ (100, 80): 320 × 200 px"
        )

    def test_copy_annotations_markdown_to_clipboard_uses_exported_text(self, monkeypatch):
        backend = self._backend()
        backend.addAnnotation(self._sample(text="A"))

        copied = []
        monkeypatch.setattr(backend, "_copy_text_to_clipboard", lambda text: copied.append(text))

        backend.copyAnnotationsMarkdownToClipboard()

        assert copied == [backend.annotationsToMarkdown()]

    def test_build_composite_image_for_region_crops_source_image(self):
        from PyQt6.QtGui import QColor, QImage

        source = QImage(20, 20, QImage.Format.Format_ARGB32)
        source.fill(QColor("#112233"))
        edge_map = np.zeros((20, 20), dtype=bool)
        backend = RulerBackend(
            edge_map=edge_map,
            dpr_x=1.0,
            dpr_y=1.0,
            virtual_x=0,
            virtual_y=0,
            virtual_w=20,
            virtual_h=20,
            source_image=source,
            threshold_low=50,
            threshold_high=150,
            always_show_debug_overlay=False,
        )

        cropped = backend.buildCompositeImageForRegion(4, 6, 8, 5)

        assert not cropped.isNull()
        assert cropped.width() == 8
        assert cropped.height() == 5
        assert cropped.pixelColor(0, 0) == QColor("#112233")

    def test_copy_composite_region_to_clipboard_uses_generated_image(self, monkeypatch):
        from PyQt6.QtGui import QColor, QImage

        source = QImage(20, 20, QImage.Format.Format_ARGB32)
        source.fill(QColor("#abcdef"))
        edge_map = np.zeros((20, 20), dtype=bool)
        backend = RulerBackend(
            edge_map=edge_map,
            dpr_x=1.0,
            dpr_y=1.0,
            virtual_x=0,
            virtual_y=0,
            virtual_w=20,
            virtual_h=20,
            source_image=source,
            threshold_low=50,
            threshold_high=150,
            always_show_debug_overlay=False,
        )

        copied_sizes = []
        monkeypatch.setattr(
            backend,
            "_copy_image_to_clipboard",
            lambda image: copied_sizes.append((image.width(), image.height())),
        )

        result = backend.copyCompositeRegionToClipboard(2, 3, 7, 4)

        assert result is True
        assert copied_sizes == [(7, 4)]
