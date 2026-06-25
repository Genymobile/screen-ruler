import QtQuick

Item {
    required property int activeMode
    required property int modeRectDrag
    required property int modeShrinkToFit
    required property int modeDistance
    required property bool canCopy
    required property bool sessionMode
    required property bool quickRectConfirmPending

    signal pointerPressed(real x, real y, int button)
    signal pointerMoved(real x, real y)
    signal pointerReleased(real x, real y, int button)
    signal copyRequested()
    signal sessionClickRequested(real x, real y)
    signal wheelAdjusted(real deltaY)

    anchors.fill: parent

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true

        onPressed: (mouse) => {
            pointerPressed(mouse.x, mouse.y, mouse.button)
            if ((activeMode === modeRectDrag || activeMode === modeShrinkToFit)
                    && !quickRectConfirmPending
                    && mouse.button === Qt.LeftButton)
                mouse.accepted = true
        }

        onPositionChanged: (mouse) => {
            pointerMoved(mouse.x, mouse.y)
        }

        onReleased: (mouse) => {
            pointerReleased(mouse.x, mouse.y, mouse.button)
            if ((activeMode === modeRectDrag || activeMode === modeShrinkToFit)
                    && !quickRectConfirmPending
                    && mouse.button === Qt.LeftButton)
                mouse.accepted = true
        }

        onClicked: (mouse) => {
            if (sessionMode) {
                sessionClickRequested(mouse.x, mouse.y)
                mouse.accepted = true
                return
            }
            if (quickRectConfirmPending) {
                copyRequested()
                mouse.accepted = true
                return
            }
            if (activeMode === modeDistance) {
                copyRequested()
                mouse.accepted = true
                return
            }
            if (!canCopy) {
                mouse.accepted = true
                return
            }
            copyRequested()
        }

        onWheel: (wheel) => {
            wheelAdjusted(wheel.angleDelta.y)
            wheel.accepted = true
        }
    }
}
