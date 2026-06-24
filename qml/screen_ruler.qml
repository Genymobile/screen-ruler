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
    readonly property int modeShrinkToFit: 3
    readonly property int modeColorPicker: 4
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
    property bool exportSelectionActive: false
    property bool exportDragActive: false
    property bool exportHasSelection: false
    property real exportStartX: 0
    property real exportStartY: 0
    property real exportEndX: 0
    property real exportEndY: 0

    readonly property real rectLeft: Math.min(rectStartX, rectEndX)
    readonly property real rectTop: Math.min(rectStartY, rectEndY)
    readonly property real rectWidth: Math.abs(rectEndX - rectStartX)
    readonly property real rectHeight: Math.abs(rectEndY - rectStartY)
    readonly property real exportLeft: Math.min(exportStartX, exportEndX)
    readonly property real exportTop: Math.min(exportStartY, exportEndY)
    readonly property real exportWidth: Math.abs(exportEndX - exportStartX)
    readonly property real exportHeight: Math.abs(exportEndY - exportStartY)
    property bool containerHasSelection: false
    property real containerX: 0
    property real containerY: 0
    property real containerWidth: 0
    property real containerHeight: 0
    property real rectSnapDistance: 10
    property bool helpOverlayTargetVisible: false
    property bool startupHelpActive: false
    property real helpOverlayOpacity: 0.0
    readonly property bool helpOverlayVisible: helpOverlayTargetVisible || helpOverlayOpacity > 0.01
    property bool sessionMode: false
    property bool sessionShrinkCommitPending: false
    property int pendingDestructiveKey: -1
    property string pendingDestructiveMessage: ""
    property string sessionActionFeedbackMessage: ""
    property bool suppressNextQuickConfirmClick: false
    property bool sampledColorAvailable: false
    property real sampledColorX: 0
    property real sampledColorY: 0
    property real sampledColorRadius: 0
    property string sampledColorHex: ""
    property string sampledColorRgb: ""
    property string sampledColorHsl: ""
    property int sampledColorR: 0
    property int sampledColorG: 0
    property int sampledColorB: 0
    readonly property int rectSnapMin: 0
    readonly property int rectSnapMax: 30
    readonly property int rectSnapStep: 1
    readonly property int colorSampleRadiusMin: 0
    readonly property int colorSampleRadiusMax: 24
    readonly property int colorSampleRadiusStep: 1
    property real colorSampleRadius: 0
    readonly property int destructiveActionConfirmMs: 1500
    readonly property int sessionActionFeedbackMs: 1200
    readonly property int destructiveSessionToggleButtonKey: -2

    function isRectSelectionMode(mode) {
        return mode === modeRectDrag || mode === modeShrinkToFit
    }

    function setActiveMode(mode) {
        clearPendingDestructiveAction()
        cancelExportSelection()
        var clamped = Math.max(modeDynamicEdge, Math.min(modeColorPicker, mode))
        if (activeMode === clamped)
            return
        activeMode = clamped
        if (!isRectSelectionMode(activeMode)) {
            rectDragActive = false
            rectHasSelection = false
            snappedPointerIsSnapped = false
        } else if (activeMode === modeRectDrag) {
            refreshSnappedPointer()
        }
        if (activeMode !== modeContainerTrace) {
            containerHasSelection = false
        } else {
            refreshContainerSelection()
        }
        if (activeMode !== modeColorPicker) {
            sampledColorAvailable = false
            sampledColorX = 0
            sampledColorY = 0
            sampledColorRadius = 0
            sampledColorHex = ""
            sampledColorRgb = ""
            sampledColorHsl = ""
        } else {
            refreshSampledColor()
        }
    }

    function clearRectSelection() {
        rectDragActive = false
        rectHasSelection = false
        suppressNextQuickConfirmClick = false
        sessionShrinkCommitPending = false
        sessionShrinkCommitTimer.stop()
    }

    function hasValidRectSelection() {
        return isRectSelectionMode(activeMode)
                && rectHasSelection
                && !rectDragActive
                && rectWidth > 2
                && rectHeight > 2
    }

    function hasQuickRectSelectionResult() {
        return !sessionMode && hasValidRectSelection()
    }

    function copyCurrentMeasurementAndQuit() {
        if (!hasBackend)
            return
        backend.copyTextToClipboardAndQuit(activeMeasurementText(false))
    }

    function confirmQuickRectSelectionCopy() {
        if (!hasQuickRectSelectionResult())
            return
        copyCurrentMeasurementAndQuit()
    }

    function scheduleSessionShrinkCommit() {
        sessionShrinkCommitPending = true
        sessionShrinkCommitTimer.restart()
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
        cancelExportSelection()
        clearRectSelection()
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

    function showSessionActionFeedback(message) {
        sessionActionFeedbackMessage = message
        sessionActionFeedbackTimer.restart()
    }

    function pendingMessageForKey(key) {
        if (key === Qt.Key_Tab)
            return "Press Tab again to discard annotations"
        if (key === destructiveSessionToggleButtonKey)
            return "Click SESSION again to discard annotations"
        if (key === Qt.Key_Escape)
            return "Press Esc again to discard annotations"
        if (key === Qt.Key_Q)
            return "Press Q again to discard annotations"
        return ""
    }

    function performDestructiveAction(key) {
        if (key === Qt.Key_Tab || key === destructiveSessionToggleButtonKey) {
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

    function handleSessionToggleButtonAction() {
        if (!sessionMode) {
            clearPendingDestructiveAction()
            setSessionMode(true)
            return
        }
        requestDestructiveAction(destructiveSessionToggleButtonKey)
    }

    function handleEscapeAction() {
        if (exportSelectionActive) {
            cancelExportSelection()
            return
        }
        if (hasQuickRectSelectionResult()) {
            clearRectSelection()
            return
        }
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
        if (activeMode === modeColorPicker)
            refreshSampledColor()
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

    function refreshSampledColor() {
        if (!hasBackend || activeMode !== modeColorPicker) {
            sampledColorAvailable = false
            sampledColorX = 0
            sampledColorY = 0
            sampledColorRadius = 0
            sampledColorHex = ""
            sampledColorRgb = ""
            sampledColorHsl = ""
            sampledColorR = 0
            sampledColorG = 0
            sampledColorB = 0
            return
        }

        var sampled = backend.sampleColorAtPoint(pointerX, pointerY, colorSampleRadius)
        if (!sampled || !sampled.available) {
            sampledColorAvailable = false
            sampledColorX = 0
            sampledColorY = 0
            sampledColorRadius = 0
            sampledColorHex = ""
            sampledColorRgb = ""
            sampledColorHsl = ""
            sampledColorR = 0
            sampledColorG = 0
            sampledColorB = 0
            return
        }
        sampledColorAvailable = true
        sampledColorX = sampled.x
        sampledColorY = sampled.y
        sampledColorRadius = sampled.sampleRadius
        sampledColorHex = sampled.hex
        sampledColorRgb = sampled.rgb
        sampledColorHsl = sampled.hsl
        sampledColorR = sampled.r
        sampledColorG = sampled.g
        sampledColorB = sampled.b
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

    function dragPointerX() {
        return activeMode === modeRectDrag ? snappedPointerX : pointerX
    }

    function dragPointerY() {
        return activeMode === modeRectDrag ? snappedPointerY : pointerY
    }

    function beginRectDrag() {
        sessionShrinkCommitPending = false
        sessionShrinkCommitTimer.stop()
        rectDragActive = true
        rectHasSelection = true
        rectStartX = dragPointerX()
        rectStartY = dragPointerY()
        rectEndX = dragPointerX()
        rectEndY = dragPointerY()
    }

    function updateRectDrag() {
        if (!rectDragActive)
            return
        rectEndX = dragPointerX()
        rectEndY = dragPointerY()
    }

    function endRectDrag() {
        if (!rectDragActive)
            return
        rectEndX = dragPointerX()
        rectEndY = dragPointerY()
        rectDragActive = false
    }

    function applyShrinkToFitOnCurrentRect() {
        if (!hasBackend || activeMode !== modeShrinkToFit || !rectHasSelection)
            return
        var contracted = backend.shrinkRectToContent(rectLeft, rectTop, rectWidth, rectHeight)
        if (!contracted || !contracted.available)
            return
        rectStartX = contracted.x
        rectStartY = contracted.y
        rectEndX = contracted.x + contracted.width
        rectEndY = contracted.y + contracted.height
    }

    function clampSensitivity(value) {
        return Math.max(sensitivityMin, Math.min(sensitivityMax, value))
    }

    function clampRectSnapDistance(value) {
        return Math.max(rectSnapMin, Math.min(rectSnapMax, value))
    }

    function clampColorSampleRadius(value) {
        return Math.max(colorSampleRadiusMin, Math.min(colorSampleRadiusMax, value))
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

    function applyColorSampleRadiusValue(value) {
        colorSampleRadius = clampColorSampleRadius(value)
        refreshSampledColor()
    }

    function adjustSensitivityByWheelSteps(notchSteps) {
        var currentSensitivity = controlsPanel.sensitivitySliderValue
        var nextSensitivity = currentSensitivity + notchSteps * sensitivityStep
        nextSensitivity = Math.max(sensitivityMin, Math.min(sensitivityMax, nextSensitivity))
        if (nextSensitivity === currentSensitivity)
            return
        controlsPanel.sensitivitySliderValue = nextSensitivity
        applySensitivityValue(nextSensitivity)
    }

    function adjustSnapDistanceByWheelSteps(notchSteps) {
        var currentSnap = controlsPanel.snapSliderValue
        var nextSnap = currentSnap + notchSteps * rectSnapStep
        nextSnap = Math.max(rectSnapMin, Math.min(rectSnapMax, nextSnap))
        if (nextSnap === currentSnap)
            return
        controlsPanel.snapSliderValue = nextSnap
        applySnapDistanceValue(nextSnap)
    }

    function adjustColorSampleRadiusByWheelSteps(notchSteps) {
        var currentRadius = controlsPanel.colorSampleSliderValue
        var nextRadius = currentRadius + notchSteps * colorSampleRadiusStep
        nextRadius = Math.max(colorSampleRadiusMin, Math.min(colorSampleRadiusMax, nextRadius))
        if (nextRadius === currentRadius)
            return
        controlsPanel.colorSampleSliderValue = nextRadius
        applyColorSampleRadiusValue(nextRadius)
    }

    function adjustActiveModeSliderByWheel(wheelDeltaY) {
        if (!hasBackend || wheelDeltaY === 0)
            return
        if (
            activeMode !== modeDynamicEdge
            && activeMode !== modeRectDrag
            && activeMode !== modeContainerTrace
            && activeMode !== modeShrinkToFit
            && activeMode !== modeColorPicker
        )
            return

        var notchSteps = wheelDeltaY / 120
        if (notchSteps === 0)
            notchSteps = wheelDeltaY > 0 ? 1 : -1

        if (activeMode === modeRectDrag) {
            adjustSnapDistanceByWheelSteps(notchSteps)
            return
        }

        if (activeMode === modeColorPicker) {
            adjustColorSampleRadiusByWheelSteps(notchSteps)
            return
        }

        adjustSensitivityByWheelSteps(notchSteps)
    }

    function canCopyCurrentMeasurement() {
        return activeMode === modeDynamicEdge
               || hasValidRectSelection()
               || (activeMode === modeContainerTrace && containerHasSelection)
               || (activeMode === modeColorPicker && sampledColorAvailable)
    }

    function canAnnotateCurrentMeasurement() {
        if (isRectSelectionMode(activeMode))
            return hasValidRectSelection()
        if (activeMode === modeContainerTrace)
            return containerHasSelection
        if (activeMode === modeColorPicker)
            return sampledColorAvailable
        return hasBackend && backend.cursorX >= 0
    }

    function canExportCompositeRegion() {
        return hasBackend && exportHasSelection && exportWidth > 2 && exportHeight > 2
    }

    function beginExportSelection() {
        exportSelectionActive = true
        exportDragActive = false
        exportHasSelection = false
    }

    function cancelExportSelection() {
        exportSelectionActive = false
        exportDragActive = false
        exportHasSelection = false
    }

    function handleCompositeExportShortcut() {
        clearPendingDestructiveAction()
        if (!sessionMode || !hasBackend)
            return
        if (exportSelectionActive) {
            cancelExportSelection()
            return
        }
        beginExportSelection()
    }

    function beginExportDrag(x, y) {
        exportDragActive = true
        exportHasSelection = true
        exportStartX = x
        exportStartY = y
        exportEndX = x
        exportEndY = y
    }

    function updateExportDrag(x, y) {
        if (!exportDragActive)
            return
        exportEndX = x
        exportEndY = y
    }

    function finishExportDrag(x, y) {
        if (!exportDragActive)
            return
        exportEndX = x
        exportEndY = y
        exportDragActive = false
    }

    function exportCompositeSelectionToClipboardAndExitMode() {
        if (!canExportCompositeRegion())
            return
        if (backend.copyCompositeRegionToClipboard(exportLeft, exportTop, exportWidth, exportHeight))
            showSessionActionFeedback("Composite image copied to clipboard")
        cancelExportSelection()
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
        } else if (isRectSelectionMode(activeMode)) {
            data = {
                mode: activeMode,
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
        } else if (activeMode === modeColorPicker) {
            data = {
                mode: modeColorPicker,
                x: sampledColorX,
                y: sampledColorY,
                width: 0,
                height: 0,
                text: activeMeasurementText(true),
                cursorX: sampledColorX,
                cursorY: sampledColorY,
                colorHex: sampledColorHex,
                colorRgb: sampledColorRgb,
                colorHsl: sampledColorHsl,
                colorR: sampledColorR,
                colorG: sampledColorG,
                colorB: sampledColorB,
                sampleRadius: sampledColorRadius,
            }
        } else {
            return
        }
        backend.addAnnotation(data)
        if (isRectSelectionMode(activeMode))
            clearRectSelection()
    }

    function showStartupHelpOverlay() {
        startupHelpActive = true
        helpOverlayTargetVisible = true
        helpOverlayOpacity = 1.0
        helpAutoHideTimer.restart()
    }

    function hideHelpOverlay() {
        startupHelpActive = false
        helpAutoHideTimer.stop()
        helpOverlayTargetVisible = false
        helpOverlayOpacity = 0.0
    }

    function dismissStartupHelpOverlay() {
        if (!startupHelpActive || !helpOverlayTargetVisible)
            return
        hideHelpOverlay()
    }

    function toggleHelpOverlay() {
        if (helpOverlayTargetVisible) {
            hideHelpOverlay()
            return
        }
        startupHelpActive = false
        helpAutoHideTimer.stop()
        helpOverlayTargetVisible = true
        helpOverlayOpacity = 1.0
    }

    function dismissStartupHelpFromPointer() {
        if (!startupHelpActive || !helpOverlayTargetVisible)
            return
        hideHelpOverlay()
    }

    function activeMeasurementText(includeUnit) {
        if (activeMode === modeColorPicker) {
            if (!sampledColorAvailable)
                return ""
            return sampledColorHex + " " + sampledColorRgb + " " + sampledColorHsl
        }
        if (isRectSelectionMode(activeMode))
            return Format.roundedPair(rectWidth, rectHeight, includeUnit)
        if (activeMode === modeContainerTrace)
            return Format.roundedPair(containerWidth, containerHeight, includeUnit)
        var widthPx = hasBackend ? backend.widthPx : 0
        var heightPx = hasBackend ? backend.heightPx : 0
        return Format.roundedPair(widthPx, heightPx, includeUnit)
    }

    readonly property real labelAnchorX: isRectSelectionMode(activeMode)
                                                 ? rectLeft
                                                 : (activeMode === modeContainerTrace
                                                     ? containerX
                                                     : (activeMode === modeColorPicker
                                                         ? sampledColorX
                                                         : (hasBackend ? backend.cursorX : 0)))
    readonly property real labelAnchorY: isRectSelectionMode(activeMode)
                                                 ? rectTop
                                                 : (activeMode === modeContainerTrace
                                                     ? containerY
                                                     : (activeMode === modeColorPicker
                                                         ? sampledColorY
                                                         : (hasBackend ? backend.cursorY : 0)))
    readonly property bool labelVisible: isRectSelectionMode(activeMode)
                                                  ? rectHasSelection
                                                  : (activeMode === modeContainerTrace
                                                      ? containerHasSelection
                                                      : (activeMode === modeColorPicker
                                                          ? sampledColorAvailable
                                                          : (hasBackend && backend.cursorX >= 0)))

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
        sequence: "4"
        onActivated: root.setActiveMode(modeShrinkToFit)
    }

    Shortcut {
        sequence: "5"
        onActivated: root.setActiveMode(modeColorPicker)
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
        sequence: "Ctrl+Shift+C"
        onActivated: root.handleCompositeExportShortcut()
    }

    Shortcut {
        sequence: "Enter"
        onActivated: root.confirmQuickRectSelectionCopy()
    }

    Shortcut {
        sequence: "Return"
        onActivated: root.confirmQuickRectSelectionCopy()
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
    // Keyboard handling — Escape cancels export mode or exits session/app, Q exits app.
    // -----------------------------------------------------------------------
    GlobalInputLayer {
        enabled: hasBackend
        activeMode: root.activeMode
        modeRectDrag: root.modeRectDrag
        modeShrinkToFit: root.modeShrinkToFit
        canCopy: root.canCopyCurrentMeasurement()
        sessionMode: root.sessionMode
        quickRectConfirmPending: root.hasQuickRectSelectionResult()

        onPointerPressed: (x, y, button) => {
            root.clearPendingDestructiveAction()
            root.dismissStartupHelpFromPointer()
            root.updatePointer(x, y)
            if (root.exportSelectionActive) {
                if (button === Qt.LeftButton)
                    root.beginExportDrag(x, y)
                return
            }
            if (root.isRectSelectionMode(root.activeMode)
                    && button === Qt.LeftButton
                    && (root.sessionMode || !root.hasQuickRectSelectionResult()))
                root.beginRectDrag()
        }

        onPointerMoved: (x, y) => {
            root.updatePointer(x, y)
            if (root.exportSelectionActive) {
                root.updateExportDrag(x, y)
                return
            }
            if (root.isRectSelectionMode(root.activeMode))
                root.updateRectDrag()
        }

        onPointerReleased: (x, y, button) => {
            root.updatePointer(x, y)
            if (root.exportSelectionActive) {
                if (button === Qt.LeftButton) {
                    root.finishExportDrag(x, y)
                    root.exportCompositeSelectionToClipboardAndExitMode()
                }
                return
            }
            if (root.isRectSelectionMode(root.activeMode) && button === Qt.LeftButton) {
                var hadActiveDrag = root.rectDragActive
                root.endRectDrag()
                if (root.activeMode === modeShrinkToFit)
                    root.applyShrinkToFitOnCurrentRect()
                if (root.sessionMode && root.hasValidRectSelection()) {
                    if (root.activeMode === modeShrinkToFit && hadActiveDrag)
                        root.scheduleSessionShrinkCommit()
                    else
                        root.requestSessionAnnotationPlacement()
                }
                else if (hadActiveDrag && !root.sessionMode && root.hasQuickRectSelectionResult())
                    root.suppressNextQuickConfirmClick = true
            }
        }

        onCopyRequested: {
            root.clearPendingDestructiveAction()
            if (root.exportSelectionActive)
                return
            if (root.sessionMode)
                return
            if (root.hasQuickRectSelectionResult()) {
                if (root.suppressNextQuickConfirmClick) {
                    root.suppressNextQuickConfirmClick = false
                    return
                }
                root.confirmQuickRectSelectionCopy()
                return
            }
            root.copyCurrentMeasurementAndQuit()
        }

        onSessionClickRequested: (x, y) => {
            root.clearPendingDestructiveAction()
            if (root.exportSelectionActive)
                return
            root.updatePointer(x, y)
            // Rect drag commits on mouse release, not via click
            if (root.isRectSelectionMode(root.activeMode))
                return
            if (!root.sessionMode || !root.canAnnotateCurrentMeasurement())
                return
            root.requestSessionAnnotationPlacement()
        }

        onWheelAdjusted: (deltaY) => {
            root.clearPendingDestructiveAction()
            if (root.exportSelectionActive)
                return
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
        modeColorPicker: root.modeColorPicker
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
        colorSampleRadiusMin: root.colorSampleRadiusMin
        colorSampleRadiusMax: root.colorSampleRadiusMax
        colorSampleRadiusStep: root.colorSampleRadiusStep
        colorSampleRadius: root.colorSampleRadius

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
        onColorSampleRadiusMoved: (value) => {
            root.clearPendingDestructiveAction()
            root.dismissStartupHelpOverlay()
            root.applyColorSampleRadiusValue(value)
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
            if (!controlsPanel.colorSampleSliderPressed)
                controlsPanel.colorSampleSliderValue = root.colorSampleRadius
            root.refreshSnappedPointer()
            root.refreshContainerSelection()
            root.refreshSampledColor()
        }
    }

    OverlayActionButton {
        id: helpToggleButton
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.topMargin: RulerTheme.baseMargin
        anchors.rightMargin: RulerTheme.baseMargin
        implicitWidth: RulerTheme.modeButtonSize
        implicitHeight: RulerTheme.modeButtonSize
        cornerRadius: width / 2
        labelText: "?"
        tooltipText: "Toggle shortcut help"
        isActive: root.helpOverlayTargetVisible
        activeBgColor: Qt.rgba(0.90, 0.10, 0.37, 0.32)
        labelColor: isActive ? RulerTheme.accentColor : RulerTheme.primaryTextColor
        z: 40
        onClicked: {
            root.clearPendingDestructiveAction()
            root.dismissStartupHelpOverlay()
            root.toggleHelpOverlay()
        }
    }

    ShortcutHelpOverlay {
        id: helpOverlay
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.topMargin: RulerTheme.baseMargin + helpToggleButton.height + 8
        anchors.rightMargin: RulerTheme.baseMargin
        overlayVisible: root.helpOverlayVisible
        overlayOpacity: root.helpOverlayOpacity
        sessionMode: root.sessionMode
    }

    OverlayActionButton {
        id: sessionModeToggleButton
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.leftMargin: RulerTheme.baseMargin
        anchors.topMargin: RulerTheme.baseMargin
        implicitWidth: 96
        implicitHeight: 30
        labelText: "SESSION"
        tooltipText: root.sessionMode ? "Exit session mode" : "Enter session mode"
        isActive: root.sessionMode
        z: 40
        onClicked: {
            root.dismissStartupHelpOverlay()
            root.handleSessionToggleButtonAction()
        }
    }

    Row {
        anchors.left: sessionModeToggleButton.right
        anchors.leftMargin: 8
        anchors.verticalCenter: sessionModeToggleButton.verticalCenter
        visible: root.sessionMode
        spacing: 6
        z: 40

        OverlayActionButton {
            implicitWidth: 46
            implicitHeight: sessionModeToggleButton.height
            labelText: "MD"
            tooltipText: "Copy annotations as Markdown"
            isActive: false
            onClicked: {
                root.clearPendingDestructiveAction()
                if (root.sessionMode && hasBackend) {
                    backend.copyAnnotationsMarkdownToClipboard()
                    root.showSessionActionFeedback("Markdown copied to clipboard")
                }
            }
        }

        OverlayActionButton {
            implicitWidth: 52
            implicitHeight: sessionModeToggleButton.height
            labelText: "IMG"
            tooltipText: "Copy annotations as image"
            isActive: root.exportSelectionActive
            onClicked: root.handleCompositeExportShortcut()
        }
    }

    OverlayMessageBubble {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        anchors.verticalCenterOffset: root.pendingDestructiveMessage !== "" ? 44 : 0
        bubbleVisible: root.sessionMode && root.sessionActionFeedbackMessage !== ""
        z: 45
        maxWidth: parent.width - 80
        textPointSize: RulerTheme.controlsTitlePointSize + 2
        textValue: root.sessionActionFeedbackMessage
    }

    OverlayMessageBubble {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        bubbleVisible: root.sessionMode && root.pendingDestructiveMessage !== ""
        z: 45
        maxWidth: parent.width - 80
        textPointSize: RulerTheme.controlsTitlePointSize + 4
        textValue: root.pendingDestructiveMessage
    }

    OverlayMessageBubble {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.topMargin: RulerTheme.baseMargin + 44
        bubbleVisible: root.exportSelectionActive
        z: 45
        maxWidth: parent.width - 80
        textPointSize: RulerTheme.controlsValuePointSize + 2
        verticalPadding: 9
        textValue: "Composite export: drag a rectangle to copy image to clipboard"
    }

    OverlayMessageBubble {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        bubbleVisible: root.hasQuickRectSelectionResult()
        z: 45
        maxWidth: parent.width - 80
        textPointSize: RulerTheme.controlsValuePointSize + 2
        verticalPadding: 9
        textValue: "Selection ready: click or press Enter to copy to clipboard and exit"
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
        id: sessionActionFeedbackTimer
        interval: root.sessionActionFeedbackMs
        repeat: false
        onTriggered: root.sessionActionFeedbackMessage = ""
    }

    Timer {
        id: sessionShrinkCommitTimer
        interval: RulerTheme.selectionTransitionMs
        repeat: false
        onTriggered: {
            if (!root.sessionShrinkCommitPending)
                return
            root.sessionShrinkCommitPending = false
            if (root.sessionMode
                    && root.activeMode === modeShrinkToFit
                    && root.hasValidRectSelection()) {
                root.requestSessionAnnotationPlacement()
            }
        }
    }

    Timer {
        id: helpAutoHideTimer
        interval: RulerTheme.helpOverlayAutoHideMs
        repeat: false
        onTriggered: root.hideHelpOverlay()
    }

    Behavior on helpOverlayOpacity {
        NumberAnimation {
            duration: RulerTheme.helpOverlayFadeMs
            easing.type: Easing.OutQuad
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
        visible: root.activeMode === modeDynamicEdge && !root.exportSelectionActive
        backend: root.backend
    }

    SelectionOutline {
        id: dragSelectionRect
        x: root.rectLeft
        y: root.rectTop
        width: root.rectWidth
        height: root.rectHeight
        animateGeometry: root.activeMode === modeShrinkToFit && !root.rectDragActive
        visible: root.isRectSelectionMode(root.activeMode) && root.rectHasSelection && !root.exportSelectionActive
        z: 1
    }

    SelectionOutline {
        id: containerSelectionRect
        x: root.containerX
        y: root.containerY
        width: root.containerWidth
        height: root.containerHeight
        animateGeometry: true
        visible: root.activeMode === modeContainerTrace && root.containerHasSelection && !root.exportSelectionActive
        z: 1
    }

    SelectionOutline {
        id: exportSelectionRect
        x: root.exportLeft
        y: root.exportTop
        width: root.exportWidth
        height: root.exportHeight
        animateGeometry: false
        visible: root.exportSelectionActive && root.exportHasSelection
        z: 9
    }

    SnappedPointerMarker {
        markerX: root.snappedPointerX
        markerY: root.snappedPointerY
        visible: root.activeMode === modeRectDrag && hasBackend && !root.exportSelectionActive
        isSnapped: root.snappedPointerIsSnapped
    }

    ColorSampleMarker {
        markerX: root.sampledColorX
        markerY: root.sampledColorY
        sampleRadius: root.sampledColorRadius
        visible: root.activeMode === modeColorPicker && root.sampledColorAvailable && !root.exportSelectionActive
        z: 3
    }

    ColorSampleBubble {
        anchorX: root.sampledColorX
        anchorY: root.sampledColorY
        swatchColor: Qt.rgba(
                         Math.max(0, Math.min(255, root.sampledColorR)) / 255.0,
                         Math.max(0, Math.min(255, root.sampledColorG)) / 255.0,
                         Math.max(0, Math.min(255, root.sampledColorB)) / 255.0,
                         1.0
                     )
        hexText: root.sampledColorHex
        rgbText: root.sampledColorRgb
        hslText: root.sampledColorHsl
        visible: root.activeMode === modeColorPicker && root.sampledColorAvailable && !root.exportSelectionActive
        z: 4
    }

    // -----------------------------------------------------------------------
    // Measurement label — semi-transparent box, positioned near cursor
    // -----------------------------------------------------------------------
    MeasurementLabel {
        anchorX: root.labelAnchorX
        anchorY: root.labelAnchorY
        visible: root.labelVisible
                 && root.activeMode !== modeColorPicker
                 && !root.exportSelectionActive
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
