import QtQuick
import QtQuick.Controls

OverlayActionButton {
    id: root

    required property int modeIndex
    required property int activeMode

    signal modeSelected(int mode)
    isActive: activeMode === modeIndex

    function modeTooltipText(index) {
        switch (index) {
        case 0:
            return "Crosshair"
        case 1:
            return "Drag rectangle"
        case 2:
            return "Container detection"
        case 3:
            return "Shrink-to-fit"
        case 4:
            return "Color picker"
        default:
            return "Mode"
        }
    }

    implicitWidth: RulerTheme.modeButtonSize
    implicitHeight: RulerTheme.modeButtonSize
    tooltipText: modeTooltipText(modeIndex)
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
            } else if (modeIndex === 2) {
                ctx.strokeRect(5, 5, width - 10, height - 10)
                ctx.strokeRect(9, 9, width - 18, height - 18)
            } else if (modeIndex === 3) {
                ctx.strokeRect(5, 5, width - 10, height - 10)
                ctx.strokeRect(10, 10, width - 20, height - 20)
                var c = width / 2
                ctx.beginPath()
                ctx.moveTo(c, 6)
                ctx.lineTo(c - 2, 9)
                ctx.moveTo(c, 6)
                ctx.lineTo(c + 2, 9)
                ctx.moveTo(c, height - 6)
                ctx.lineTo(c - 2, height - 9)
                ctx.moveTo(c, height - 6)
                ctx.lineTo(c + 2, height - 9)
                ctx.moveTo(6, c)
                ctx.lineTo(9, c - 2)
                ctx.moveTo(6, c)
                ctx.lineTo(9, c + 2)
                ctx.moveTo(width - 6, c)
                ctx.lineTo(width - 9, c - 2)
                ctx.moveTo(width - 6, c)
                ctx.lineTo(width - 9, c + 2)
                ctx.stroke()
            } else if (modeIndex === 4) {
                var cx = width / 2
                var cy = height / 2
                ctx.save()
                ctx.translate(cx + 1, cy - 1)
                ctx.rotate(-Math.PI / 4)

                // Pipette body
                ctx.strokeRect(-2, -8, 4, 10)

                // Bulb at the top
                ctx.beginPath()
                ctx.arc(0, -9, 3, Math.PI, 0)
                ctx.stroke()

                // Tip
                ctx.beginPath()
                ctx.moveTo(-2, 2)
                ctx.lineTo(0, 6)
                ctx.lineTo(2, 2)
                ctx.closePath()
                ctx.fill()
                ctx.restore()

                // Small droplet near the tip
                ctx.beginPath()
                ctx.arc(cx + 6, cy + 5, 1.2, 0, Math.PI * 2)
                ctx.fill()
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
