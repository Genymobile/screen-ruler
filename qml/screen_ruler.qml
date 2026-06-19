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
import "screen_ruler_format.js" as Format

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
    property bool helpOverlayVisible: false
    property bool startupHelpActive: false
    property real helpOverlayOpacity: 0.0
    property bool sessionMode: false
    property int pendingDestructiveKey: -1
    property string pendingDestructiveMessage: ""
    readonly property int rectSnapMin: 0
    readonly property int rectSnapMax: 30
    readonly property int rectSnapStep: 1
    readonly property int destructiveActionConfirmMs: 1500

    function setActiveMode(mode) {
        clearPendingDestructiveAction()
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

    readonly property real sessionBorderProximity: Math.max(
        0,
        Math.min(pointerX, pointerY, width - pointerX, height - pointerY)
    )
    readonly property real sessionBorderOpacity: {
        if (!sessionMode)
            return 0
        var half = RulerTheme.sessionModeBorderFadeDistance / 2
        if (sessionBorderProximity >= RulerTheme.sessionModeBorderFadeDistance)
            return RulerTheme.sessionModeBorderBaseOpacity
        if (sessionBorderProximity <= half)
            return RulerTheme.sessionModeBorderNearOpacity
        var t = (sessionBorderProximity - half) / half
        return RulerTheme.sessionModeBorderNearOpacity
                + t * (RulerTheme.sessionModeBorderBaseOpacity - RulerTheme.sessionModeBorderNearOpacity)
    }

    function setSessionMode(enabled) {
        if (sessionMode === enabled)
            return
        clearPendingDestructiveAction()
        sessionMode = enabled
        if (!enabled && hasBackend)
            backend.clearAnnotations()
    }

    function hasPersistentSessionAnnotations() {
        return sessionMode && hasBackend && backend.annotationCount > 0
    }

    function clearPendingDestructiveAction() {
        pendingDestructiveKey = -1
        pendingDestructiveMessage = ""
        destructiveActionTimer.stop()
    }

    function pendingMessageForKey(key) {
        if (key === Qt.Key_Tab)
            return "Press Tab again to discard annotations"
        if (key === Qt.Key_Escape)
            return "Press Esc again to discard annotations"
        if (key === Qt.Key_Q)
            return "Press Q again to discard annotations"
        return ""
    }

    function performDestructiveAction(key) {
        if (key === Qt.Key_Tab) {
            setSessionMode(false)
            return
        }
        if (key === Qt.Key_Escape) {
            if (sessionMode) {
                setSessionMode(false)
                return
            }
            Qt.quit()
            return
        }
        if (key === Qt.Key_Q)
            Qt.quit()
    }

    function requestDestructiveAction(key) {
        if (!hasPersistentSessionAnnotations()) {
            clearPendingDestructiveAction()
            performDestructiveAction(key)
            return
        }

        if (pendingDestructiveKey === key) {
            clearPendingDestructiveAction()
            performDestructiveAction(key)
            return
        }

        pendingDestructiveKey = key
        pendingDestructiveMessage = pendingMessageForKey(key)
        destructiveActionTimer.restart()
    }

    function handleTabAction() {
        if (!sessionMode) {
            clearPendingDestructiveAction()
            setSessionMode(true)
            return
        }
        requestDestructiveAction(Qt.Key_Tab)
    }

    function handleEscapeAction() {
        if (sessionMode) {
            requestDestructiveAction(Qt.Key_Escape)
            return
        }
        clearPendingDestructiveAction()
        Qt.quit()
    }

    function handleQuitAction() {
        requestDestructiveAction(Qt.Key_Q)
    }

    function updatePointer(x, y) {
        pointerX = x
        pointerY = y
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
        var nextSensitivity = sensitivityRow.sliderValue + notchSteps * sensitivityStep
        nextSensitivity = Math.max(sensitivityMin, Math.min(sensitivityMax, nextSensitivity))
        if (nextSensitivity === sensitivityRow.sliderValue)
            return
        sensitivityRow.sliderValue = nextSensitivity
        applySensitivityValue(nextSensitivity)
    }

    function adjustSnapDistanceByWheelSteps(notchSteps) {
        var nextSnap = snapRow.sliderValue + notchSteps * rectSnapStep
        nextSnap = Math.max(rectSnapMin, Math.min(rectSnapMax, nextSnap))
        if (nextSnap === snapRow.sliderValue)
            return
        snapRow.sliderValue = nextSnap
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

    function canCopyCurrentMeasurement() {
        return activeMode === modeDynamicEdge
               || (activeMode === modeContainerTrace && containerHasSelection)
    }

    function canAnnotateCurrentMeasurement() {
        if (activeMode === modeRectDrag)
            return rectHasSelection
        if (activeMode === modeContainerTrace)
            return containerHasSelection
        return hasBackend && backend.cursorX >= 0
    }

    function requestSessionAnnotationPlacement() {
        if (!hasBackend)
            return
        var data = {}
        if (activeMode === modeDynamicEdge) {
            data = {
                mode: modeDynamicEdge,
                x: backend.cursorX,
                y: backend.cursorY,
                width: backend.widthPx,
                height: backend.heightPx,
                text: activeMeasurementText(true),
                cursorX: backend.cursorX,
                cursorY: backend.cursorY,
                northEnd: backend.northEnd,
                southEnd: backend.southEnd,
                westEnd: backend.westEnd,
                eastEnd: backend.eastEnd,
            }
        } else if (activeMode === modeRectDrag) {
            data = {
                mode: modeRectDrag,
                x: rectLeft,
                y: rectTop,
                width: rectWidth,
                height: rectHeight,
                text: activeMeasurementText(true),
                cursorX: rectLeft,
                cursorY: rectTop,
                northEnd: rectTop,
                southEnd: rectTop + rectHeight,
                westEnd: rectLeft,
                eastEnd: rectLeft + rectWidth,
            }
        } else if (activeMode === modeContainerTrace) {
            data = {
                mode: modeContainerTrace,
                x: containerX,
                y: containerY,
                width: containerWidth,
                height: containerHeight,
                text: activeMeasurementText(true),
                cursorX: containerX,
                cursorY: containerY,
                northEnd: containerY,
                southEnd: containerY + containerHeight,
                westEnd: containerX,
                eastEnd: containerX + containerWidth,
            }
        } else {
            return
        }
        backend.addAnnotation(data)
    }

    function showStartupHelpOverlay() {
        startupHelpActive = true
        helpOverlayVisible = true
        helpOverlayOpacity = 1.0
        helpFadeAnimation.stop()
        helpAutoHideTimer.restart()
    }

    function hideHelpOverlay() {
        startupHelpActive = false
        helpAutoHideTimer.stop()
        helpFadeAnimation.stop()
        helpOverlayVisible = false
        helpOverlayOpacity = 0.0
    }

    function dismissStartupHelpOverlay() {
        if (!startupHelpActive || !helpOverlayVisible)
            return
        hideHelpOverlay()
    }

    function toggleHelpOverlay() {
        if (helpOverlayVisible) {
            hideHelpOverlay()
            return
        }
        startupHelpActive = false
        helpOverlayVisible = true
        helpOverlayOpacity = 1.0
        helpFadeAnimation.stop()
    }

    function dismissStartupHelpFromPointer() {
        if (!startupHelpActive || !helpOverlayVisible)
            return
        hideHelpOverlay()
    }

    function activeMeasurementText(includeUnit) {
        if (activeMode === modeRectDrag)
            return Format.roundedPair(rectWidth, rectHeight, includeUnit)
        if (activeMode === modeContainerTrace)
            return Format.roundedPair(containerWidth, containerHeight, includeUnit)
        var widthPx = hasBackend ? backend.widthPx : 0
        var heightPx = hasBackend ? backend.heightPx : 0
        return Format.roundedPair(widthPx, heightPx, includeUnit)
    }

    readonly property real labelX: activeMode === modeRectDrag
                                            ? rectLeft + RulerTheme.baseMargin
                                            : (activeMode === modeContainerTrace
                                                ? containerX + RulerTheme.baseMargin
                                                : (hasBackend ? backend.cursorX : -RulerTheme.baseMargin) + RulerTheme.baseMargin)
    readonly property real labelY: activeMode === modeRectDrag
                                            ? rectTop + RulerTheme.labelOffsetY
                                            : (activeMode === modeContainerTrace
                                                ? containerY + RulerTheme.labelOffsetY
                                                : (hasBackend ? backend.cursorY : -RulerTheme.labelOffsetY) + RulerTheme.labelOffsetY)
    readonly property bool labelVisible: activeMode === modeRectDrag
                                                  ? rectHasSelection
                                                  : (activeMode === modeContainerTrace
                                                      ? containerHasSelection
                                                      : (hasBackend && backend.cursorX >= 0))

    readonly property int edgePreviewHoldMs: 1000
    readonly property int edgePreviewFadeMs: 1000
    readonly property real edgePreviewPeakOpacity: 0.5
    property real edgePreviewOpacity: 0.0

    readonly property int sensitivityMin: 0
    readonly property int sensitivityMax: 100
    readonly property int sensitivityStep: 1
    readonly property int sensitivityDefaultValue: RulerTheme.sensitivityDefaultValue

    // Position and size are driven by the virtual desktop bounds that Python
    // computed from the union of all screen geometries.
    x:      hasBackend && backend.isWaylandSession ? 0 : (hasBackend ? backend.virtualDesktopX : 0)
    y:      hasBackend && backend.isWaylandSession ? 0 : (hasBackend ? backend.virtualDesktopY : 0)
    width:  hasBackend && backend.isWaylandSession ? Screen.width : (hasBackend ? backend.virtualDesktopWidth : Screen.width)
    height: hasBackend && backend.isWaylandSession ? Screen.height : (hasBackend ? backend.virtualDesktopHeight : Screen.height)

    // Frameless, always-on-top, transparent overlay.
    // On X11 use bypass hint to avoid WM work-area insets (panels/docks).
    // Q reliability is preserved by an app-level key filter in Python.
    flags:  hasBackend && backend.isWaylandSession
            ? (Qt.WindowStaysOnTopHint | Qt.FramelessWindowHint)
            : (Qt.WindowStaysOnTopHint
             | Qt.FramelessWindowHint
             | Qt.X11BypassWindowManagerHint)
    color:   "transparent"
    visibility: hasBackend && backend.isWaylandSession ? Window.FullScreen : Window.Windowed
    title:   "Screen Ruler"

    Component.onCompleted: {
        requestActivate()
        keyInputLayer.forceActiveFocus()
        showStartupHelpOverlay()
    }

    Shortcut {
        sequence: "Escape"
        onActivated: root.handleEscapeAction()
    }

    Shortcut {
        sequence: "Q"
        onActivated: root.handleQuitAction()
    }

    Shortcut {
        sequence: "1"
        onActivated: root.setActiveMode(modeDynamicEdge)
    }

    Shortcut {
        sequence: "2"
        onActivated: root.setActiveMode(modeRectDrag)
    }

    Shortcut {
        sequence: "3"
        onActivated: root.setActiveMode(modeContainerTrace)
    }

    Shortcut {
        sequence: "Ctrl+C"
        onActivated: {
            root.clearPendingDestructiveAction()
            if (!hasBackend)
                return
            if (root.sessionMode) {
                backend.copyAnnotationsMarkdownToClipboard()
                return
            }
            if (!root.canCopyCurrentMeasurement())
                return
            backend.copyTextToClipboardAndQuit(root.activeMeasurementText(false))
        }
    }

    Shortcut {
        sequence: "Tab"
        onActivated: root.handleTabAction()
    }

    Shortcut {
        sequence: "Ctrl+Z"
        onActivated: {
            root.clearPendingDestructiveAction()
            if (root.sessionMode && hasBackend)
                backend.removeLastAnnotation()
        }
    }

    Shortcut {
        sequence: "Z"
        onActivated: {
            root.clearPendingDestructiveAction()
            if (root.sessionMode && hasBackend)
                backend.removeLastAnnotation()
        }
    }

    Shortcut {
        sequence: "Ctrl+Shift+Z"
        onActivated: {
            root.clearPendingDestructiveAction()
            if (root.sessionMode && hasBackend)
                backend.redoAnnotation()
        }
    }

    Shortcut {
        sequence: "?"
        onActivated: {
            root.clearPendingDestructiveAction()
            root.toggleHelpOverlay()
        }
    }

    Shortcut {
        sequence: "H"
        onActivated: {
            root.clearPendingDestructiveAction()
            root.toggleHelpOverlay()
        }
    }

    Item {
        id: keyInputLayer
        anchors.fill: parent
        focus: true

        Keys.onPressed: (event) => {
            root.dismissStartupHelpOverlay()
            if (event.key === Qt.Key_Escape || event.key === Qt.Key_Q) {
                event.accepted = true
                if (event.key === Qt.Key_Escape)
                    root.handleEscapeAction()
                else
                    root.handleQuitAction()
            }
        }
    }

    // -----------------------------------------------------------------------
    // Keyboard handling — Escape exits session mode (or app), Q exits app.
    // -----------------------------------------------------------------------
    GlobalInputLayer {
        enabled: hasBackend
        activeMode: root.activeMode
        modeRectDrag: root.modeRectDrag
        canCopy: root.canCopyCurrentMeasurement()
        sessionMode: root.sessionMode

        onPointerPressed: (x, y, button) => {
            root.clearPendingDestructiveAction()
            root.dismissStartupHelpFromPointer()
            root.updatePointer(x, y)
            if (root.activeMode === modeRectDrag && button === Qt.LeftButton)
                root.beginRectDrag()
        }

        onPointerMoved: (x, y) => {
            root.updatePointer(x, y)
            if (root.activeMode === modeRectDrag)
                root.updateRectDrag()
        }

        onPointerReleased: (x, y, button) => {
            root.updatePointer(x, y)
            if (root.activeMode === modeRectDrag && button === Qt.LeftButton) {
                root.endRectDrag()
                if (root.sessionMode && root.rectHasSelection
                        && root.rectWidth > 2 && root.rectHeight > 2)
                    root.requestSessionAnnotationPlacement()
            }
        }

        onCopyRequested: {
            root.clearPendingDestructiveAction()
            if (root.sessionMode)
                return
            if (!hasBackend)
                return
            backend.copyTextToClipboardAndQuit(root.activeMeasurementText(false))
        }

        onSessionClickRequested: (x, y) => {
            root.clearPendingDestructiveAction()
            root.updatePointer(x, y)
            // Rect drag commits on mouse release, not via click
            if (root.activeMode === modeRectDrag)
                return
            if (!root.sessionMode || !root.canAnnotateCurrentMeasurement())
                return
            root.requestSessionAnnotationPlacement()
        }

        onWheelAdjusted: (deltaY) => {
            root.clearPendingDestructiveAction()
            root.adjustActiveModeSliderByWheel(deltaY)
        }
    }

    ControlsPanel {
        id: controlsPanel
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.topMargin: RulerTheme.baseMargin
        activeMode: root.activeMode
        modeRectDrag: root.modeRectDrag
        hasBackend: root.hasBackend
        sensitivityMin: root.sensitivityMin
        sensitivityMax: root.sensitivityMax
        sensitivityStep: root.sensitivityStep
        sensitivityDefaultValue: root.sensitivityDefaultValue
        sensitivityValue: hasBackend ? backend.sensitivity : root.sensitivityDefaultValue
        rectSnapMin: root.rectSnapMin
        rectSnapMax: root.rectSnapMax
        rectSnapStep: root.rectSnapStep
        rectSnapDistance: root.rectSnapDistance

        onPanelPressed: { root.clearPendingDestructiveAction(); root.dismissStartupHelpOverlay() }
        onModeSelected: (mode) => { root.dismissStartupHelpOverlay(); root.setActiveMode(mode) }
        onSensitivityMoved: (value) => {
            root.clearPendingDestructiveAction()
            root.dismissStartupHelpOverlay()
            root.applySensitivityValue(value)
        }
        onSnapMoved: (value) => {
            root.clearPendingDestructiveAction()
            root.dismissStartupHelpOverlay()
            root.applySnapDistanceValue(value)
        }
    }

    Connections {
        target: hasBackend ? backend : null
        function onControlsChanged() {
            if (!controlsPanel.sensitivitySliderPressed) {
                controlsPanel.sensitivitySliderValue = backend.sensitivity
            }
            root.refreshSnappedPointer()
            root.refreshContainerSelection()
        }
    }

    Connections {
        target: root
        function onActiveModeChanged() {
            if (!controlsPanel.sensitivitySliderPressed && hasBackend)
                controlsPanel.sensitivitySliderValue = backend.sensitivity
            if (!controlsPanel.snapSliderPressed)
                controlsPanel.snapSliderValue = root.rectSnapDistance
            root.refreshSnappedPointer()
            root.refreshContainerSelection()
        }
    }

    ShortcutHelpOverlay {
        id: helpOverlay
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.topMargin: RulerTheme.baseMargin
        anchors.rightMargin: RulerTheme.baseMargin
        overlayVisible: root.helpOverlayVisible
        overlayOpacity: root.helpOverlayOpacity
        sessionMode: root.sessionMode
    }

    Rectangle {
        id: sessionModeBadge
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.leftMargin: RulerTheme.baseMargin
        anchors.topMargin: RulerTheme.baseMargin
        visible: root.sessionMode
        z: 40
        radius: RulerTheme.cornerRadius
        color: RulerTheme.sessionModeBadgeColor
        opacity: RulerTheme.sessionModeBadgeOpacity
        width: sessionModeLabel.implicitWidth + RulerTheme.sessionModeBadgeHorizontalPadding
        height: sessionModeLabel.implicitHeight + RulerTheme.sessionModeBadgeVerticalPadding

        Text {
            id: sessionModeLabel
            anchors.centerIn: parent
            text: "SESSION"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
            font.bold: true
        }
    }

    Rectangle {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        visible: root.sessionMode && root.pendingDestructiveMessage !== ""
        z: 45
        radius: RulerTheme.cornerRadius + 2
        color: RulerTheme.panelBackgroundColor
        opacity: 0.95
        border.width: 2
        border.color: RulerTheme.sessionModeBorderColor
        width: Math.min(parent.width - 80, destructiveHintLabel.implicitWidth + 48)
        height: destructiveHintLabel.implicitHeight + 28

        Text {
            id: destructiveHintLabel
            anchors.centerIn: parent
            text: root.pendingDestructiveMessage
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsTitlePointSize + 4
            font.bold: true
        }
    }

    Rectangle {
        visible: root.sessionMode
        z: 35
        anchors.fill: parent
        color: "transparent"
        border.width: RulerTheme.sessionModeBorderThickness
        border.color: RulerTheme.sessionModeBorderColor
        opacity: root.sessionBorderOpacity
    }

    Timer {
        id: destructiveActionTimer
        interval: root.destructiveActionConfirmMs
        repeat: false
        onTriggered: root.clearPendingDestructiveAction()
    }

    Timer {
        id: helpAutoHideTimer
        interval: RulerTheme.helpOverlayAutoHideMs
        repeat: false
        onTriggered: helpFadeAnimation.restart()
    }

    NumberAnimation {
        id: helpFadeAnimation
        target: root
        property: "helpOverlayOpacity"
        to: 0.0
        duration: RulerTheme.helpOverlayFadeMs
        easing.type: Easing.OutQuad
        onFinished: {
            if (root.helpOverlayOpacity <= 0.01) {
                root.helpOverlayVisible = false
                root.startupHelpActive = false
            }
        }
    }

    // -----------------------------------------------------------------------
    // Captured screenshot background (always shown)
    // -----------------------------------------------------------------------
    OverlayImageLayer {
        visible: hasBackend && backend.screenshotAvailable
        source: hasBackend ? backend.screenshotSource : ""
        opacity: 1.0
        z: -2
    }

    // -----------------------------------------------------------------------
    // Optional debug overlay — displays the captured Canny edge map
    // -----------------------------------------------------------------------
    OverlayImageLayer {
         visible: hasBackend
               && backend.debugOverlaySource !== ""
               && (backend.debugOverlayEnabled || edgePreviewOpacity > 0)
        source: hasBackend ? backend.debugOverlaySource : ""
        opacity: hasBackend && backend.debugOverlayEnabled
                 ? Math.max(RulerTheme.debugOverlayOpacity, edgePreviewOpacity)
                 : edgePreviewOpacity
        z: -1
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
    CrosshairCanvas {
        visible: root.activeMode === modeDynamicEdge
        backend: root.backend
    }

    SelectionOutline {
        id: dragSelectionRect
        x: root.rectLeft
        y: root.rectTop
        width: root.rectWidth
        height: root.rectHeight
        visible: root.activeMode === modeRectDrag && root.rectHasSelection
        z: 1
    }

    SelectionOutline {
        id: containerSelectionRect
        x: root.containerX
        y: root.containerY
        width: root.containerWidth
        height: root.containerHeight
        visible: root.activeMode === modeContainerTrace && root.containerHasSelection
        z: 1
    }

    SnappedPointerMarker {
        markerX: root.snappedPointerX
        markerY: root.snappedPointerY
        visible: root.activeMode === modeRectDrag && hasBackend
        isSnapped: root.snappedPointerIsSnapped
    }

    // -----------------------------------------------------------------------
    // Measurement label — semi-transparent box, positioned near cursor
    // -----------------------------------------------------------------------
    MeasurementLabel {
        labelX: root.labelX
        labelY: root.labelY
        visible: root.labelVisible
        textValue: root.activeMeasurementText(true)
    }

    // -----------------------------------------------------------------------
    // Persistent session annotations
    // -----------------------------------------------------------------------
    Repeater {
        model: hasBackend ? backend.annotations : []
        delegate: AnnotationItem {
            z: 5
        }
    }
}
