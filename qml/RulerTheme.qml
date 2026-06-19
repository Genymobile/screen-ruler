pragma Singleton
import QtQuick

QtObject {
    // Core color tokens
    readonly property color accentColor: "#E6195E"
    readonly property color panelBackgroundColor: "#1A1A1A"
    readonly property color primaryTextColor: "#FFFFFF"
    readonly property color panelShadowColor: Qt.rgba(0, 0, 0, 0.22)

    // Shared sizing and spacing
    readonly property int baseMargin: 14
    readonly property int cornerRadius: 5
    readonly property real panelOpacity: 0.9

    // Label metrics
    readonly property int labelOffsetY: 4
    readonly property int labelShadowOffset: 2
    readonly property int labelHorizontalPadding: 18
    readonly property int labelVerticalPadding: 12

    // Overlay defaults
    readonly property real debugOverlayOpacity: 0.3

    // Controls panel defaults
    readonly property int controlsPanelWidth: 380
    readonly property int controlsPanelCompactHeight: 104
    readonly property int controlsPanelExpandedHeight: 136
    readonly property int controlsPanelVerticalPadding: 8
    readonly property int controlsRowSpacing: 12
    readonly property int controlsColumnSpacing: 8
    readonly property int modeRowSpacing: 8

    readonly property int controlsTitlePointSize: 11
    readonly property int controlsValuePointSize: 10

    readonly property int helpOverlayVerticalPadding: 10
    readonly property int helpOverlayColumnSpacing: 4
    readonly property int helpOverlayAutoHideMs: 2000
    readonly property int helpOverlayFadeMs: 220

    readonly property int modeButtonSize: 30
    readonly property int modeButtonRadius: 4
    readonly property color modeButtonBorderColor: Qt.rgba(0.52, 0.56, 0.61, 0.5)
    readonly property color modeButtonBgColor: Qt.rgba(0.26, 0.28, 0.31, 0.7)
    readonly property color modeButtonActiveBgColor: Qt.rgba(0.90, 0.10, 0.37, 0.18)

    readonly property int sensitivitySliderWidth: 240
    readonly property int sensitivityDefaultValue: 85
}
