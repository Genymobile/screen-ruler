import QtQuick

Rectangle {
    id: root

    required property bool overlayVisible
    required property real overlayOpacity
    required property bool sessionMode

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
            text: "1 / 2 / 3 / 4 / 5 / 6  — switch measurement mode"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: root.sessionMode
                  ? "Ctrl+C — copy all annotations as Markdown"
                  : "Ctrl+C — copy measurement, then quit"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: root.sessionMode
                  ? "Click — place annotation (persistent)"
                  : "Click — copy measurement (crosshair/container/color), then quit"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "Enter / click — copy drag/shrink selection, then quit"
            visible: !root.sessionMode
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "Tab — toggle session mode"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: root.sessionMode
                  ? "Tab / Esc — exit session mode"
                  : "Esc / Q — quit"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: root.sessionMode ? "Q — quit app" : ""
            visible: root.sessionMode
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "Ctrl+Z / Z — undo last annotation"
            visible: root.sessionMode
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "Ctrl+Shift+Z — redo annotation"
            visible: root.sessionMode
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "Ctrl+Shift+C — start composite export, then drag region"
            visible: root.sessionMode
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "Mouse wheel — adjust sensitivity/snap distance/color averaging"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }

        Text {
            text: "? / H — toggle this help"
            color: RulerTheme.primaryTextColor
            font.pointSize: RulerTheme.controlsValuePointSize
        }
    }
}
