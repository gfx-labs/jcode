package dev.jcode.mobile.ui

import androidx.compose.foundation.layout.requiredSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import dev.jcode.mobile.data.*
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** Actual production composable, Android layout, pointer gestures and effects. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w360dp-h640dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ThreadViewportUiTest {
    @get:Rule val compose = createComposeRule()
    private val session = MobileSession("s")
    private fun history() = (0..60).map { TranscriptEntry("m$it", "assistant", "Message $it") }
    private fun show(state: androidx.compose.runtime.State<MobileState>) {
        compose.setContent {
            MaterialTheme {
                SessionThread(state.value, session, Modifier.requiredSize(320.dp, 400.dp).testTag("viewport"))
            }
        }
    }

    @Test fun opensLongThreadAtNewestMessageNotOldest() {
        show(mutableStateOf(MobileState(transcripts = mapOf("s" to history()))))
        compose.onNodeWithText("Message 60").assertIsDisplayed()
        compose.onNodeWithText("Message 0").assertDoesNotExist()
        compose.onNodeWithContentDescription("Jump to latest messages").assertDoesNotExist()
    }

    @Test fun asynchronousHistoryOpensAtNewestAndFollowsNewMessages() {
        val state = mutableStateOf(MobileState())
        show(state)
        compose.onNodeWithText("The thread starts here").assertIsDisplayed()
        compose.runOnIdle { state.value = MobileState(transcripts = mapOf("s" to history())) }
        compose.onNodeWithText("Message 60").assertIsDisplayed()
        compose.runOnIdle {
            state.value = state.value.copy(transcripts = mapOf("s" to history() + TranscriptEntry("new", "assistant", "New arrival")))
        }
        compose.onNodeWithText("New arrival").assertIsDisplayed()
    }

    @Test fun userScrollUpPreservesReadingPositionAndJumpButtonReturnsToLatest() {
        val state = mutableStateOf(MobileState(transcripts = mapOf("s" to history())))
        show(state)
        compose.onNodeWithTag("viewport").performTouchInput { swipeDown(durationMillis = 600) }
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Jump to latest messages").assertIsDisplayed()
        compose.runOnIdle {
            state.value = state.value.copy(transcripts = mapOf("s" to history() + TranscriptEntry("new", "assistant", "New arrival")))
        }
        compose.onNodeWithText("New arrival").assertIsNotDisplayed()
        compose.onNodeWithContentDescription("Jump to latest messages").performClick()
        compose.onNodeWithText("New arrival").assertIsDisplayed()
        compose.onNodeWithContentDescription("Jump to latest messages").assertDoesNotExist()
    }

    @Test fun tallNewestMessageShowsItsEndInsteadOfItsBeginning() {
        val text = "Beginning\n" + "Long message line\n".repeat(100) + "Newest tail"
        show(mutableStateOf(MobileState(transcripts = mapOf("s" to history() + TranscriptEntry("tall", "assistant", text)))))
        val viewport = compose.onNodeWithTag("viewport").getUnclippedBoundsInRoot()
        val message = compose.onNodeWithText(text).getUnclippedBoundsInRoot()
        assertTrue("Beginning should be above the visible viewport", message.top < viewport.top)
        assertTrue("End should be visible", message.bottom <= viewport.bottom)
        assertTrue("End should be near bottom", message.bottom > viewport.bottom - 70.dp)
    }
}
