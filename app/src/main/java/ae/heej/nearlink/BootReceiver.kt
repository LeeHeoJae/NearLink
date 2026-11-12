package ae.heej.nearlink

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/** Restore only a connection the user left enabled. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val addresses = BridgeSession.savedPeers(context)
        if (addresses.isEmpty()) return
        if (!BridgeSession.hasPermissions(context)) return
        try {
            addresses.forEach { BridgeSession.start(context, it) }
        } catch (e: Exception) {
            Log.e("BridgeBoot", "Could not restore connection after boot", e)
        }
    }
}
