import QtQuick
import QtQuick.Controls

Button {
    id: root

    property bool isActive: false
    property string labelText: ""
    property string tooltipText: ""

    property color baseBgColor: RulerTheme.modeButtonBgColor
    property color hoverBgColor: RulerTheme.modeButtonHoverBgColor
    property color pressedBgColor: RulerTheme.modeButtonPressedBgColor
    property color activeBgColor: RulerTheme.modeButtonActiveBgColor

    property color baseBorderColor: RulerTheme.modeButtonBorderColor
    property color hoverBorderColor: RulerTheme.modeButtonHoverBorderColor
    property color pressedBorderColor: RulerTheme.modeButtonPressedBorderColor
    property color activeBorderColor: RulerTheme.accentColor

    implicitWidth: RulerTheme.modeButtonSize
    implicitHeight: RulerTheme.modeButtonSize
    padding: 0
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus

    background: Rectangle {
        radius: RulerTheme.modeButtonRadius
        border.width: 1
        color: root.pressed
               ? root.pressedBgColor
               : (root.hovered ? root.hoverBgColor : (root.isActive ? root.activeBgColor : root.baseBgColor))
        border.color: root.pressed
                      ? root.pressedBorderColor
                      : (root.hovered ? root.hoverBorderColor : (root.isActive ? root.activeBorderColor : root.baseBorderColor))
    }

    contentItem: Text {
        visible: root.labelText !== ""
        text: root.labelText
        color: RulerTheme.primaryTextColor
        font.pointSize: RulerTheme.controlsValuePointSize
        font.bold: true
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    ToolTip {
        visible: root.hovered && root.tooltipText !== ""
        text: root.tooltipText
        delay: 150
        y: root.height + 6

        background: Rectangle {
            radius: RulerTheme.cornerRadius
            color: RulerTheme.panelBackgroundColor
            opacity: RulerTheme.panelOpacity
        }

        contentItem: Text {
            text: root.tooltipText
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
    }
}
