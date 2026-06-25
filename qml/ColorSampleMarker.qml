import QtQuick

Item {
    id: root

    required property real markerX
    required property real markerY
    property real sampleRadius: 0
    property color markerColor: RulerTheme.accentColor

    readonly property int centerXInt: Math.round(markerX)
    readonly property int centerYInt: Math.round(markerY)
    readonly property int arm: 5
    readonly property int gap: 2
    readonly property int radiusInt: Math.max(0, Math.round(sampleRadius))
    readonly property int extent: Math.max(radiusInt, arm) + 2

    x: centerXInt - extent
    y: centerYInt - extent
    width: 2 * extent + 1
    height: 2 * extent + 1

    Canvas {
        id: canvas
        anchors.fill: parent

        function repaint() {
            requestPaint()
        }

        Component.onCompleted: repaint()

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)

            var centerX = root.extent
            var centerY = root.extent
            var x = centerX + 0.5
            var y = centerY + 0.5
            var radius = root.radiusInt
            var radiusSq = radius * radius

            ctx.fillStyle = Qt.rgba(root.markerColor.r, root.markerColor.g, root.markerColor.b, 0.22)
            for (var py = centerY - radius; py <= centerY + radius; py++) {
                var dy = py - centerY
                for (var px = centerX - radius; px <= centerX + radius; px++) {
                    var dx = px - centerX
                    if (dx * dx + dy * dy <= radiusSq)
                        ctx.fillRect(px, py, 1, 1)
                }
            }

            if (radius === 0) {
                ctx.strokeStyle = root.markerColor
                ctx.lineWidth = 1
                ctx.beginPath()
                ctx.moveTo(x - root.arm, y)
                ctx.lineTo(x - root.gap, y)
                ctx.moveTo(x + root.gap, y)
                ctx.lineTo(x + root.arm, y)
                ctx.moveTo(x, y - root.arm)
                ctx.lineTo(x, y - root.gap)
                ctx.moveTo(x, y + root.gap)
                ctx.lineTo(x, y + root.arm)
                ctx.stroke()
            }

            ctx.fillStyle = root.markerColor
            ctx.fillRect(centerX - 1, centerY - 1, 3, 3)
        }
    }

    Connections {
        target: root
        function onMarkerXChanged() { canvas.repaint() }
        function onMarkerYChanged() { canvas.repaint() }
        function onSampleRadiusChanged() { canvas.repaint() }
        function onMarkerColorChanged() { canvas.repaint() }
        function onExtentChanged() { canvas.repaint() }
    }
}
