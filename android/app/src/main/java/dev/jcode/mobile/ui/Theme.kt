package dev.jcode.mobile.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.darkColorScheme
import dev.jcode.mobile.data.ThemeMode
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

val Mist: Color @Composable get() = MaterialTheme.colorScheme.background
val Ink: Color @Composable get() = MaterialTheme.colorScheme.onSurface
val Cobalt: Color @Composable get() = MaterialTheme.colorScheme.primary
val Teal: Color @Composable get() = MaterialTheme.colorScheme.secondary
val Amber: Color @Composable get() = MaterialTheme.colorScheme.tertiary
val Muted: Color @Composable get() = MaterialTheme.colorScheme.onSurfaceVariant
val Line: Color @Composable get() = MaterialTheme.colorScheme.outlineVariant

@Composable
fun JcodeTheme(mode: ThemeMode = ThemeMode.SYSTEM, content: @Composable () -> Unit) {
    val dark = mode == ThemeMode.DARK || (mode == ThemeMode.SYSTEM && isSystemInDarkTheme())
    MaterialTheme(
        colorScheme = if (dark) darkColorScheme(
            primary = Color(0xFFAEC4FF), onPrimary = Color(0xFF122C60),
            secondary = Color(0xFF7BD5D4), onSecondary = Color(0xFF003738),
            tertiary = Color(0xFFE8BA74),
            background = Color(0xFF10151D), onBackground = Color(0xFFE1E8F2),
            surface = Color(0xFF18212D), onSurface = Color(0xFFE1E8F2),
            surfaceVariant = Color(0xFF253347), onSurfaceVariant = Color(0xFFB3C0D2),
            outline = Color(0xFF8290A1), outlineVariant = Color(0xFF354255),
            primaryContainer = Color(0xFF273C61), onPrimaryContainer = Color(0xFFDCE6FF)
        ) else lightColorScheme(
            primary = Color(0xFF315ED4), onPrimary = Color.White,
            secondary = Color(0xFF107D82), onSecondary = Color.White,
            tertiary = Color(0xFFAD6A19),
            primaryContainer = Color(0xFFE8EEFC), onPrimaryContainer = Color(0xFF172D47),
            background = Color(0xFFF3F6FA), onBackground = Color(0xFF172D47),
            surface = Color.White, onSurface = Color(0xFF172D47),
            surfaceVariant = Color(0xFFEAF0F7), onSurfaceVariant = Color(0xFF596A7F),
            outline = Color(0xFF8290A1), outlineVariant = Color(0xFFDCE3ED),
            error = Color(0xFFB23C43)
        ),
        typography = Typography(
            headlineLarge = TextStyle(fontFamily = FontFamily.SansSerif, fontWeight = FontWeight.Bold, fontSize = 26.sp, lineHeight = 32.sp, letterSpacing = (-1).sp),
            headlineMedium = TextStyle(fontFamily = FontFamily.SansSerif, fontWeight = FontWeight.Bold, fontSize = 23.sp, lineHeight = 29.sp, letterSpacing = (-0.6).sp),
            titleLarge = TextStyle(fontFamily = FontFamily.SansSerif, fontWeight = FontWeight.SemiBold, fontSize = 19.sp, lineHeight = 24.sp),
            titleMedium = TextStyle(fontFamily = FontFamily.SansSerif, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, lineHeight = 21.sp),
            bodyLarge = TextStyle(fontFamily = FontFamily.SansSerif, fontSize = 16.sp, lineHeight = 22.sp),
            bodyMedium = TextStyle(fontFamily = FontFamily.SansSerif, fontSize = 14.sp, lineHeight = 19.sp),
            labelSmall = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 11.sp, lineHeight = 16.sp, letterSpacing = 0.3.sp)
        ),
        content = content
    )
}
