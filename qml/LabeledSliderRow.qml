import QtQuick
import QtQuick.Controls

Row {
    id: root

    required property string label
    required property real fromValue
    required property real toValue
    required property real stepValue
    required property bool sliderEnabled

    property real sliderValue: 0
    readonly property bool sliderPressed: slider.pressed

    signal moved(real value)

    spacing: RulerTheme.controlsRowSpacing

    Text {
        anchors.verticalCenter: parent.verticalCenter
        text: root.label
        color: RulerTheme.primaryTextColor
        font.pointSize: RulerTheme.controlsValuePointSize
        font.bold: true
    }

    Slider {
        id: slider
        anchors.verticalCenter: parent.verticalCenter
        from: root.fromValue
        to: root.toValue
        stepSize: root.stepValue
        width: RulerTheme.sensitivitySliderWidth
        enabled: root.sliderEnabled
        value: root.sliderValue
        onMoved: {
            root.sliderValue = value
            root.moved(value)
        }
    }

    Text {
        anchors.verticalCenter: parent.verticalCenter
        text: Math.round(slider.value)
        color: RulerTheme.primaryTextColor
        font.pointSize: RulerTheme.controlsValuePointSize
    }
}
