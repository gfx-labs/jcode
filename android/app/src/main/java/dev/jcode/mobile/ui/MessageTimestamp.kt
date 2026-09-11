package dev.jcode.mobile.ui

import android.content.Context
import android.text.format.DateFormat
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import java.util.Date
import java.util.Locale
import java.util.TimeZone

internal data class TimestampLabels(val time: String, val fullDate: String)

/** Reformat stored instants in the current device locale, time zone and 12/24-hour preference. */
internal fun timestampLabels(context: Context, timestampUnixMs: Long, locale: Locale): TimestampLabels {
    val date = Date(timestampUnixMs)
    val time = DateFormat.getTimeFormat(context).format(date)
    val zone = TimeZone.getDefault()
    val zoneName = zone.getDisplayName(zone.inDaylightTime(date), TimeZone.LONG, locale)
    return TimestampLabels(time, "${DateFormat.getLongDateFormat(context).format(date)}, $time, $zoneName")
}

@Composable
internal fun MessageTimestamp(timestampUnixMs: Long?) {
    // Old history and unstamped live tails intentionally have no time label.
    if (timestampUnixMs == null) return
    val context = LocalContext.current
    val locale = LocalConfiguration.current.locales[0]
    // Do not remember by message ID: locale/time-zone settings can change while the message survives.
    val labels = timestampLabels(context, timestampUnixMs, locale)
    Text(labels.time, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.testTag("message-timestamp").clearAndSetSemantics { contentDescription = labels.fullDate })
}
