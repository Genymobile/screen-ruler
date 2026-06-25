import QtQuick
import "floating_panel_position.js" as FloatingPanelPosition

Item {
    id: root

    required property real anchorX
    required property real anchorY
    property real offsetX: RulerTheme.baseMargin
    property real offsetY: RulerTheme.labelOffsetY
    required property string textValue
    readonly property real margin: 2
    readonly property var resolvedPosition: FloatingPanelPosition.resolveFloatingPanelPosition(
        anchorX,
        anchorY,
        labelBox.width,
        labelBox.height,
        offsetX,
        offsetY,
        parent ? parent.width : width,
        parent ? parent.height : height,
        margin
    )
    readonly property real resolvedX: resolvedPosition.x
    readonly property real resolvedY: resolvedPosition.y

    Rectangle {
        id: labelShadow
        x: root.resolvedX + RulerTheme.labelShadowOffset
        y: root.resolvedY + RulerTheme.labelShadowOffset
        width: labelBox.width
        height: labelBox.height
        color: RulerTheme.panelShadowColor
        radius: RulerTheme.cornerRadius
        z: 1

    }

    Rectangle {
        id: labelBox
        x: root.resolvedX
        y: root.resolvedY
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
