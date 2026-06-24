import QtQuick

Item {
    id: root

    required property real bubbleX
    required property real bubbleY
    required property color swatchColor
    required property string hexText
    required property string rgbText
    required property string hslText

    x: bubbleX
    y: bubbleY
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
