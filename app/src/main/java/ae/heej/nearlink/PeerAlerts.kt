package ae.heej.nearlink

import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Same defaults and threshold range as the Windows receiver. */
data class PeerAlertSettings(val low: Boolean = true, val threshold: Int = 20, val full: Boolean = true)

enum class PeerAlert { LOW, FULL }

/** Raises each alert once, re-arming low 5 points above the threshold and full below 95%. */
internal class AlertLatch {
    private var low = false
    private var full = false

    fun update(level: Int, charging: Boolean?, settings: PeerAlertSettings): PeerAlert? {
        if (level > settings.threshold + 5) low = false
        if (level < 95) full = false
        // Unknown charging still warns once; only a confirmed charge suppresses it.
        if (level <= settings.threshold && charging != true && !low) {
            low = true
            if (settings.low) return PeerAlert.LOW
        }
        if (level == 100 && !full) {
            full = true
            if (settings.full) return PeerAlert.FULL
        }
        return null
    }
}

object PeerAlerts {
    private val mutable = MutableStateFlow(PeerAlertSettings())
    val settings = mutable.asStateFlow()
    private val latches = mutableMapOf<String, AlertLatch>()
    private var loaded = false

    private fun prefs(context: Context) = context.applicationContext.getSharedPreferences("peer_alerts", Context.MODE_PRIVATE)

    @Synchronized fun load(context: Context) {
        if (loaded) return
        val store = prefs(context)
        mutable.value = PeerAlertSettings(store.getBoolean("low", true),
            store.getInt("threshold", 20).coerceIn(5, 50), store.getBoolean("full", true))
        loaded = true
    }

    @Synchronized fun save(context: Context, value: PeerAlertSettings) {
        val next = value.copy(threshold = value.threshold.coerceIn(5, 50))
        prefs(context).edit { putBoolean("low", next.low); putInt("threshold", next.threshold); putBoolean("full", next.full) }
        mutable.value = next
    }

    @Synchronized fun onReading(context: Context, peerId: String, name: String, level: Int, charging: Boolean?) {
        load(context)
        val alert = latches.getOrPut(peerId) { AlertLatch() }.update(level, charging, mutable.value) ?: return
        val text = when (alert) {
            PeerAlert.LOW -> "배터리가 부족해요. 현재 $level%"
            PeerAlert.FULL -> "배터리가 완전히 충전됐어요."
        }
        Notifications.alert(context, peerId, name, text)
    }

    @Synchronized fun forget(peerId: String) { latches.remove(peerId) }
}
