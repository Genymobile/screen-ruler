import QtQuick

Rectangle {
    id: root

    property bool bubbleVisible: false
    required property string textValue
    property int textPointSize: RulerTheme.controlsTitlePointSize + 2
    property int horizontalPadding: 24
    property int verticalPadding: 14
    property real maxWidth: 99999

    visible: root.bubbleVisible || opacity > 0.01
    radius: RulerTheme.cornerRadius + 2
    color: RulerTheme.panelBackgroundColor
    opacity: root.bubbleVisible ? 0.95 : 0.0
    scale: root.bubbleVisible ? 1.0 : 0.96
    border.width: 2
    border.color: RulerTheme.sessionModeBorderColor
    width: Math.min(root.maxWidth, bubbleText.implicitWidth + 2 * root.horizontalPadding)
    height: bubbleText.implicitHeight + 2 * root.verticalPadding

    Behavior on opacity {
        NumberAnimation {
            duration: 140
            easing.type: Easing.OutQuad
        }
    }

    Behavior on scale {
        NumberAnimation {
            duration: 160
            easing.type: Easing.OutCubic
        }
    }

    Text {
        id: bubbleText
        anchors.centerIn: parent
        text: root.textValue
        color: RulerTheme.primaryTextColor
        font.pointSize: root.textPointSize
        font.bold: true
    }
}
