"""Application entrypoint and bootstrap wiring for screen-ruler."""

from __future__ import annotations

import argparse
import os
import sys

from PyQt6.QtCore import QEvent, QObject, QRect, QTimer, QUrl, Qt
from PyQt6.QtGui import QGuiApplication, QKeyEvent
from PyQt6.QtQml import QQmlApplicationEngine

from .backend import RulerBackend
from .capture import _capture_tool_hint, capture_screen
from .core import compute_edge_map
from .overlay import (
    create_debug_edge_overlay_source,
    create_screenshot_overlay_source,
)

TIMER_INTERVAL_MS = 16


class QuitKeysEventFilter(QObject):
    """Catch Esc/Q at app level when WM focus is unreliable on X11 overlays."""

    def eventFilter(self, _obj: QObject, event: QEvent) -> bool:
        if event.type() != QEvent.Type.KeyPress:
            return False
        if not isinstance(event, QKeyEvent):
            return False

        key = event.key()
        if key in (Qt.Key.Key_Escape, Qt.Key.Key_Q):
            QGuiApplication.quit()
            return True
        return False


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Smart Edge-Detection Screen Ruler - "
            "move the mouse to measure distances between UI edges."
        ),
        epilog=(
            "Mouse interactions:\n"
            "  - Left click: copy the current measurement to clipboard and quit\n"
            "  - Mouse wheel: adjust sensitivity\n"
            "Keyboard shortcuts:\n"
            "  - 1 / 2 / 3: switch measurement mode\n"
            "  - Ctrl+C: copy measurement to clipboard and quit\n"
            "  - ? / H: toggle shortcut overlay\n"
            "  - Esc / Q: quit"
        ),
        formatter_class=argparse.RawTextHelpFormatter,
    )
    parser.add_argument(
        "--threshold-low",
        type=int,
        default=16,
        metavar="N",
        help="Lower Canny edge-detection threshold (default: 16)",
    )
    parser.add_argument(
        "--threshold-high",
        type=int,
        default=54,
        metavar="N",
        help="Upper Canny edge-detection threshold (default: 54)",
    )
    parser.add_argument(
        "--debug-edge-overlay",
        action="store_true",
        help=(
            "Show the captured Canny edge map as a 30%%-opacity overlay "
            "for alignment debugging"
        ),
    )
    return parser.parse_args()


def find_qml() -> str:
    """Return the absolute path to screen_ruler.qml."""
    if getattr(sys, "frozen", False) and hasattr(sys, "_MEIPASS"):
        base = getattr(sys, "_MEIPASS")
    else:
        # app.py now lives under ruler/, while QML assets live under qml/
        # at the repository root.
        base = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    return os.path.join(base, "qml", "screen_ruler.qml")


def main() -> None:
    args = parse_args()

    os.environ["QT_QUICK_CONTROLS_UNIVERSAL_ACCENT"] = "#E6195E"
    try:
        from PyQt6.QtQuickControls2 import QQuickStyle

        QQuickStyle.setStyle("Universal")
    except Exception:
        os.environ.setdefault("QT_QUICK_CONTROLS_STYLE", "Universal")

    app = QGuiApplication(sys.argv)
    app.setApplicationName("Screen Ruler")
    quit_filter = QuitKeysEventFilter(app)
    app.installEventFilter(quit_filter)

    all_rect = QRect()
    for screen in app.screens():
        all_rect = all_rect.united(screen.geometry())

    print("Capturing screen...")
    qimage = capture_screen(app)

    print(
        f"Computing edge map  (Canny thresholds: {args.threshold_low} / {args.threshold_high})..."
    )
    try:
        edge_map = compute_edge_map(qimage, args.threshold_low, args.threshold_high)
    except ValueError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        print(
            "Screenshot capture may be unavailable in this desktop/session environment.",
            file=sys.stderr,
        )
        hint = _capture_tool_hint()
        if hint:
            print(hint, file=sys.stderr)
        sys.exit(1)

    print(
        f"Edge map: {edge_map.shape[1]}x{edge_map.shape[0]} px  |  "
        f"{edge_map.sum():,} edge pixels found."
    )

    if all_rect.width() <= 0 or all_rect.height() <= 0:
        dpr_x = 1.0
        dpr_y = 1.0
    else:
        dpr_x = edge_map.shape[1] / all_rect.width()
        dpr_y = edge_map.shape[0] / all_rect.height()

    debug_overlay_source = create_debug_edge_overlay_source(edge_map) or ""
    screenshot_source = create_screenshot_overlay_source(qimage) or ""
    if not screenshot_source:
        print("Warning: failed to create screenshot background image.", file=sys.stderr)
    if not debug_overlay_source:
        print("Warning: failed to create debug edge overlay image.", file=sys.stderr)

    ruler = RulerBackend(
        edge_map,
        dpr_x,
        dpr_y,
        all_rect.x(),
        all_rect.y(),
        all_rect.width(),
        all_rect.height(),
        qimage,
        args.threshold_low,
        args.threshold_high,
        args.debug_edge_overlay,
        screenshot_source,
        debug_overlay_source,
    )

    engine = QQmlApplicationEngine()
    engine.rootContext().setContextProperty("ruler", ruler)
    engine.load(QUrl.fromLocalFile(find_qml()))

    if not engine.rootObjects():
        print("Error: failed to load QML UI", file=sys.stderr)
        sys.exit(1)

    timer = QTimer()
    timer.timeout.connect(ruler.poll)
    timer.start(TIMER_INTERVAL_MS)

    sys.exit(app.exec())
