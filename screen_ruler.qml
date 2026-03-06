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
import QtQuick.Controls

Window {
    id: root
    property var backend: (typeof ruler !== "undefined" ? ruler : null)
    property bool hasBackend: backend !== null

    readonly property color crosshairColor: "#00DCFF"
    readonly property int crosshairLineWidth: 1
    readonly property int crosshairTickHalfLength: 5

    readonly property int uiBaseMargin: 14
    readonly property int uiCornerRadius: 5
    readonly property color uiPanelBackgroundColor: Qt.rgba(0.16, 0.17, 0.19, 1.0)
    readonly property real uiPanelOpacity: 0.9
    readonly property color uiPrimaryTextColor: "#D7DADF"

    readonly property int labelOffsetX: uiBaseMargin
    readonly property int labelOffsetY: 4
    readonly property int labelShadowOffset: 2
    readonly property int labelRadius: uiCornerRadius
    readonly property int labelHorizontalPadding: 18
    readonly property int labelVerticalPadding: 12
    readonly property real labelOpacity: uiPanelOpacity
    readonly property color labelBackgroundColor: uiPanelBackgroundColor
    readonly property color labelShadowColor: Qt.rgba(0, 0, 0, 0.22)
    readonly property color labelTextColor: uiPrimaryTextColor

    readonly property real debugOverlayOpacity: 0.3
    readonly property int edgePreviewHoldMs: 1000
    readonly property int edgePreviewFadeMs: 1000
    readonly property real edgePreviewPeakOpacity: 0.5
    property real edgePreviewOpacity: 0.0

    readonly property int controlsTopMargin: uiBaseMargin
    readonly property int controlsPanelWidth: 380
    readonly property int controlsPanelHeight: 56
    readonly property int controlsPanelRadius: uiCornerRadius
    readonly property int controlsPanelZ: 30
    readonly property color controlsPanelColor: uiPanelBackgroundColor
    readonly property real controlsPanelOpacity: uiPanelOpacity
    readonly property int controlsSideMargin: uiBaseMargin
    readonly property int controlsRowSpacing: 12
    readonly property color controlsTextColor: uiPrimaryTextColor
    readonly property int controlsTitlePointSize: 11
    readonly property int controlsValuePointSize: 10
    readonly property int sensitivityMin: 0
    readonly property int sensitivityMax: 100
    readonly property int sensitivityStep: 1
    readonly property int sensitivitySliderWidth: 240
    readonly property int sensitivityDefaultValue: 50

    readonly property int screenshotZ: -2
    readonly property int edgeOverlayZ: -1

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

    Shortcut {
        sequence: "Escape"
        onActivated: Qt.quit()
    }

    Shortcut {
        sequence: "Q"
        onActivated: Qt.quit()
    }

    // -----------------------------------------------------------------------
    // Keyboard handling — Escape or Q exits the application
    // -----------------------------------------------------------------------
    Item {
        anchors.fill: parent

        MouseArea {
            anchors.fill: parent
            enabled: hasBackend
            onClicked: backend.copySizeToClipboardAndQuit()
        }
    }

    Rectangle {
        id: controlsPanel
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.topMargin: controlsTopMargin
        width: controlsPanelWidth
        height: controlsPanelHeight
        radius: controlsPanelRadius
        color: controlsPanelColor
        opacity: controlsPanelOpacity
        z: controlsPanelZ

        MouseArea {
            anchors.fill: parent
            onClicked: (mouse) => mouse.accepted = true
        }

        Row {
            anchors.fill: parent
            anchors.leftMargin: controlsSideMargin
            anchors.rightMargin: controlsSideMargin
            spacing: controlsRowSpacing

            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: "Sensitivity"
                color: controlsTextColor
                font.pointSize: controlsTitlePointSize
                font.bold: true
            }

            Slider {
                id: sensitivitySlider
                anchors.verticalCenter: parent.verticalCenter
                from: sensitivityMin
                to: sensitivityMax
                stepSize: sensitivityStep
                width: sensitivitySliderWidth
                enabled: hasBackend
                value: hasBackend ? backend.sensitivity : sensitivityDefaultValue
                onMoved: {
                    edgePreviewOpacity = edgePreviewPeakOpacity
                    if (hasBackend) backend.setSensitivity(value)
                }
            }

            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: Math.round(sensitivitySlider.value)
                color: controlsTextColor
                font.pointSize: controlsValuePointSize
            }
        }

        Connections {
            target: hasBackend ? backend : null
            function onControlsChanged() {
                if (!sensitivitySlider.pressed) {
                    sensitivitySlider.value = backend.sensitivity
                }
            }
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
        z: screenshotZ
    }

    // -----------------------------------------------------------------------
    // Optional debug overlay — displays the captured Canny edge map
    // -----------------------------------------------------------------------
    Image {
        anchors.fill: parent
        visible: hasBackend
                 && backend.debugOverlaySource !== ""
                 && (backend.debugOverlayEnabled || edgePreviewOpacity > 0)
        source: hasBackend ? backend.debugOverlaySource : ""
        fillMode: Image.Stretch
        smooth: false
        opacity: hasBackend && backend.debugOverlayEnabled
                 ? Math.max(debugOverlayOpacity, edgePreviewOpacity)
                 : edgePreviewOpacity
        z: edgeOverlayZ
    }

    Connections {
        target: hasBackend ? backend : null
        function onEdgeMapPreviewRequested() {
            edgePreviewAnimation.restart()
        }
    }

    SequentialAnimation {
        id: edgePreviewAnimation
        PropertyAction {
            target: root
            property: "edgePreviewOpacity"
            value: edgePreviewPeakOpacity
        }
        PauseAnimation {
            duration: edgePreviewHoldMs
        }
        NumberAnimation {
            target: root
            property: "edgePreviewOpacity"
            to: 0
            duration: edgePreviewFadeMs
            easing.type: Easing.OutQuad
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
            target: hasBackend ? backend : null
            function onDataChanged() { canvas.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, canvas.width, canvas.height)

            if (!hasBackend || backend.cursorX < 0) return

            // Cyan crosshair
            ctx.strokeStyle = crosshairColor
            ctx.lineWidth   = crosshairLineWidth
            ctx.beginPath()
            // Vertical ray: north tip → cursor → south tip
            ctx.moveTo(backend.cursorX, backend.northEnd)
            ctx.lineTo(backend.cursorX, backend.southEnd)
            // Horizontal ray: west tip → cursor → east tip
            ctx.moveTo(backend.westEnd,  backend.cursorY)
            ctx.lineTo(backend.eastEnd,  backend.cursorY)

            // Perpendicular end-caps — small tick marks at each ray tip
            // that highlight the measured boundary
            var t = crosshairTickHalfLength  // half-length of each tick → 10 px total
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
        id: labelShadow

        x:       labelBox.x + labelShadowOffset
        y:       labelBox.y + labelShadowOffset
        width:   labelBox.width
        height:  labelBox.height
        color:   labelShadowColor
        radius:  labelRadius
        visible: labelBox.visible
        z:       1
    }

    Rectangle {
        id: labelBox

        x:       (hasBackend ? backend.cursorX : -labelOffsetX) + labelOffsetX
        y:       (hasBackend ? backend.cursorY : -labelOffsetY) + labelOffsetY
        width:   sizeText.implicitWidth + labelHorizontalPadding
        height:  sizeText.implicitHeight + labelVerticalPadding
        color:   labelBackgroundColor
        opacity: labelOpacity
        radius:  labelRadius
        visible: hasBackend && backend.cursorX >= 0
        z:       2

        Text {
            id: sizeText
            anchors.centerIn: parent
            text:             (hasBackend ? backend.widthPx : 0)
                             + " × "
                             + (hasBackend ? backend.heightPx : 0)
                             + " px"
            color:            labelTextColor
            font.family:      "DejaVu Sans Mono, Consolas, monospace"
            font.pointSize:   13
            font.bold:        true
        }
    }
}
