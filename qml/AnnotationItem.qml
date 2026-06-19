// AnnotationItem.qml
//
// Renders one frozen annotation from the session annotation list.
// modelData is a dict with: mode, x, y, width, height, text,
// and (for mode 0) cursorX, cursorY, northEnd, southEnd, westEnd, eastEnd.

import QtQuick

Item {
    required property var modelData

    anchors.fill: parent

    // -----------------------------------------------------------------------
    // Crosshair — dynamic edge mode (mode 0)
    // -----------------------------------------------------------------------
    Canvas {
        anchors.fill: parent
        visible: modelData.mode === 0

        Component.onCompleted: requestPaint()

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)

            var cx = modelData.cursorX
            var cy = modelData.cursorY

            ctx.strokeStyle = RulerTheme.accentColor
            ctx.lineWidth = 1
            ctx.beginPath()
            ctx.moveTo(cx, modelData.northEnd)
            ctx.lineTo(cx, modelData.southEnd)
            ctx.moveTo(modelData.westEnd, cy)
            ctx.lineTo(modelData.eastEnd, cy)

            var t = 5
            ctx.moveTo(cx - t, modelData.northEnd)
            ctx.lineTo(cx + t, modelData.northEnd)
            ctx.moveTo(cx - t, modelData.southEnd)
            ctx.lineTo(cx + t, modelData.southEnd)
            ctx.moveTo(modelData.westEnd, cy - t)
            ctx.lineTo(modelData.westEnd, cy + t)
            ctx.moveTo(modelData.eastEnd, cy - t)
            ctx.lineTo(modelData.eastEnd, cy + t)
            ctx.stroke()
        }
    }

    // -----------------------------------------------------------------------
    // Selection outline — rect drag / container trace modes (mode 1 and 2)
    // -----------------------------------------------------------------------
    SelectionOutline {
        x: modelData.x
        y: modelData.y
        width: modelData.width
        height: modelData.height
        visible: modelData.mode !== 0
    }

    // -----------------------------------------------------------------------
    // Measurement label — all modes
    // -----------------------------------------------------------------------
    MeasurementLabel {
        labelX: (modelData.mode === 0 ? modelData.cursorX : modelData.x) + RulerTheme.baseMargin
        labelY: (modelData.mode === 0 ? modelData.cursorY : modelData.y) + RulerTheme.labelOffsetY
        textValue: modelData.text
        visible: true
    }
}
