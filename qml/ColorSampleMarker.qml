import QtQuick

Canvas {
    id: root

    required property real markerX
    required property real markerY
    property real sampleRadius: 0
    property color markerColor: RulerTheme.accentColor

    anchors.fill: parent
    Component.onCompleted: requestPaint()
    onMarkerXChanged: requestPaint()
    onMarkerYChanged: requestPaint()
    onSampleRadiusChanged: requestPaint()
    onMarkerColorChanged: requestPaint()

    onPaint: {
        var ctx = getContext("2d")
        ctx.clearRect(0, 0, width, height)

        var centerX = Math.round(markerX)
        var centerY = Math.round(markerY)
        var x = centerX + 0.5
        var y = centerY + 0.5
        var arm = 5
        var gap = 2
        var radius = Math.max(0, Math.round(sampleRadius))
        var radiusSq = radius * radius

        ctx.fillStyle = Qt.rgba(markerColor.r, markerColor.g, markerColor.b, 0.22)
        for (var py = centerY - radius; py <= centerY + radius; py++) {
            var dy = py - centerY
            for (var px = centerX - radius; px <= centerX + radius; px++) {
                var dx = px - centerX
                if (dx * dx + dy * dy <= radiusSq)
                    ctx.fillRect(px, py, 1, 1)
            }
        }

        if (radius === 0) {
            ctx.strokeStyle = markerColor
            ctx.lineWidth = 1
            ctx.beginPath()
            ctx.moveTo(x - arm, y)
            ctx.lineTo(x - gap, y)
            ctx.moveTo(x + gap, y)
            ctx.lineTo(x + arm, y)
            ctx.moveTo(x, y - arm)
            ctx.lineTo(x, y - gap)
            ctx.moveTo(x, y + gap)
            ctx.lineTo(x, y + arm)
            ctx.stroke()
        }

        ctx.fillStyle = markerColor
        ctx.fillRect(centerX - 1, centerY - 1, 3, 3)
    }
}
