import QtQuick

Row {
    id: root
    required property int activeMode

    signal modeSelected(int mode)

    spacing: RulerTheme.modeRowSpacing

    ModeIconButton {
        modeIndex: 0
        activeMode: root.activeMode
        onModeSelected: (mode) => root.modeSelected(mode)
    }

    ModeIconButton {
        modeIndex: 1
        activeMode: root.activeMode
        onModeSelected: (mode) => root.modeSelected(mode)
    }

    ModeIconButton {
        modeIndex: 2
        activeMode: root.activeMode
        onModeSelected: (mode) => root.modeSelected(mode)
    }

    ModeIconButton {
        modeIndex: 3
        activeMode: root.activeMode
        onModeSelected: (mode) => root.modeSelected(mode)
    }

    ModeIconButton {
        modeIndex: 4
        activeMode: root.activeMode
        onModeSelected: (mode) => root.modeSelected(mode)
    }
}
