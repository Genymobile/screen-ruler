import QtQuick

Item {
    id: root

    required property real labelX
    required property real labelY
    required property string textValue

    Rectangle {
        id: labelShadow
        x: root.labelX + RulerTheme.labelShadowOffset
        y: root.labelY + RulerTheme.labelShadowOffset
        width: labelBox.width
        height: labelBox.height
        color: RulerTheme.panelShadowColor
        radius: RulerTheme.cornerRadius
        z: 1
    }

    Rectangle {
        id: labelBox
        x: root.labelX
        y: root.labelY
        width: sizeText.implicitWidth + RulerTheme.labelHorizontalPadding
        height: sizeText.implicitHeight + RulerTheme.labelVerticalPadding
        color: RulerTheme.panelBackgroundColor
        opacity: RulerTheme.panelOpacity
        radius: RulerTheme.cornerRadius
        z: 2

        Text {
            id: sizeText
            anchors.centerIn: parent
            text: root.textValue
            color: RulerTheme.primaryTextColor
            font.family: "DejaVu Sans Mono, Consolas, monospace"
            font.pointSize: 13
            font.bold: true
        }
    }
}
