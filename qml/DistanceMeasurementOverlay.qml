import QtQuick
import "screen_ruler_format.js" as Format

Item {
    id: root

    required property real pointAX
    required property real pointAY
    required property real pointBX
    required property real pointBY
    property real triangleThreshold: 8
    property color lineColor: RulerTheme.accentColor
    property real labelNormalOffset: 14
    property real endpointRadius: 2.5

    readonly property real deltaX: root.pointBX - root.pointAX
    readonly property real deltaY: root.pointBY - root.pointAY
    readonly property real distancePx: Math.sqrt(root.deltaX * root.deltaX + root.deltaY * root.deltaY)
    readonly property real safeDistancePx: Math.max(root.distancePx, 0.001)
    readonly property real absDeltaX: Math.abs(root.deltaX)
    readonly property real absDeltaY: Math.abs(root.deltaY)
    readonly property bool triangleVisible: Math.min(root.absDeltaX, root.absDeltaY) > root.triangleThreshold
    readonly property real triangleCornerX: root.pointBX
    readonly property real triangleCornerY: root.pointAY
    readonly property real lineMidX: (root.pointAX + root.pointBX) / 2
    readonly property real lineMidY: (root.pointAY + root.pointBY) / 2
    readonly property real lineNormalX: -root.deltaY / root.safeDistancePx
    readonly property real lineNormalY: root.deltaX / root.safeDistancePx
    readonly property real horizontalMidX: (root.pointAX + root.triangleCornerX) / 2
    readonly property real horizontalMidY: root.pointAY
    readonly property real verticalMidX: root.pointBX
    readonly property real verticalMidY: (root.pointAY + root.pointBY) / 2

    function repaint() {
        measurementCanvas.requestPaint()
    }

    onPointAXChanged: repaint()
    onPointAYChanged: repaint()
    onPointBXChanged: repaint()
    onPointBYChanged: repaint()
    onTriangleThresholdChanged: repaint()
    onLineColorChanged: repaint()

    Canvas {
        id: measurementCanvas
        anchors.fill: parent

        Component.onCompleted: requestPaint()
        onVisibleChanged: requestPaint()

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)

            ctx.strokeStyle = root.lineColor
            ctx.fillStyle = root.lineColor
            ctx.lineWidth = 1.5

            ctx.beginPath()
            ctx.moveTo(root.pointAX, root.pointAY)
            ctx.lineTo(root.pointBX, root.pointBY)
            ctx.stroke()

            if (root.triangleVisible) {
                ctx.save()
                ctx.globalAlpha = 0.7
                ctx.setLineDash([5, 4])
                ctx.beginPath()
                ctx.moveTo(root.pointAX, root.pointAY)
                ctx.lineTo(root.triangleCornerX, root.triangleCornerY)
                ctx.lineTo(root.pointBX, root.pointBY)
                ctx.stroke()
                ctx.restore()
            }

            ctx.beginPath()
            ctx.arc(root.pointAX, root.pointAY, root.endpointRadius, 0, Math.PI * 2)
            ctx.arc(root.pointBX, root.pointBY, root.endpointRadius, 0, Math.PI * 2)
            ctx.fill()
        }
    }

    MeasurementLabel {
        anchorX: root.lineMidX
        anchorY: root.lineMidY
        offsetX: root.lineNormalX * root.labelNormalOffset
        offsetY: root.lineNormalY * root.labelNormalOffset
        textValue: Format.distanceValue(root.distancePx, true)
    }

    MeasurementLabel {
        anchorX: root.horizontalMidX
        anchorY: root.horizontalMidY
        offsetX: 0
        offsetY: root.deltaY >= 0 ? 8 : -28
        textValue: Format.pixelValue(root.deltaX, true)
        visible: root.triangleVisible
    }

    MeasurementLabel {
        anchorX: root.verticalMidX
        anchorY: root.verticalMidY
        offsetX: root.deltaX >= 0 ? 10 : -10
        offsetY: root.deltaY >= 0 ? 0 : -20
        textValue: Format.pixelValue(root.deltaY, true)
        visible: root.triangleVisible
    }
}
