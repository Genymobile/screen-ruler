import QtQuick
import QtQuick.Controls

OverlayActionButton {
    id: root

    required property int modeIndex
    required property int activeMode

    signal modeSelected(int mode)
    readonly property bool isActive: activeMode === modeIndex

    implicitWidth: RulerTheme.modeButtonSize
    implicitHeight: RulerTheme.modeButtonSize
    tooltipText: modeIndex === 0 ? "Crosshair" : (modeIndex === 1 ? "Drag rectangle" : "Container detection")
    baseBgColor: RulerTheme.modeButtonBgColor
    hoverBgColor: RulerTheme.modeButtonHoverBgColor
    pressedBgColor: RulerTheme.modeButtonPressedBgColor
    activeBgColor: RulerTheme.modeButtonActiveBgColor
    baseBorderColor: RulerTheme.modeButtonBorderColor
    hoverBorderColor: RulerTheme.modeButtonHoverBorderColor
    pressedBorderColor: RulerTheme.modeButtonPressedBorderColor
    activeBorderColor: RulerTheme.accentColor

    onClicked: {
        if (isActive)
            return
        modeSelected(modeIndex)
    }

    contentItem: Canvas {
        anchors.fill: parent

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)
            var iconColor = (root.isActive || root.pressed)
                    ? RulerTheme.accentColor
                    : RulerTheme.primaryTextColor

            ctx.strokeStyle = iconColor
            ctx.fillStyle = iconColor
            ctx.lineWidth = 1.5

            if (modeIndex === 0) {
                var cx = width / 2
                var cy = height / 2
                var ray = 9
                var cap = 3
                ctx.beginPath()
                ctx.moveTo(cx - ray, cy)
                ctx.lineTo(cx + ray, cy)
                ctx.moveTo(cx, cy - ray)
                ctx.lineTo(cx, cy + ray)
                ctx.moveTo(cx - ray, cy - cap)
                ctx.lineTo(cx - ray, cy + cap)
                ctx.moveTo(cx + ray, cy - cap)
                ctx.lineTo(cx + ray, cy + cap)
                ctx.moveTo(cx - cap, cy - ray)
                ctx.lineTo(cx + cap, cy - ray)
                ctx.moveTo(cx - cap, cy + ray)
                ctx.lineTo(cx + cap, cy + ray)
                ctx.stroke()
            } else if (modeIndex === 1) {
                var margin = 7
                ctx.strokeRect(margin, margin, width - 2 * margin, height - 2 * margin)
                var cornerX = width - margin
                var cornerY = height - margin
                var cornerSize = 3
                ctx.beginPath()
                ctx.moveTo(cornerX - cornerSize, cornerY)
                ctx.lineTo(cornerX + cornerSize, cornerY)
                ctx.moveTo(cornerX, cornerY - cornerSize)
                ctx.lineTo(cornerX, cornerY + cornerSize)
                ctx.stroke()
            } else {
                ctx.strokeRect(5, 5, width - 10, height - 10)
                ctx.strokeRect(9, 9, width - 18, height - 18)
            }
        }

        Connections {
            target: root
            function onIsActiveChanged() { root.contentItem.requestPaint() }
            function onHoveredChanged() { root.contentItem.requestPaint() }
            function onPressedChanged() { root.contentItem.requestPaint() }
        }
    }

}
