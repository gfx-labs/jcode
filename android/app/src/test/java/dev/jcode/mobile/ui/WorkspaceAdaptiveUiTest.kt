package dev.jcode.mobile.ui

import android.graphics.Rect
import android.graphics.Bitmap
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.*
import androidx.window.layout.FoldingFeature
import dev.jcode.mobile.data.*
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w1200dp-h1200dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class WorkspaceAdaptiveUiTest {
    @get:Rule val compose = createComposeRule()
    private data class Frame(val width: Int = 360, val height: Int = 760, val scale: Float = 1f, val fold: FoldingFeature? = null)
    private class Fold(override val bounds: Rect, override val orientation: FoldingFeature.Orientation) : FoldingFeature {
        override val isSeparating = true
        override val occlusionType = FoldingFeature.OcclusionType.FULL
        override val state = FoldingFeature.State.HALF_OPENED
    }
    private lateinit var view: android.view.View
    private val sessions = listOf(
        MobileSession("fresh", "Gateway improvements", "working", isProcessing = true, workingDirectory = "/home/dev/projects/jcode", lastActivityAgeSecs = 4),
        MobileSession("old", "Documentation", "idle", workingDirectory = "/home/dev/projects/docs", lastActivityAgeSecs = 86400)
    )
    private fun state(selected: String? = "fresh") = MobileState(connection = ConnectionStatus.CONNECTED, sessions = sessions, selectedSessionId = selected,
        transcripts = mapOf("fresh" to (0..30).map { TranscriptEntry("m$it", if (it % 3 == 0) "user" else "assistant", "Message $it: Updated gateway connection handling and verified the Android client.") }))
    private fun show(frame: State<Frame>, data: MutableState<MobileState>) {
        compose.setContent {
            CompositionLocalProvider(LocalDensity provides Density(1f, frame.value.scale)) {
                view = LocalView.current
                JcodeTheme { Box(Modifier.requiredSize(frame.value.width.dp, frame.value.height.dp).testTag("app")) {
                    Workspace(data.value, frame.value.fold, WorkspaceActions(selectSession = { data.value = data.value.copy(selectedSessionId = it) }))
                } }
            }
        }
    }
    private fun screenshot(name: String) {
        val directory = File(System.getProperty("user.home"), ".jcode/scratch/android-adaptive-screens").apply { mkdirs() }
        val bounds = compose.onNodeWithTag("app").getUnclippedBoundsInRoot()
        compose.runOnIdle {
            val bitmap = Bitmap.createBitmap(bounds.width.value.toInt(), bounds.height.value.toInt(), Bitmap.Config.ARGB_8888)
            val canvas = android.graphics.Canvas(bitmap)
            canvas.translate(-bounds.left.value, -bounds.top.value)
            view.draw(canvas)
            File(directory, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
    }
    @Test fun phoneSearchSelectionAndFocusedChat() {
        show(mutableStateOf(Frame()), mutableStateOf(state(null)))
        compose.onNodeWithTag("session-search").performTextInput("docs")
        compose.onNodeWithTag("session:old").assertIsDisplayed()
        compose.onNodeWithTag("session:fresh").assertDoesNotExist()
        screenshot("phone-search")
        compose.onNodeWithTag("session:old").performClick()
        compose.onNodeWithTag("composer").assertIsDisplayed()
        compose.onNodeWithTag("session-search").assertDoesNotExist()
        val viewport = compose.onNodeWithTag("thread-viewport").getUnclippedBoundsInRoot()
        assertTrue(viewport.height > 500.dp)
        screenshot("phone-chat")
    }
    @Test fun foldTransitionsPreserveDraftAndAvoidHinge() {
        val frame = mutableStateOf(Frame())
        show(frame, mutableStateOf(state()))
        compose.onNodeWithTag("message-input").performTextInput("Keep this draft")
        compose.runOnIdle { frame.value = Frame(840, 900) }
        compose.onNodeWithTag("session-list-pane").assertIsDisplayed()
        compose.onNodeWithTag("message-input").assertTextContains("Keep this draft")
        screenshot("unfolded")
        compose.runOnIdle { frame.value = Frame(840, 900, fold = Fold(Rect(400, 0, 424, 900), FoldingFeature.Orientation.VERTICAL)) }
        val app = compose.onNodeWithTag("app").getUnclippedBoundsInRoot()
        // FoldingFeature coordinates are window-local, while the test's frame may be centered.
        compose.runOnIdle { frame.value = frame.value.copy(fold = Fold(Rect((app.left.value + 400).toInt(), 0, (app.left.value + 424).toInt(), 1200), FoldingFeature.Orientation.VERTICAL)) }
        assertEquals(400f, (compose.onNodeWithTag("session-list-pane").getUnclippedBoundsInRoot().right - app.left).value, 1f)
        assertEquals(424f, (compose.onNodeWithTag("conversation-pane").getUnclippedBoundsInRoot().left - app.left).value, 1f)
        screenshot("book-fold")
        compose.runOnIdle { frame.value = Frame(840, 900, fold = Fold(Rect(0, (app.top.value + 400).toInt(), 1200, (app.top.value + 424).toInt()), FoldingFeature.Orientation.HORIZONTAL)) }
        val thread = compose.onNodeWithTag("tabletop-thread").getUnclippedBoundsInRoot()
        val composer = compose.onNodeWithTag("composer").getUnclippedBoundsInRoot()
        assertTrue(thread.bottom <= app.top + 400.dp)
        assertTrue(composer.top >= app.top + 424.dp)
        compose.onNodeWithTag("message-input").assertTextContains("Keep this draft")
        screenshot("tabletop")
        compose.runOnIdle { frame.value = Frame() }
        compose.onNodeWithTag("message-input").assertTextContains("Keep this draft")
    }
    @Test fun largeFontsAndShortWindowKeepComposerUsable() {
        show(mutableStateOf(Frame(360, 420, 1.6f)), mutableStateOf(state()))
        compose.onNodeWithTag("composer").assertIsDisplayed()
        compose.onNodeWithTag("message-input").performTextInput("A reply")
        compose.onNodeWithContentDescription("Send message to Gateway improvements").assertIsDisplayed().assertIsEnabled()
        val input = compose.onNodeWithTag("composer").getUnclippedBoundsInRoot()
        val app = compose.onNodeWithTag("app").getUnclippedBoundsInRoot()
        assertTrue(input.bottom <= app.bottom)
        screenshot("large-font-short-window")
    }
}
