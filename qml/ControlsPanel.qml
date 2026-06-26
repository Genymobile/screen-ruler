import QtQuick

Rectangle {
    id: root

    required property int activeMode
    required property int modeRectDrag
    required property int modeColorPicker
    required property int modeDistance
    required property bool hasBackend
    required property real sensitivityMin
    required property real sensitivityMax
    required property real sensitivityStep
    required property real sensitivityDefaultValue
    required property real sensitivityValue
    required property real rectSnapMin
    required property real rectSnapMax
    required property real rectSnapStep
    required property real rectSnapDistance
    required property real colorSampleRadiusMin
    required property real colorSampleRadiusMax
    required property real colorSampleRadiusStep
    required property real colorSampleRadius

    signal panelPressed()
    signal modeSelected(int mode)
    signal sensitivityMoved(real value)
    signal snapMoved(real value)
    signal colorSampleRadiusMoved(real value)

    readonly property bool sensitivitySliderPressed: sensitivityRow.sliderPressed
    readonly property bool snapSliderPressed: snapRow.sliderPressed
    readonly property bool colorSampleSliderPressed: colorSampleRow.sliderPressed
    property alias sensitivitySliderValue: sensitivityRow.sliderValue
    property alias snapSliderValue: snapRow.sliderValue
    property alias colorSampleSliderValue: colorSampleRow.sliderValue

    width: RulerTheme.controlsPanelWidth
    height: (root.activeMode === root.modeRectDrag
            || root.activeMode === root.modeColorPicker
            || root.activeMode === root.modeDistance)
            ? RulerTheme.controlsPanelExpandedHeight
            : RulerTheme.controlsPanelCompactHeight
    radius: RulerTheme.cornerRadius
    color: RulerTheme.panelBackgroundColor
    opacity: RulerTheme.panelOpacity
    z: 30

    MouseArea {
        anchors.fill: parent
        onPressed: root.panelPressed()
        onClicked: (mouse) => mouse.accepted = true
    }

    Column {
        anchors.fill: parent
        anchors.topMargin: RulerTheme.controlsPanelVerticalPadding
        anchors.bottomMargin: RulerTheme.controlsPanelVerticalPadding
        anchors.leftMargin: RulerTheme.baseMargin
        anchors.rightMargin: RulerTheme.baseMargin
        spacing: RulerTheme.controlsColumnSpacing

        ModeSelector {
            activeMode: root.activeMode
            anchors.horizontalCenter: parent.horizontalCenter
            onModeSelected: (mode) => root.modeSelected(mode)
        }

        LabeledSliderRow {
            id: sensitivityRow
            anchors.horizontalCenter: parent.horizontalCenter
            label: "Sensitivity"
            fromValue: root.sensitivityMin
            toValue: root.sensitivityMax
            stepValue: root.sensitivityStep
            sliderEnabled: root.hasBackend
            sliderValue: root.hasBackend ? root.sensitivityValue : root.sensitivityDefaultValue
            onMoved: (value) => root.sensitivityMoved(value)
        }

        LabeledSliderRow {
            id: snapRow
            anchors.horizontalCenter: parent.horizontalCenter
            label: "Snap (px)"
            fromValue: root.rectSnapMin
            toValue: root.rectSnapMax
            stepValue: root.rectSnapStep
            sliderEnabled: root.hasBackend
                           && (root.activeMode === root.modeRectDrag || root.activeMode === root.modeDistance)
            visible: root.activeMode === root.modeRectDrag || root.activeMode === root.modeDistance
            sliderValue: root.rectSnapDistance
            onMoved: (value) => root.snapMoved(value)
        }

        LabeledSliderRow {
            id: colorSampleRow
            anchors.horizontalCenter: parent.horizontalCenter
            label: "Average (px)"
            fromValue: root.colorSampleRadiusMin
            toValue: root.colorSampleRadiusMax
            stepValue: root.colorSampleRadiusStep
            sliderEnabled: root.hasBackend && root.activeMode === root.modeColorPicker
            visible: root.activeMode === root.modeColorPicker
            sliderValue: root.colorSampleRadius
            onMoved: (value) => root.colorSampleRadiusMoved(value)
        }
    }
}
