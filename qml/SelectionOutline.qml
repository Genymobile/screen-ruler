import QtQuick

Rectangle {
    id: root

    property bool animateGeometry: false

    color: "transparent"
    border.width: 1
    border.color: RulerTheme.accentColor

    Behavior on x {
        enabled: root.animateGeometry
        NumberAnimation {
            duration: RulerTheme.selectionTransitionMs
            easing.type: Easing.OutCubic
        }
    }

    Behavior on y {
        enabled: root.animateGeometry
        NumberAnimation {
            duration: RulerTheme.selectionTransitionMs
            easing.type: Easing.OutCubic
        }
    }

    Behavior on width {
        enabled: root.animateGeometry
        NumberAnimation {
            duration: RulerTheme.selectionTransitionMs
            easing.type: Easing.OutCubic
        }
    }

    Behavior on height {
        enabled: root.animateGeometry
        NumberAnimation {
            duration: RulerTheme.selectionTransitionMs
            easing.type: Easing.OutCubic
        }
    }
}
