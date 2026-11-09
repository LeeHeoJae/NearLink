package ae.heej.nearlink

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/** Restore only a connection the user left enabled. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val address = BridgeSession.savedPeer(context) ?: return
        if (!BridgeSession.hasPermissions(context)) return
        try {
            BridgeSession.start(context, address)
        } catch (e: Exception) {
            Log.e("BridgeBoot", "Could not restore connection after boot", e)
        }
    }
}
