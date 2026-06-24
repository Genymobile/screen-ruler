// AnnotationItem.qml
//
// Renders one frozen annotation from the session annotation list.
// modelData is a dict with: mode, x, y, width, height, text,
// and (for mode 0) cursorX, cursorY, northEnd, southEnd, westEnd, eastEnd.

import QtQuick

Item {
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
    // Selection outline — rectangle-like modes (mode 1/2/3)
    // -----------------------------------------------------------------------
    SelectionOutline {
        x: modelData.x
        y: modelData.y
        width: modelData.width
        height: modelData.height
        visible: modelData.mode === 1 || modelData.mode === 2 || modelData.mode === 3
    }

    ColorSampleMarker {
        markerX: modelData.x
        markerY: modelData.y
        sampleRadius: modelData.sampleRadius ? modelData.sampleRadius : 0
        visible: modelData.mode === 4
    }

    ColorSampleBubble {
        anchorX: modelData.x
        anchorY: modelData.y
        swatchColor: modelData.colorHex ? modelData.colorHex : "#000000"
        hexText: modelData.colorHex ? modelData.colorHex : "#000000"
        rgbText: modelData.colorRgb ? modelData.colorRgb : "rgb(0, 0, 0)"
        hslText: modelData.colorHsl ? modelData.colorHsl : "hsl(0, 0%, 0%)"
        visible: modelData.mode === 4
    }

    // -----------------------------------------------------------------------
    // Measurement label — all modes
    // -----------------------------------------------------------------------
    MeasurementLabel {
        anchorX: modelData.mode === 0 ? modelData.cursorX : modelData.x
        anchorY: modelData.mode === 0 ? modelData.cursorY : modelData.y
        textValue: modelData.text
        visible: modelData.mode !== 4
    }
}
