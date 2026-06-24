import QtQuick
import "floating_panel_position.js" as FloatingPanelPosition

Item {
    id: root

    required property real anchorX
    required property real anchorY
    property real offsetX: RulerTheme.baseMargin + 8
    property real offsetY: RulerTheme.labelOffsetY + 6
    required property color swatchColor
    required property string hexText
    required property string rgbText
    required property string hslText
    readonly property real margin: 2
    readonly property var resolvedPosition: FloatingPanelPosition.resolveFloatingPanelPosition(
        anchorX,
        anchorY,
        bubbleBackground.width,
        bubbleBackground.height,
        offsetX,
        offsetY,
        parent ? parent.width : width,
        parent ? parent.height : height,
        margin
    )
    readonly property real resolvedX: resolvedPosition.x
    readonly property real resolvedY: resolvedPosition.y

    x: resolvedX
    y: resolvedY
    width: bubbleBackground.width
    height: bubbleBackground.height

    Rectangle {
        id: bubbleShadow
        x: RulerTheme.labelShadowOffset
        y: RulerTheme.labelShadowOffset
        width: bubbleBackground.width
        height: bubbleBackground.height
        radius: RulerTheme.cornerRadius
        color: RulerTheme.panelShadowColor
    }

    Rectangle {
        id: bubbleBackground
        width: row.implicitWidth + RulerTheme.labelHorizontalPadding
        height: row.implicitHeight + RulerTheme.labelVerticalPadding
        radius: RulerTheme.cornerRadius
        color: RulerTheme.panelBackgroundColor
        opacity: RulerTheme.panelOpacity

        Row {
            id: row
            anchors.centerIn: parent
            spacing: 10

            Rectangle {
                width: 16
                height: 16
                radius: 2
                border.width: 1
                border.color: RulerTheme.primaryTextColor
                color: root.swatchColor
            }

            Column {
                spacing: 1

                Text {
                    text: root.hexText
                    color: RulerTheme.primaryTextColor
                    font.family: "DejaVu Sans Mono"
                    font.bold: true
                    font.pixelSize: 13
                }

                Text {
                    text: root.rgbText
                    color: RulerTheme.primaryTextColor
                    font.family: "DejaVu Sans Mono"
                    font.bold: true
                    font.pixelSize: 13
                }

                Text {
                    text: root.hslText
                    color: RulerTheme.primaryTextColor
                    font.family: "DejaVu Sans Mono"
                    font.bold: true
                    font.pixelSize: 13
                }
            }
        }
    }
}
