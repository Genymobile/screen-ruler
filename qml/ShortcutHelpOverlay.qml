import QtQuick

Rectangle {
    id: root

    required property bool overlayVisible
    required property real overlayOpacity
    required property string modeHint

    width: RulerTheme.controlsPanelWidth
    height: helpOverlayContent.implicitHeight + 2 * RulerTheme.helpOverlayVerticalPadding
    radius: RulerTheme.cornerRadius
    color: RulerTheme.panelBackgroundColor
    opacity: root.overlayOpacity * RulerTheme.panelOpacity
    visible: root.overlayVisible
    z: 20

    Column {
        id: helpOverlayContent
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.leftMargin: RulerTheme.baseMargin
        anchors.rightMargin: RulerTheme.baseMargin
        anchors.topMargin: RulerTheme.helpOverlayVerticalPadding
        spacing: RulerTheme.helpOverlayColumnSpacing

        Text {
            text: "Shortcuts"
            color: RulerTheme.primaryTextColor
            font.bold: true
            font.pointSize: RulerTheme.controlsTitlePointSize
        }

        Text {
            text: "1 / 2 / 3  — switch measurement mode"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "Tab — enter session mode (#6)"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "Ctrl+C — copy measurement to clipboard"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "Esc / Q — quit"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "? / H — toggle this help"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: root.modeHint
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
            font.italic: true
        }
    }
}
