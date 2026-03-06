// screen_ruler.qml
//
// Transparent full-screen overlay for the screen-ruler tool.
//
// Visual elements:
//   * Canvas  — cyan crosshair lines (N/S/E/W rays)
//   * Rectangle + Column of Text — semi-transparent W/H label box
//
// All data comes from the ``ruler`` context property (RulerBackend QObject)
// exposed by the Python host via QQmlApplicationEngine.rootContext().

import QtQuick
import QtQuick.Window

Window {
    id: root
    property var backend: (typeof ruler !== "undefined" ? ruler : null)
    property bool hasBackend: backend !== null

    // Position and size are driven by the virtual desktop bounds that Python
    // computed from the union of all screen geometries.
    x:      hasBackend && backend.isWaylandSession ? 0 : (hasBackend ? backend.virtualDesktopX : 0)
    y:      hasBackend && backend.isWaylandSession ? 0 : (hasBackend ? backend.virtualDesktopY : 0)
    width:  hasBackend && backend.isWaylandSession ? Screen.width : (hasBackend ? backend.virtualDesktopWidth : Screen.width)
    height: hasBackend && backend.isWaylandSession ? Screen.height : (hasBackend ? backend.virtualDesktopHeight : Screen.height)

    // Frameless, always-on-top, transparent overlay
        flags:  hasBackend && backend.isWaylandSession
            ? (Qt.WindowStaysOnTopHint | Qt.FramelessWindowHint)
            : (Qt.WindowStaysOnTopHint
             | Qt.FramelessWindowHint
             | Qt.Tool
             | Qt.X11BypassWindowManagerHint)
    color:   "transparent"
        visibility: hasBackend && backend.isWaylandSession ? Window.FullScreen : Window.Windowed
    title:   "Screen Ruler"

    // -----------------------------------------------------------------------
    // Keyboard handling — Escape or Q exits the application
    // -----------------------------------------------------------------------
    Item {
        anchors.fill: parent
        focus: true
        Keys.onEscapePressed: Qt.quit()
        Keys.onPressed: (event) => {
            if (event.key === Qt.Key_Q) Qt.quit()
        }
    }

    // -----------------------------------------------------------------------
    // Captured screenshot background (always shown)
    // -----------------------------------------------------------------------
    Image {
        anchors.fill: parent
        visible: hasBackend && backend.screenshotAvailable
        source: hasBackend ? backend.screenshotSource : ""
        fillMode: Image.Stretch
        smooth: false
        opacity: 1.0
        z: -2
    }

    // -----------------------------------------------------------------------
    // Optional debug overlay — displays the captured Canny edge map
    // -----------------------------------------------------------------------
    Image {
        anchors.fill: parent
        visible: hasBackend && backend.debugOverlayEnabled
        source: hasBackend ? backend.debugOverlaySource : ""
        fillMode: Image.Stretch
        smooth: false
        opacity: 0.3
        z: -1
    }

    // -----------------------------------------------------------------------
    // Crosshair canvas — repainted on every cursor move
    // -----------------------------------------------------------------------
    Canvas {
        id: canvas
        anchors.fill: parent

        // Re-request a paint pass whenever the ruler backend emits dataChanged
        Connections {
            target: hasBackend ? backend : null
            function onDataChanged() { canvas.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, canvas.width, canvas.height)

            if (!hasBackend || backend.cursorX < 0) return

            // Cyan crosshair
            ctx.strokeStyle = "#00DCFF"
            ctx.lineWidth   = 1
            ctx.beginPath()
            // Vertical ray: north tip → cursor → south tip
            ctx.moveTo(backend.cursorX, backend.northEnd)
            ctx.lineTo(backend.cursorX, backend.southEnd)
            // Horizontal ray: west tip → cursor → east tip
            ctx.moveTo(backend.westEnd,  backend.cursorY)
            ctx.lineTo(backend.eastEnd,  backend.cursorY)

            // Perpendicular end-caps — small tick marks at each ray tip
            // that highlight the measured boundary
            var t = 5  // half-length of each tick → 10 px total
            // North tip (horizontal tick)
            ctx.moveTo(backend.cursorX - t, backend.northEnd)
            ctx.lineTo(backend.cursorX + t, backend.northEnd)
            // South tip (horizontal tick)
            ctx.moveTo(backend.cursorX - t, backend.southEnd)
            ctx.lineTo(backend.cursorX + t, backend.southEnd)
            // West tip (vertical tick)
            ctx.moveTo(backend.westEnd, backend.cursorY - t)
            ctx.lineTo(backend.westEnd, backend.cursorY + t)
            // East tip (vertical tick)
            ctx.moveTo(backend.eastEnd, backend.cursorY - t)
            ctx.lineTo(backend.eastEnd, backend.cursorY + t)

            ctx.stroke()
        }
    }

    // -----------------------------------------------------------------------
    // Measurement label — semi-transparent box, positioned near cursor
    // -----------------------------------------------------------------------
    Rectangle {
        id: labelBox

        x:       (hasBackend ? backend.cursorX : -14) + 14
        y:       (hasBackend ? backend.cursorY : -4) + 4
        width:   labelColumn.width  + 12
        height:  labelColumn.height + 12
        color:   Qt.rgba(0, 0, 0, 0.63)
        radius:  3
        visible: hasBackend && backend.cursorX >= 0

        Column {
            id: labelColumn
            anchors.centerIn: parent
            spacing: 2

            Text {
                text:             "W: " + (hasBackend ? backend.widthPx : 0)  + " px"
                color:            "#FFFF3C"
                font.family:      "DejaVu Sans Mono, Consolas, monospace"
                font.pointSize:   13
                font.bold:        true
            }

            Text {
                text:             "H: " + (hasBackend ? backend.heightPx : 0) + " px"
                color:            "#FFFF3C"
                font.family:      "DejaVu Sans Mono, Consolas, monospace"
                font.pointSize:   13
                font.bold:        true
            }
        }
    }
}
