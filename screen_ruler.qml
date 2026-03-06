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

    // Position and size are driven by the virtual desktop bounds that Python
    // computed from the union of all screen geometries.
    x:      ruler.virtualDesktopX
    y:      ruler.virtualDesktopY
    width:  ruler.virtualDesktopWidth
    height: ruler.virtualDesktopHeight

    // Frameless, always-on-top, transparent overlay
    flags:  Qt.WindowStaysOnTopHint
          | Qt.FramelessWindowHint
          | Qt.Tool
          | Qt.X11BypassWindowManagerHint
    color:   "transparent"
    visible: true
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
    // Crosshair canvas — repainted on every cursor move
    // -----------------------------------------------------------------------
    Canvas {
        id: canvas
        anchors.fill: parent

        // Re-request a paint pass whenever the ruler backend emits dataChanged
        Connections {
            target: ruler
            function onDataChanged() { canvas.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, canvas.width, canvas.height)

            if (ruler.cursorX < 0) return

            // Cyan crosshair
            ctx.strokeStyle = "#00DCFF"
            ctx.lineWidth   = 1
            ctx.beginPath()
            // Vertical ray: north tip → cursor → south tip
            ctx.moveTo(ruler.cursorX, ruler.northEnd)
            ctx.lineTo(ruler.cursorX, ruler.southEnd)
            // Horizontal ray: west tip → cursor → east tip
            ctx.moveTo(ruler.westEnd,  ruler.cursorY)
            ctx.lineTo(ruler.eastEnd,  ruler.cursorY)

            // Perpendicular end-caps — small tick marks at each ray tip
            // that highlight the measured boundary
            var t = 5  // half-length of each tick → 10 px total
            // North tip (horizontal tick)
            ctx.moveTo(ruler.cursorX - t, ruler.northEnd)
            ctx.lineTo(ruler.cursorX + t, ruler.northEnd)
            // South tip (horizontal tick)
            ctx.moveTo(ruler.cursorX - t, ruler.southEnd)
            ctx.lineTo(ruler.cursorX + t, ruler.southEnd)
            // West tip (vertical tick)
            ctx.moveTo(ruler.westEnd, ruler.cursorY - t)
            ctx.lineTo(ruler.westEnd, ruler.cursorY + t)
            // East tip (vertical tick)
            ctx.moveTo(ruler.eastEnd, ruler.cursorY - t)
            ctx.lineTo(ruler.eastEnd, ruler.cursorY + t)

            ctx.stroke()
        }
    }

    // -----------------------------------------------------------------------
    // Measurement label — semi-transparent box, positioned near cursor
    // -----------------------------------------------------------------------
    Rectangle {
        id: labelBox

        x:       ruler.cursorX + 14
        y:       ruler.cursorY + 4
        width:   labelColumn.width  + 12
        height:  labelColumn.height + 12
        color:   Qt.rgba(0, 0, 0, 0.63)
        radius:  3
        visible: ruler.cursorX >= 0

        Column {
            id: labelColumn
            anchors.centerIn: parent
            spacing: 2

            Text {
                text:             "W: " + ruler.widthPx  + " px"
                color:            "#FFFF3C"
                font.family:      "DejaVu Sans Mono, Consolas, monospace"
                font.pointSize:   13
                font.bold:        true
            }

            Text {
                text:             "H: " + ruler.heightPx + " px"
                color:            "#FFFF3C"
                font.family:      "DejaVu Sans Mono, Consolas, monospace"
                font.pointSize:   13
                font.bold:        true
            }
        }
    }
}
