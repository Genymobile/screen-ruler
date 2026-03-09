"""Screenshot capture helpers for Wayland/X11 environments."""

from __future__ import annotations

import shutil
import subprocess

from PyQt6.QtCore import QEventLoop, QRect, QTimer
from PyQt6.QtGui import QGuiApplication, QImage

from .platform import session_type

CAPTURE_SETTLE_MS = 250


def capture_screen(app: QGuiApplication) -> QImage:
    """Capture a screenshot covering all connected monitors."""
    screens = app.screens()
    if not screens:
        return QImage()

    all_rect = QRect()
    for screen in screens:
        all_rect = all_rect.united(screen.geometry())

    if all_rect.width() <= 0 or all_rect.height() <= 0:
        return QImage()

    current_session = session_type(app)
    if current_session == "wayland":
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


def _capture_screen_qt_native(app: QGuiApplication, timeout_ms: int = 12000) -> QImage:
    """Try Qt Multimedia native screen capture (portal-based on Wayland)."""
    try:
        from PyQt6.QtMultimedia import QMediaCaptureSession, QScreenCapture, QVideoSink
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

    result: dict[str, QImage] = {"image": QImage(), "latest": QImage()}
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


def _capture_screen_external(all_rect: QRect) -> QImage:
    """Try an external screenshot tool based on session type."""
    current_session = session_type()
    command = None

    if current_session == "wayland":
        grim = shutil.which("grim")
        if grim:
            geometry = f"{all_rect.x()},{all_rect.y()} {all_rect.width()}x{all_rect.height()}"
            command = [grim, "-g", geometry, "-t", "png", "-"]
    elif current_session == "x11":
        imagemagick_import = shutil.which("import")
        if imagemagick_import:
            command = [imagemagick_import, "-window", "root", "png:-"]

    if not command:
        return QImage()

    try:
        result = subprocess.run(command, check=True, capture_output=True, timeout=5)
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
    current_session = session_type()
    if current_session == "wayland":
        try:
            from PyQt6.QtMultimedia import QMediaCaptureSession, QScreenCapture, QVideoSink
            _ = (QMediaCaptureSession, QScreenCapture, QVideoSink)
            return "Tip: when prompted, allow desktop screen-capture authorization for this app."
        except Exception:
            if not shutil.which("grim"):
                return "Tip: install 'grim' to enable Wayland fallback screenshot capture."
    if current_session == "x11" and not shutil.which("import"):
        return "Tip: install ImageMagick ('import') to enable X11 fallback screenshot capture."
    return None
