package dev.jcode.mobile.ui

import kotlin.math.max
import kotlin.math.min

/** Arrangement of the primary and detail panes. */
enum class PaneMode { SINGLE, DUAL, TABLETOP }

/** Fold interval in content-local dp: x for vertical folds, y for horizontal folds. */
data class FoldBounds(val vertical: Boolean, val start: Float, val end: Float)

/**
 * Pane extents in dp, along x for flat/vertical layouts and y for horizontal folds.
 * [first] and [second] exclude [gap]. SINGLE has no gap or second pane and begins
 * at [singleOffset] along that same axis. For SINGLE, [singleAlongHeight] selects
 * y/height when true and x/width otherwise. The perpendicular extent is unrestricted.
 */
data class PanePlan(
    val mode: PaneMode,
    val first: Float,
    val gap: Float,
    val second: Float,
    val singleOffset: Float = 0f,
    val singleAlongHeight: Boolean = false,
)

/**
 * Pure layout policy, independent of Compose and window coordinates.
 * Nonfinite/negative dimensions become zero. Invalid font scales default to 1.
 * Reversed, nonfinite, or partly/wholly out-of-window folds are ignored, rather
 * than guessed or clipped. Valid edge folds still reserve their exact interval.
 * Fold-constrained SINGLE uses the larger clear segment (first wins ties).
 */
fun adaptivePanePlan(
    width: Float,
    height: Float,
    fontScale: Float = 1f,
    fold: FoldBounds? = null,
): PanePlan {
    val w = width.sanitizedDimension()
    val h = height.sanitizedDimension()
    val scale = if (fontScale.isFinite() && fontScale > 0f) min(fontScale, 1.3f) else 1f
    val sidebarMin = 260f * scale
    val detailMin = 340f * scale
    val axis = if (fold?.vertical == false) h else w
    val validFold = fold?.takeIf {
        it.start.isFinite() && it.end.isFinite() &&
            it.start >= 0f && it.end >= it.start && it.end <= axis
    }

    if (validFold != null) {
        val first = validFold.start
        val second = axis - validFold.end
        val meaningful = if (validFold.vertical) {
            first >= sidebarMin && second >= detailMin
        } else {
            first >= 160f && second >= 96f
        }
        if (meaningful) {
            return PanePlan(
                if (validFold.vertical) PaneMode.DUAL else PaneMode.TABLETOP,
                first,
                validFold.end - validFold.start,
                second,
            )
        }
        return if (first >= second) {
            PanePlan(PaneMode.SINGLE, first, 0f, 0f, singleAlongHeight = !validFold.vertical)
        } else {
            PanePlan(PaneMode.SINGLE, second, 0f, 0f, validFold.end, !validFold.vertical)
        }
    }

    val flatGap = 1f
    val dualThreshold = max(640f, sidebarMin + detailMin + flatGap)
    if (w < dualThreshold) return PanePlan(PaneMode.SINGLE, w, 0f, 0f)
    val sidebarMax = min(340f * scale, w - detailMin - flatGap)
    val sidebar = (w * 0.34f).coerceIn(sidebarMin, sidebarMax)
    return PanePlan(PaneMode.DUAL, sidebar, flatGap, w - sidebar - flatGap)
}

private fun Float.sanitizedDimension(): Float = if (isFinite()) coerceAtLeast(0f) else 0f
