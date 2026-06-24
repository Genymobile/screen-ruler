.pragma library

function resolveFloatingPanelPosition(
    anchorX,
    anchorY,
    boxWidth,
    boxHeight,
    offsetX,
    offsetY,
    canvasWidth,
    canvasHeight,
    margin
) {
    var safeMargin = margin === undefined ? 2 : margin
    var desiredX = anchorX + offsetX
    var desiredY = anchorY + offsetY
    var resolvedX = desiredX
    var resolvedY = desiredY

    if (desiredX + boxWidth > canvasWidth - safeMargin)
        resolvedX = Math.max(safeMargin, anchorX - offsetX - boxWidth)
    if (desiredY + boxHeight > canvasHeight - safeMargin)
        resolvedY = Math.max(safeMargin, anchorY - offsetY - boxHeight)

    return { x: resolvedX, y: resolvedY }
}
