#!/usr/bin/env python3
"""Compatibility facade for screen-ruler modules.

This module keeps existing imports stable while implementation details
are split into smaller files.
"""

from __future__ import annotations

# Keep module aliases for test monkeypatch paths like:
#   monkeypatch.setattr("screen_ruler.shutil.which", ...)
#   monkeypatch.setattr("screen_ruler.subprocess.run", ...)
import shutil as shutil
import subprocess as subprocess

from screen_ruler_app import find_qml as _find_qml
from screen_ruler_app import main, parse_args
from screen_ruler_backend import RulerBackend
from screen_ruler_capture import _capture_screen_external, _capture_tool_hint, capture_screen
from screen_ruler_core import compute_edge_map, trace_ray
from screen_ruler_overlay import (
    create_debug_edge_overlay_source as _create_debug_edge_overlay_source,
    create_screenshot_overlay_source as _create_screenshot_overlay_source,
    edge_map_to_qimage as _edge_map_to_qimage,
)

__all__ = [
    "RulerBackend",
    "_capture_screen_external",
    "_capture_tool_hint",
    "_create_debug_edge_overlay_source",
    "_create_screenshot_overlay_source",
    "_edge_map_to_qimage",
    "_find_qml",
    "capture_screen",
    "compute_edge_map",
    "main",
    "parse_args",
    "trace_ray",
]


if __name__ == "__main__":
    main()
