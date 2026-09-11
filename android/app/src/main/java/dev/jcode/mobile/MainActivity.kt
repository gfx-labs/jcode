package dev.jcode.mobile

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.SystemBarStyle
import androidx.compose.runtime.SideEffect
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.graphics.luminance
import androidx.activity.viewModels
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.compose.runtime.DisposableEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.window.layout.WindowInfoTracker
import androidx.window.layout.FoldingFeature
import dev.jcode.mobile.data.MobileViewModel
import dev.jcode.mobile.data.ConnectionStatus
import dev.jcode.mobile.notifications.SessionMonitorService
import dev.jcode.mobile.notifications.notificationHostKey
import dev.jcode.mobile.ui.JcodeApp
import dev.jcode.mobile.ui.JcodeTheme

class MainActivity : ComponentActivity() {
    private val viewModel: MobileViewModel by viewModels()
    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        // Granting permission is not consent to start a service later or after recreation.
        Toast.makeText(this, if (granted) "Notifications allowed. Tap Start monitoring to begin."
            else "Notifications denied. Monitoring remains stopped. You can allow them in Android settings.", Toast.LENGTH_LONG).show()
    }

    /** Called by notification controls only as a direct response to a visible user tap. */
    fun startSessionMonitoring(host: String): String? {
        val state = viewModel.state.value
        if (state.isDemo || state.connection != ConnectionStatus.CONNECTED ||
            runCatching { notificationHostKey(host) != notificationHostKey(state.host) }.getOrDefault(true)) {
            return "Connect to this gateway before starting monitoring"
        }
        if (!lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) return "Keep the app open and try again"
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
            return "Allow notifications, then tap Start monitoring again. No monitoring starts automatically."
        }
        return SessionMonitorService.start(this, host)
    }

    private fun openNotification(intent: Intent?) {
        if (intent?.action != SessionMonitorService.ACTION_OPEN) return
        val uri = intent.data ?: return
        if (uri.scheme != "jcode-notification" || uri.host != "session") return
        val parts = uri.pathSegments
        if (parts.size == 2 && parts[0].matches(Regex("[a-f0-9]{64}")) && parts[1].isNotBlank()) {
            viewModel.openNotificationSession(parts[0], parts[1])
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        openNotification(intent)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) openNotification(intent)
        enableEdgeToEdge()
        setContent {
            val state by viewModel.state.collectAsStateWithLifecycle()
            val windowInfo = remember { WindowInfoTracker.getOrCreate(this).windowLayoutInfo(this) }
            val layout by windowInfo.collectAsState(initial = null)
            DisposableEffect(lifecycle) {
                val observer = LifecycleEventObserver { _, event ->
                    when (event) {
                        Lifecycle.Event.ON_START -> viewModel.setForeground(true)
                        Lifecycle.Event.ON_STOP -> viewModel.setForeground(false)
                        else -> Unit
                    }
                }
                lifecycle.addObserver(observer)
                viewModel.setForeground(lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED))
                onDispose {
                    lifecycle.removeObserver(observer)
                    viewModel.setForeground(false)
                }
            }
            val appearance by viewModel.appearance.collectAsStateWithLifecycle()
            JcodeTheme(mode = appearance.theme) {
                val dark = MaterialTheme.colorScheme.background.luminance() < 0.5f
                SideEffect {
                    val style = if (dark) SystemBarStyle.dark(android.graphics.Color.TRANSPARENT)
                        else SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT)
                    enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
                }
                JcodeApp(appearance.present(state), viewModel, layout?.displayFeatures?.filterIsInstance<FoldingFeature>()?.firstOrNull())
            }
        }
    }
}
