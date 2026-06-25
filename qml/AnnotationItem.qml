// AnnotationItem.qml
//
// Renders one frozen annotation from the session annotation model.

import QtQuick

Item {
    id: root

    required property int mode
    required property real annotationX
    required property real annotationY
    required property real annotationWidth
    required property real annotationHeight
    required property string text
    required property real cursorX
    required property real cursorY
    required property real northEnd
    required property real southEnd
    required property real westEnd
    required property real eastEnd
    required property string colorHex
    required property string colorRgb
    required property string colorHsl
    required property real sampleRadius
    anchors.fill: parent

    // -----------------------------------------------------------------------
    // Crosshair — dynamic edge mode (mode 0)
    // -----------------------------------------------------------------------
    Canvas {
        anchors.fill: parent
        visible: root.mode === 0

        Component.onCompleted: requestPaint()
        onVisibleChanged: requestPaint()

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)

            var cx = root.cursorX
            var cy = root.cursorY

            ctx.strokeStyle = RulerTheme.accentColor
            ctx.lineWidth = 1
            ctx.beginPath()
            ctx.moveTo(cx, root.northEnd)
            ctx.lineTo(cx, root.southEnd)
            ctx.moveTo(root.westEnd, cy)
            ctx.lineTo(root.eastEnd, cy)

            var t = 5
            ctx.moveTo(cx - t, root.northEnd)
            ctx.lineTo(cx + t, root.northEnd)
            ctx.moveTo(cx - t, root.southEnd)
            ctx.lineTo(cx + t, root.southEnd)
            ctx.moveTo(root.westEnd, cy - t)
            ctx.lineTo(root.westEnd, cy + t)
            ctx.moveTo(root.eastEnd, cy - t)
            ctx.lineTo(root.eastEnd, cy + t)
            ctx.stroke()
        }
    }

    // -----------------------------------------------------------------------
    // Selection outline — rectangle-like modes (mode 1/2/3)
    // -----------------------------------------------------------------------
    SelectionOutline {
        x: root.annotationX
        y: root.annotationY
        width: root.annotationWidth
        height: root.annotationHeight
        visible: root.mode === 1 || root.mode === 2 || root.mode === 3
    }

    ColorSampleMarker {
        markerX: root.annotationX
        markerY: root.annotationY
        sampleRadius: root.sampleRadius
        visible: root.mode === 4
    }

    ColorSampleBubble {
        anchorX: root.annotationX
        anchorY: root.annotationY
        swatchColor: root.colorHex
        hexText: root.colorHex
        rgbText: root.colorRgb
        hslText: root.colorHsl
        visible: root.mode === 4
    }

    // -----------------------------------------------------------------------
    // Measurement label — all modes
    // -----------------------------------------------------------------------
    MeasurementLabel {
        anchorX: root.mode === 0 ? root.cursorX : root.annotationX
        anchorY: root.mode === 0 ? root.cursorY : root.annotationY
        textValue: root.text
        visible: root.mode !== 4
    }
}
