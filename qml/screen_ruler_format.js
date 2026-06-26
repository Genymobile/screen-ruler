// Shared measurement formatting helpers for QML.

function formatNumber(value, decimals) {
    var factor = Math.pow(10, decimals)
    var rounded = Math.round(value * factor) / factor
    var text = rounded.toFixed(decimals)
    text = text.replace(/\.0+$/, "")
    text = text.replace(/(\.\d*[1-9])0+$/, "$1")
    return text
}

function roundedPair(widthPx, heightPx, includeUnit) {
    var text = Math.round(widthPx) + " \u00D7 " + Math.round(heightPx)
    return includeUnit ? (text + " px") : text
}

function pixelValue(valuePx, includeUnit) {
    var text = formatNumber(Math.abs(valuePx), 0)
    return includeUnit ? (text + " px") : text
}

function distanceValue(distancePx, includeUnit) {
    var text = formatNumber(distancePx, 1)
    return includeUnit ? (text + " px") : text
}

function pointToPointSummary(ax, ay, bx, by, deltaThresholdPx) {
    var deltaX = Math.abs(Math.round(bx - ax))
    var deltaY = Math.abs(Math.round(by - ay))
    var distancePx = Math.sqrt(Math.pow(bx - ax, 2) + Math.pow(by - ay, 2))
    var summary = "A(" + Math.round(ax) + ", " + Math.round(ay)
            + ") \u2192 B(" + Math.round(bx) + ", " + Math.round(by)
            + ") \u2014 " + distanceValue(distancePx, true)
    var threshold = typeof deltaThresholdPx === "number" ? deltaThresholdPx : 8
    if (Math.min(deltaX, deltaY) > threshold)
        summary += " (\u0394x=" + deltaX + ", \u0394y=" + deltaY + ")"
    return summary
}
