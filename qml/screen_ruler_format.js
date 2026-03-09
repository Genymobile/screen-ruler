// Shared measurement formatting helpers for QML.

function roundedPair(widthPx, heightPx, includeUnit) {
    var text = Math.round(widthPx) + " \u00D7 " + Math.round(heightPx)
    return includeUnit ? (text + " px") : text
}
