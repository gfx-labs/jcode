package dev.jcode.mobile.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AdaptiveLayoutTest {
    @Test fun coverPhoneUsesSinglePane() {
        assertPlan(adaptivePanePlan(360f, 800f), PaneMode.SINGLE, 360f)
    }

    @Test fun unfoldedAndWideWindowsUseClampedSidebar() {
        assertPlan(adaptivePanePlan(673f, 800f), PaneMode.DUAL, 260f, 1f, 412f)
        assertPlan(adaptivePanePlan(840f, 800f), PaneMode.DUAL, 285.6f, 1f, 553.4f)
        assertPlan(adaptivePanePlan(1200f, 800f), PaneMode.DUAL, 340f, 1f, 859f)
    }

    @Test fun flatThresholdIsInclusive() {
        assertEquals(PaneMode.SINGLE, adaptivePanePlan(639.99f, 800f).mode)
        assertPlan(adaptivePanePlan(640f, 800f), PaneMode.DUAL, 260f, 1f, 379f)
    }

    @Test fun largeFontIsCappedButProtectsMeaningfulWidths() {
        assertPlan(adaptivePanePlan(673f, 800f, 2f), PaneMode.SINGLE, 673f)
        assertEquals(PaneMode.SINGLE, adaptivePanePlan(780f, 800f, 2f).mode)
        assertPlan(adaptivePanePlan(781f, 800f, 2f), PaneMode.DUAL, 338f, 1f, 442f)
        assertEquals(adaptivePanePlan(840f, 800f, 1.3f), adaptivePanePlan(840f, 800f, 2f))
    }

    @Test fun verticalHingePreservesActualGap() {
        assertPlan(adaptivePanePlan(840f, 800f, fold = FoldBounds(true, 400f, 424f)),
            PaneMode.DUAL, 400f, 24f, 416f)
    }

    @Test fun verticalCreaseDoesNotInventDividerSpace() {
        assertPlan(adaptivePanePlan(840f, 800f, fold = FoldBounds(true, 400f, 400f)),
            PaneMode.DUAL, 400f, 0f, 440f)
    }

    @Test fun verticalMinimumsAreInclusiveAndIndependentOfFlatThreshold() {
        assertPlan(adaptivePanePlan(600f, 800f, fold = FoldBounds(true, 260f, 260f)),
            PaneMode.DUAL, 260f, 0f, 340f)
        assertPlan(adaptivePanePlan(600f, 800f, fold = FoldBounds(true, 259f, 260f)),
            PaneMode.SINGLE, 340f, offset = 260f)
    }

    @Test fun narrowVerticalSegmentsUseOnlyLargerClearSide() {
        assertPlan(adaptivePanePlan(673f, 800f, fold = FoldBounds(true, 400f, 424f)),
            PaneMode.SINGLE, 400f)
        assertPlan(adaptivePanePlan(673f, 800f, fold = FoldBounds(true, 200f, 224f)),
            PaneMode.SINGLE, 449f, offset = 224f)
        assertPlan(adaptivePanePlan(840f, 800f, 2f, FoldBounds(true, 400f, 424f)),
            PaneMode.SINGLE, 416f, offset = 424f)
    }

    @Test fun horizontalHingeUsesTabletopEvenOnPhone() {
        assertPlan(adaptivePanePlan(360f, 800f, fold = FoldBounds(false, 380f, 404f)),
            PaneMode.TABLETOP, 380f, 24f, 396f)
        assertPlan(adaptivePanePlan(360f, 256f, fold = FoldBounds(false, 160f, 160f)),
            PaneMode.TABLETOP, 160f, 0f, 96f)
    }

    @Test fun shortHorizontalSegmentsUseLargerClearSideAndYAxisOffset() {
        assertPlan(adaptivePanePlan(840f, 800f, fold = FoldBounds(false, 100f, 124f)),
            PaneMode.SINGLE, 676f, offset = 124f, alongHeight = true)
        assertPlan(adaptivePanePlan(840f, 800f, fold = FoldBounds(false, 700f, 724f)),
            PaneMode.SINGLE, 700f, alongHeight = true)
        assertPlan(adaptivePanePlan(360f, 220f, fold = FoldBounds(false, 100f, 120f)),
            PaneMode.SINGLE, 100f, alongHeight = true)
    }

    @Test fun edgeAndFullWindowFoldsNeverCrossOcclusion() {
        assertPlan(adaptivePanePlan(840f, 800f, fold = FoldBounds(true, 0f, 24f)),
            PaneMode.SINGLE, 816f, offset = 24f)
        assertPlan(adaptivePanePlan(840f, 800f, fold = FoldBounds(true, 816f, 840f)),
            PaneMode.SINGLE, 816f)
        assertPlan(adaptivePanePlan(840f, 800f, fold = FoldBounds(true, 0f, 840f)),
            PaneMode.SINGLE, 0f)
        assertPlan(adaptivePanePlan(840f, 800f, fold = FoldBounds(false, 0f, 24f)),
            PaneMode.SINGLE, 776f, offset = 24f, alongHeight = true)
        assertPlan(adaptivePanePlan(840f, 800f, fold = FoldBounds(true, 0f, 0f)),
            PaneMode.SINGLE, 840f)
    }

    @Test fun invalidFoldsFallBackToFlatLayout() {
        assertPlan(adaptivePanePlan(360f, 800f, fold = FoldBounds(false, 790f, 801f)),
            PaneMode.SINGLE, 360f, alongHeight = false)
        val invalid = listOf(
            FoldBounds(true, Float.NaN, 424f),
            FoldBounds(true, 400f, Float.POSITIVE_INFINITY),
            FoldBounds(true, 424f, 400f),
            FoldBounds(true, -1f, 24f),
            FoldBounds(true, 820f, 841f),
            FoldBounds(true, 900f, 924f),
            FoldBounds(false, 790f, 801f),
        )
        for (fold in invalid) {
            assertEquals("Invalid fold: $fold", adaptivePanePlan(840f, 800f),
                adaptivePanePlan(840f, 800f, fold = fold))
        }
    }

    @Test fun invalidDimensionsAndFontScalesAreSafe() {
        for (width in listOf(-1f, Float.NaN, Float.POSITIVE_INFINITY, Float.NEGATIVE_INFINITY)) {
            assertPlan(adaptivePanePlan(width, 800f), PaneMode.SINGLE, 0f)
        }
        for (height in listOf(-1f, Float.NaN, Float.POSITIVE_INFINITY)) {
            assertEquals(adaptivePanePlan(840f, 0f),
                adaptivePanePlan(840f, height, fold = FoldBounds(false, 160f, 184f)))
        }
        for (scale in listOf(0f, -1f, Float.NaN, Float.POSITIVE_INFINITY)) {
            assertEquals(adaptivePanePlan(840f, 800f), adaptivePanePlan(840f, 800f, scale))
        }
        assertPlan(adaptivePanePlan(0f, 0f), PaneMode.SINGLE, 0f)
        val huge = adaptivePanePlan(Float.MAX_VALUE, Float.MAX_VALUE)
        assertTrue(listOf(huge.first, huge.gap, huge.second, huge.singleOffset)
            .all { it.isFinite() && it >= 0f })
    }

    private fun assertPlan(
        actual: PanePlan,
        mode: PaneMode,
        first: Float,
        gap: Float = 0f,
        second: Float = 0f,
        offset: Float = 0f,
        alongHeight: Boolean = false,
    ) {
        assertEquals(mode, actual.mode)
        assertEquals(alongHeight, actual.singleAlongHeight)
        assertEquals(first, actual.first, 0.001f)
        assertEquals(gap, actual.gap, 0.001f)
        assertEquals(second, actual.second, 0.001f)
        assertEquals(offset, actual.singleOffset, 0.001f)
    }
}
