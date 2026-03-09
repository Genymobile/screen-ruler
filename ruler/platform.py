"""Platform/session helpers shared by capture and backend."""

from __future__ import annotations

import os

from PyQt6.QtGui import QGuiApplication


def session_type(app: QGuiApplication | None = None) -> str:
    """Return a normalized session type: 'wayland', 'x11', or ''."""
    gui_app = app if app is not None else QGuiApplication.instance()
    if gui_app is not None:
        platform_name = (gui_app.platformName() or "").strip().lower()
        if "wayland" in platform_name:
            return "wayland"
        if "xcb" in platform_name or "x11" in platform_name:
            return "x11"

    return os.environ.get("XDG_SESSION_TYPE", "").strip().lower()


def is_wayland_session(app: QGuiApplication | None = None) -> bool:
    """True when current Qt session backend is Wayland."""
    return session_type(app) == "wayland"
