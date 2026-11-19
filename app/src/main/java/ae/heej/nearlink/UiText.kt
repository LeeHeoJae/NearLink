package ae.heej.nearlink

import android.content.Context
import androidx.annotation.PluralsRes
import androidx.annotation.StringRes
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

/**
 * Text that is resolved when it is shown, so state held in memory follows the current language
 * and pure logic can be tested without Android resources.
 */
sealed interface UiText {
    data class Res(@param:StringRes val id: Int, val args: List<Any> = emptyList()) : UiText
    data class Plural(@param:PluralsRes val id: Int, val count: Int, val args: List<Any> = listOf(count)) : UiText
    data class Raw(val value: String) : UiText

    fun resolve(context: Context): String = when (this) {
        is Raw -> value
        is Res -> context.getString(id, *args.map { it.resolveArg(context) }.toTypedArray())
        is Plural -> context.resources.getQuantityString(id, count, *args.map { it.resolveArg(context) }.toTypedArray())
    }
}

private fun Any.resolveArg(context: Context): Any = if (this is UiText) resolve(context) else this

internal fun ui(@StringRes id: Int, vararg args: Any): UiText = UiText.Res(id, args.toList())

@Composable
internal fun UiText.asString(): String = resolve(LocalContext.current)
