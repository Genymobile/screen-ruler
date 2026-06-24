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

        var x = Math.round(markerX) + 0.5
        var y = Math.round(markerY) + 0.5
        var arm = 5
        var gap = 2
        var radius = Math.max(0.8, sampleRadius)

        ctx.strokeStyle = markerColor
        ctx.lineWidth = 1
        ctx.beginPath()
        ctx.arc(x, y, radius, 0, Math.PI * 2)
        ctx.stroke()

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

        ctx.fillStyle = markerColor
        ctx.fillRect(Math.round(markerX) - 1, Math.round(markerY) - 1, 3, 3)
    }
}
