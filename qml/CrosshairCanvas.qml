import QtQuick

Canvas {
    id: root

    required property var backend

    anchors.fill: parent

    Connections {
        target: backend
        function onDataChanged() { root.requestPaint() }
    }

    onPaint: {
        var ctx = getContext("2d")
        ctx.clearRect(0, 0, width, height)

        if (!backend || backend.cursorX < 0)
            return

        ctx.strokeStyle = RulerTheme.accentColor
        ctx.lineWidth = 1
        ctx.beginPath()
        ctx.moveTo(backend.cursorX, backend.northEnd)
        ctx.lineTo(backend.cursorX, backend.southEnd)
        ctx.moveTo(backend.westEnd, backend.cursorY)
        ctx.lineTo(backend.eastEnd, backend.cursorY)

        var t = 5
        ctx.moveTo(backend.cursorX - t, backend.northEnd)
        ctx.lineTo(backend.cursorX + t, backend.northEnd)
        ctx.moveTo(backend.cursorX - t, backend.southEnd)
        ctx.lineTo(backend.cursorX + t, backend.southEnd)
        ctx.moveTo(backend.westEnd, backend.cursorY - t)
        ctx.lineTo(backend.westEnd, backend.cursorY + t)
        ctx.moveTo(backend.eastEnd, backend.cursorY - t)
        ctx.lineTo(backend.eastEnd, backend.cursorY + t)

        ctx.stroke()
    }
}
