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
    readonly property int modeDynamicEdge: 0
    readonly property int modeRectDrag: 1
    readonly property int modeContainerTrace: 2
    property int activeMode: modeDynamicEdge
    property real pointerX: 0
    property real pointerY: 0
    property real snappedPointerX: 0
    property real snappedPointerY: 0
    property bool snappedPointerIsSnapped: false
    property bool rectDragActive: false
    property bool rectHasSelection: false
    property real rectStartX: 0
    property real rectStartY: 0
    property real rectEndX: 0
    property real rectEndY: 0

    readonly property real rectLeft: Math.min(rectStartX, rectEndX)
    readonly property real rectTop: Math.min(rectStartY, rectEndY)
    readonly property real rectWidth: Math.abs(rectEndX - rectStartX)
    readonly property real rectHeight: Math.abs(rectEndY - rectStartY)
    property bool containerHasSelection: false
    property real containerX: 0
    property real containerY: 0
    property real containerWidth: 0
    property real containerHeight: 0
    property real rectSnapDistance: 10
    readonly property int rectSnapMin: 0
    readonly property int rectSnapMax: 30
    readonly property int rectSnapStep: 1

    function setActiveMode(mode) {
        var clamped = Math.max(modeDynamicEdge, Math.min(modeContainerTrace, mode))
        if (activeMode === clamped)
            return
        activeMode = clamped
        if (activeMode !== modeRectDrag) {
            rectDragActive = false
            rectHasSelection = false
            snappedPointerIsSnapped = false
        } else {
            refreshSnappedPointer()
        }
        if (activeMode !== modeContainerTrace) {
            containerHasSelection = false
        } else {
            refreshContainerSelection()
        }
    }

    function updatePointer(mouse) {
        pointerX = mouse.x
        pointerY = mouse.y
        if (activeMode === modeRectDrag)
            refreshSnappedPointer()
        if (activeMode === modeContainerTrace)
            refreshContainerSelection()
    }

    function refreshContainerSelection() {
        if (!hasBackend || activeMode !== modeContainerTrace) {
            containerHasSelection = false
            return
        }

        var detected = backend.detectContainerAtPoint(pointerX, pointerY)
        if (!detected || !detected.available) {
            containerHasSelection = false
            return
        }

        containerX = detected.x
        containerY = detected.y
        containerWidth = detected.width
        containerHeight = detected.height
        containerHasSelection = true
    }

    function refreshSnappedPointer() {
        if (activeMode !== modeRectDrag) {
            snappedPointerX = pointerX
            snappedPointerY = pointerY
            snappedPointerIsSnapped = false
            return
        }
        var p = maybeSnapPoint(pointerX, pointerY)
        snappedPointerX = p.x
        snappedPointerY = p.y
        snappedPointerIsSnapped = p.snapped === true
    }

    function maybeSnapPoint(x, y) {
        if (!hasBackend)
            return { x: x, y: y, snapped: false }

        var snapped = backend.snapPointToNearestEdge(x, y, rectSnapDistance)
        if (snapped && snapped.snapped)
            return { x: snapped.x, y: snapped.y, snapped: true }
        return { x: x, y: y, snapped: false }
    }

    function beginRectDrag() {
        rectDragActive = true
        rectHasSelection = true
        rectStartX = snappedPointerX
        rectStartY = snappedPointerY
        rectEndX = snappedPointerX
        rectEndY = snappedPointerY
    }

    function updateRectDrag() {
        if (!rectDragActive)
            return
        rectEndX = snappedPointerX
        rectEndY = snappedPointerY
    }

    function endRectDrag() {
        if (!rectDragActive)
            return
        rectEndX = snappedPointerX
        rectEndY = snappedPointerY
        rectDragActive = false
    }

    function clampSensitivity(value) {
        return Math.max(sensitivityMin, Math.min(sensitivityMax, value))
    }

    function clampRectSnapDistance(value) {
        return Math.max(rectSnapMin, Math.min(rectSnapMax, value))
    }

    function applySensitivityValue(value) {
        if (!hasBackend)
            return
        edgePreviewOpacity = edgePreviewPeakOpacity
        backend.setSensitivity(clampSensitivity(value))
        refreshSnappedPointer()
        refreshContainerSelection()
    }

    function applySnapDistanceValue(value) {
        rectSnapDistance = clampRectSnapDistance(value)
        refreshSnappedPointer()
    }

    function adjustSensitivityByWheelSteps(notchSteps) {
        var nextSensitivity = sensitivitySlider.value + notchSteps * sensitivityStep
        nextSensitivity = Math.max(sensitivityMin, Math.min(sensitivityMax, nextSensitivity))
        if (nextSensitivity === sensitivitySlider.value)
            return
        sensitivitySlider.value = nextSensitivity
        applySensitivityValue(nextSensitivity)
    }

    function adjustSnapDistanceByWheelSteps(notchSteps) {
        var nextSnap = snapDistanceSlider.value + notchSteps * rectSnapStep
        nextSnap = Math.max(rectSnapMin, Math.min(rectSnapMax, nextSnap))
        if (nextSnap === snapDistanceSlider.value)
            return
        snapDistanceSlider.value = nextSnap
        applySnapDistanceValue(nextSnap)
    }

    function adjustActiveModeSliderByWheel(wheelDeltaY) {
        if (!hasBackend || wheelDeltaY === 0)
            return
        if (
            activeMode !== modeDynamicEdge
            && activeMode !== modeRectDrag
            && activeMode !== modeContainerTrace
        )
            return

        var notchSteps = wheelDeltaY / 120
        if (notchSteps === 0)
            notchSteps = wheelDeltaY > 0 ? 1 : -1

        if (activeMode === modeRectDrag) {
            adjustSnapDistanceByWheelSteps(notchSteps)
            return
        }

        adjustSensitivityByWheelSteps(notchSteps)
    }

    readonly property color crosshairColor: "#E6195E"
    readonly property int crosshairLineWidth: 1
    readonly property int crosshairTickHalfLength: 5

    readonly property int uiBaseMargin: 14
    readonly property int uiCornerRadius: 5
    readonly property color uiPanelBackgroundColor: "#1A1A1A"
    readonly property real uiPanelOpacity: 0.9
    readonly property color uiPrimaryTextColor: "#FFFFFF"

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
    readonly property int controlsPanelHeight: root.activeMode === modeRectDrag ? 136 : 104
    readonly property int controlsPanelRadius: uiCornerRadius
    readonly property int controlsPanelZ: 30
    readonly property color controlsPanelColor: uiPanelBackgroundColor
    readonly property real controlsPanelOpacity: uiPanelOpacity
    readonly property int controlsSideMargin: uiBaseMargin
    readonly property int controlsRowSpacing: 12
    readonly property color controlsTextColor: uiPrimaryTextColor
    readonly property int controlsTitlePointSize: 11
    readonly property int controlsValuePointSize: 10
    readonly property int modeButtonSize: 30
    readonly property int modeButtonRadius: 4
    readonly property color modeButtonBorderColor: Qt.rgba(0.52, 0.56, 0.61, 0.5)
    readonly property color modeButtonActiveBorderColor: crosshairColor
    readonly property color modeButtonBgColor: Qt.rgba(0.26, 0.28, 0.31, 0.7)
    readonly property color modeButtonActiveBgColor: Qt.rgba(0.90, 0.10, 0.37, 0.18)
    readonly property int controlsColumnSpacing: 8
    readonly property int modeRowSpacing: 8
    readonly property int sensitivityMin: 0
    readonly property int sensitivityMax: 100
    readonly property int sensitivityStep: 1
    readonly property int sensitivitySliderWidth: 240
    readonly property int sensitivityDefaultValue: 85

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
            hoverEnabled: true

            onPressed: (mouse) => {
                root.updatePointer(mouse)
                if (root.activeMode === modeRectDrag && mouse.button === Qt.LeftButton) {
                    root.beginRectDrag()
                    mouse.accepted = true
                }
            }

            onPositionChanged: (mouse) => {
                root.updatePointer(mouse)
                if (root.activeMode === modeRectDrag)
                    root.updateRectDrag()
            }

            onReleased: (mouse) => {
                root.updatePointer(mouse)
                if (root.activeMode === modeRectDrag && mouse.button === Qt.LeftButton) {
                    root.endRectDrag()
                    mouse.accepted = true
                }
            }

            onClicked: (mouse) => {
                var canCopy = root.activeMode === modeDynamicEdge
                              || (root.activeMode === modeContainerTrace && root.containerHasSelection)
                if (!canCopy) {
                    mouse.accepted = true
                    return
                }

                var widthPx = root.activeMode === modeDynamicEdge ? backend.widthPx : root.containerWidth
                var heightPx = root.activeMode === modeDynamicEdge ? backend.heightPx : root.containerHeight
                var text = Math.round(widthPx) + " × " + Math.round(heightPx)
                backend.copyTextToClipboardAndQuit(text)
            }

            onWheel: (wheel) => {
                root.adjustActiveModeSliderByWheel(wheel.angleDelta.y)
                wheel.accepted = true
            }
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

        Column {
            anchors.fill: parent
            anchors.topMargin: 8
            anchors.bottomMargin: 8
            anchors.leftMargin: controlsSideMargin
            anchors.rightMargin: controlsSideMargin
            spacing: controlsColumnSpacing

            Row {
                spacing: modeRowSpacing
                anchors.horizontalCenter: parent.horizontalCenter

                Repeater {
                    model: 3

                    Button {
                        id: modeButton
                        width: modeButtonSize
                        height: modeButtonSize
                        checkable: true
                        checked: root.activeMode === index
                        focusPolicy: Qt.StrongFocus

                        onClicked: root.setActiveMode(index)

                        background: Rectangle {
                            radius: modeButtonRadius
                            color: parent.checked ? modeButtonActiveBgColor : modeButtonBgColor
                            border.width: 1
                            border.color: parent.checked ? modeButtonActiveBorderColor : modeButtonBorderColor
                        }

                        contentItem: Canvas {
                            anchors.fill: parent
                            onPaint: {
                                var ctx = getContext("2d")
                                ctx.clearRect(0, 0, width, height)
                                var iconColor = parent.checked ? modeButtonActiveBorderColor : controlsTextColor

                                ctx.strokeStyle = iconColor
                                ctx.fillStyle = iconColor
                                ctx.lineWidth = 1.5

                                if (index === modeDynamicEdge) {
                                    var cx = width / 2
                                    var cy = height / 2
                                    var ray = 9
                                    var cap = 3
                                    ctx.beginPath()
                                    ctx.moveTo(cx - ray, cy)
                                    ctx.lineTo(cx + ray, cy)
                                    ctx.moveTo(cx, cy - ray)
                                    ctx.lineTo(cx, cy + ray)

                                    // End-caps (same idea as the main crosshair)
                                    // West / East tips: vertical caps
                                    ctx.moveTo(cx - ray, cy - cap)
                                    ctx.lineTo(cx - ray, cy + cap)
                                    ctx.moveTo(cx + ray, cy - cap)
                                    ctx.lineTo(cx + ray, cy + cap)
                                    // North / South tips: horizontal caps
                                    ctx.moveTo(cx - cap, cy - ray)
                                    ctx.lineTo(cx + cap, cy - ray)
                                    ctx.moveTo(cx - cap, cy + ray)
                                    ctx.lineTo(cx + cap, cy + ray)
                                    ctx.stroke()
                                } else if (index === modeRectDrag) {
                                    var margin = 7
                                    ctx.strokeRect(margin, margin, width - 2 * margin, height - 2 * margin)
                                    var cornerX = width - margin
                                    var cornerY = height - margin
                                    var cornerSize = 3
                                    ctx.beginPath()
                                    ctx.moveTo(cornerX - cornerSize, cornerY)
                                    ctx.lineTo(cornerX + cornerSize, cornerY)
                                    ctx.moveTo(cornerX, cornerY - cornerSize)
                                    ctx.lineTo(cornerX, cornerY + cornerSize)
                                    ctx.stroke()

                                } else {
                                    ctx.strokeRect(5, 5, width - 10, height - 10)
                                    ctx.strokeRect(9, 9, width - 18, height - 18)
                                }
                            }

                            Connections {
                                target: modeButton
                                function onCheckedChanged() { modeButton.contentItem.requestPaint() }
                            }
                        }

                        ToolTip.visible: hovered
                        ToolTip.text: index === modeDynamicEdge
                                                  ? "Crosshair"
                                      : (index === modeRectDrag
                                                      ? "Drag rectangle"
                                                      : "Container detection")

                    }
                }
            }

            Row {
                spacing: controlsRowSpacing
                anchors.horizontalCenter: parent.horizontalCenter
                opacity: 1.0

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
                        root.applySensitivityValue(value)
                    }
                }

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: Math.round(sensitivitySlider.value)
                    color: controlsTextColor
                    font.pointSize: controlsValuePointSize
                }
            }

            Row {
                spacing: controlsRowSpacing
                anchors.horizontalCenter: parent.horizontalCenter
                visible: root.activeMode === modeRectDrag

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "Snap (px)"
                    color: controlsTextColor
                    font.pointSize: controlsTitlePointSize
                    font.bold: true
                }

                Slider {
                    id: snapDistanceSlider
                    anchors.verticalCenter: parent.verticalCenter
                    from: rectSnapMin
                    to: rectSnapMax
                    stepSize: rectSnapStep
                    width: sensitivitySliderWidth
                    enabled: hasBackend && root.activeMode === modeRectDrag
                    value: rectSnapDistance
                    onMoved: {
                        root.applySnapDistanceValue(value)
                    }
                }

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: Math.round(snapDistanceSlider.value)
                    color: controlsTextColor
                    font.pointSize: controlsValuePointSize
                }
            }
        }

        Connections {
            target: hasBackend ? backend : null
            function onControlsChanged() {
                if (!sensitivitySlider.pressed) {
                    sensitivitySlider.value = backend.sensitivity
                }
                root.refreshSnappedPointer()
                root.refreshContainerSelection()
            }
        }

        Connections {
            target: root
            function onActiveModeChanged() {
                if (!sensitivitySlider.pressed && hasBackend)
                    sensitivitySlider.value = backend.sensitivity
                if (!snapDistanceSlider.pressed)
                    snapDistanceSlider.value = root.rectSnapDistance
                root.refreshSnappedPointer()
                root.refreshContainerSelection()
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
        visible: root.activeMode === modeDynamicEdge

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

    Rectangle {
        id: dragSelectionRect

        x: root.rectLeft
        y: root.rectTop
        width: root.rectWidth
        height: root.rectHeight
        visible: root.activeMode === modeRectDrag && root.rectHasSelection
        color: "transparent"
        border.width: 1
        border.color: crosshairColor
        z: 1
    }

    Rectangle {
        id: containerSelectionRect

        x: root.containerX
        y: root.containerY
        width: root.containerWidth
        height: root.containerHeight
        visible: root.activeMode === modeContainerTrace && root.containerHasSelection
        color: "transparent"
        border.width: 1
        border.color: crosshairColor
        z: 1
    }

    Canvas {
        id: snappedPointerMarker

        x: root.snappedPointerX - 5
        y: root.snappedPointerY - 5
        width: 10
        height: 10
        visible: root.activeMode === modeRectDrag && hasBackend
        z: 2

        Connections {
            target: root
            function onSnappedPointerXChanged() { snappedPointerMarker.requestPaint() }
            function onSnappedPointerYChanged() { snappedPointerMarker.requestPaint() }
            function onSnappedPointerIsSnappedChanged() { snappedPointerMarker.requestPaint() }
            function onActiveModeChanged() { snappedPointerMarker.requestPaint() }
        }

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)

            var color = root.snappedPointerIsSnapped ? crosshairColor : controlsTextColor
            ctx.strokeStyle = color
            ctx.lineWidth = 1

            var cx = 5
            var cy = 5
            ctx.beginPath()
            ctx.moveTo(cx - 5, cy)
            ctx.lineTo(cx + 5, cy)
            ctx.moveTo(cx, cy - 5)
            ctx.lineTo(cx, cy + 5)
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

        x:       root.activeMode === modeRectDrag
                 ? root.rectLeft + labelOffsetX
                      : (root.activeMode === modeContainerTrace
                          ? root.containerX + labelOffsetX
                          : (hasBackend ? backend.cursorX : -labelOffsetX) + labelOffsetX)
        y:       root.activeMode === modeRectDrag
                 ? root.rectTop + labelOffsetY
                      : (root.activeMode === modeContainerTrace
                          ? root.containerY + labelOffsetY
                          : (hasBackend ? backend.cursorY : -labelOffsetY) + labelOffsetY)
        width:   sizeText.implicitWidth + labelHorizontalPadding
        height:  sizeText.implicitHeight + labelVerticalPadding
        color:   labelBackgroundColor
        opacity: labelOpacity
        radius:  labelRadius
        visible: root.activeMode === modeRectDrag
                 ? root.rectHasSelection
                      : (root.activeMode === modeContainerTrace
                          ? root.containerHasSelection
                          : (hasBackend && backend.cursorX >= 0))
        z:       2

        Text {
            id: sizeText
            anchors.centerIn: parent
            text:             root.activeMode === modeRectDrag
                             ? (Math.round(root.rectWidth) + " × " + Math.round(root.rectHeight) + " px")
                                      : (root.activeMode === modeContainerTrace
                                          ? (Math.round(root.containerWidth) + " × " + Math.round(root.containerHeight) + " px")
                             : ((hasBackend ? backend.widthPx : 0)
                                + " × "
                                + (hasBackend ? backend.heightPx : 0)
                                          + " px"))
            color:            labelTextColor
            font.family:      "DejaVu Sans Mono, Consolas, monospace"
            font.pointSize:   13
            font.bold:        true
        }
    }
}
