import QtQuick

Canvas {
    id: root

    required property real markerX
    required property real markerY
    required property bool isSnapped

    x: markerX - 5
    y: markerY - 5
    width: 10
    height: 10
    z: 2

    onMarkerXChanged: requestPaint()
    onMarkerYChanged: requestPaint()
    onIsSnappedChanged: requestPaint()
    onVisibleChanged: requestPaint()

    onPaint: {
        var ctx = getContext("2d")
        ctx.clearRect(0, 0, width, height)

        var color = isSnapped ? RulerTheme.accentColor : RulerTheme.primaryTextColor
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
