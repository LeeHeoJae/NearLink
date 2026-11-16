package ae.heej.nearlink

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** App-owned One UI inspired tokens; never depend on the wallpaper palette. */
internal object BridgeDesign {
    val pageInset = 20.dp
    val groupInset = 24.dp
    val gap = 16.dp
    val groupShape = RoundedCornerShape(26.dp)
    val controlShape = RoundedCornerShape(24.dp)
    /** Upper bound for the whole page on very wide windows. */
    val maxWidth = 1000.dp
    /** From this width the managed screen splits into a control panel and a device list. */
    val expandedWidth = 720.dp
    /** Readable single-column width for medium windows (portrait tablets, split screen). */
    val contentWidth = 760.dp
    val sidePanelWidth = 320.dp
    /** Device cards go two-up when the list is at least this wide. */
    val gridWidth = 640.dp
    /** Device cards switch to the horizontal layout (battery beside the name) at this width. */
    val wideCardWidth = 440.dp
}

private val LightColors = lightColorScheme(
    primary = Color(0xFF1559CE), onPrimary = Color.White,
    primaryContainer = Color(0xFFE8F0FF), onPrimaryContainer = Color(0xFF164B9E),
    secondaryContainer = Color(0xFFE8F0FF), onSecondaryContainer = Color(0xFF164B9E),
    background = Color(0xFFF2F2F6), onBackground = Color(0xFF17171B),
    surface = Color.White, onSurface = Color(0xFF17171B),
    surfaceContainer = Color.White, surfaceContainerHigh = Color.White, surfaceContainerHighest = Color(0xFFF2F2F6),
    onSurfaceVariant = Color(0xFF595961), outlineVariant = Color(0xFFE5E5EB),
    error = Color(0xFFB3261E), errorContainer = Color(0xFFFFEDEA), onErrorContainer = Color(0xFF8C1D18)
)
private val DarkColors = darkColorScheme(
    primary = Color(0xFF9BBFFF), onPrimary = Color(0xFF002F77),
    primaryContainer = Color(0xFF18345C), onPrimaryContainer = Color(0xFFD8E6FF),
    secondaryContainer = Color(0xFF18345C), onSecondaryContainer = Color(0xFFD8E6FF),
    background = Color(0xFF08080A), onBackground = Color(0xFFF3F3F6),
    surface = Color(0xFF1C1C20), onSurface = Color(0xFFF3F3F6),
    surfaceContainer = Color(0xFF26262B), surfaceContainerHigh = Color(0xFF26262B), surfaceContainerHighest = Color(0xFF303036),
    onSurfaceVariant = Color(0xFFB5B5BF), outlineVariant = Color(0xFF38383F),
    error = Color(0xFFFFB4AB), errorContainer = Color(0xFF4D211F), onErrorContainer = Color(0xFFFFDAD6)
)
private val BridgeTypography = Typography(
    headlineLarge = TextStyle(fontSize = 34.sp, lineHeight = 42.sp, fontWeight = FontWeight.Bold),
    displayMedium = TextStyle(fontSize = 56.sp, lineHeight = 64.sp, fontWeight = FontWeight.Bold),
    titleLarge = TextStyle(fontSize = 22.sp, lineHeight = 30.sp, fontWeight = FontWeight.SemiBold),
    titleMedium = TextStyle(fontSize = 18.sp, lineHeight = 26.sp, fontWeight = FontWeight.SemiBold),
    bodyLarge = TextStyle(fontSize = 16.sp, lineHeight = 24.sp),
    bodyMedium = TextStyle(fontSize = 14.sp, lineHeight = 21.sp),
    bodySmall = TextStyle(fontSize = 12.sp, lineHeight = 18.sp),
    labelLarge = TextStyle(fontSize = 16.sp, lineHeight = 24.sp, fontWeight = FontWeight.SemiBold)
)

// Menus use extraSmall and dialogs use extraLarge by default; align both with the app's rounded groups.
private val BridgeShapes = Shapes(
    extraSmall = RoundedCornerShape(20.dp),
    large = BridgeDesign.groupShape,
    extraLarge = BridgeDesign.groupShape
)

@Composable
internal fun BridgeTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) DarkColors else LightColors,
        typography = BridgeTypography, shapes = BridgeShapes, content = content)
}
