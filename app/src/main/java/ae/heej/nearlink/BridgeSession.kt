package ae.heej.nearlink

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build

/** User intent is persisted separately from historical readings and live service state. */
object BridgeSession {
    private fun prefs(context: Context) = context.getSharedPreferences("bridge_session", Context.MODE_PRIVATE)

    /** Migrate the old single-peer preference once, without re-enabling a stopped peer. */
    @Synchronized fun savedPeers(context: Context): Set<String> {
        val store = prefs(context)
        if (!store.contains("peers_v2")) {
            val old = store.getString("address", null)
            val peers = if (store.getBoolean("enabled", false) && old != null) setOf(old) else emptySet()
            store.edit().putStringSet("peers_v2", peers).commit()
        }
        return store.getStringSet("peers_v2", emptySet())!!.toSet()
    }

    fun savedPeer(context: Context): String? = savedPeers(context).firstOrNull()
    fun contains(context: Context, address: String): Boolean = savedPeers(context).any { it.equals(address, true) }

    @Synchronized fun removePeer(context: Context, address: String) {
        val remaining = savedPeers(context).filterNot { it.equals(address, true) }.toSet()
        prefs(context).edit().putStringSet("peers_v2", remaining).commit()
    }

    fun hasPermissions(context: Context): Boolean = if (Build.VERSION.SDK_INT >= 31) {
        listOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_SCAN,
            Manifest.permission.BLUETOOTH_ADVERTISE).all { context.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED }
    } else context.checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED

    @Synchronized fun start(context: Context, address: String) {
        val normalized = address.uppercase(java.util.Locale.ROOT)
        context.getSharedPreferences("bridge_peer_readings", Context.MODE_PRIVATE).edit()
            .remove("$normalized.stop_unconfirmed").apply()
        prefs(context).edit().putStringSet("peers_v2", savedPeers(context) + normalized)
            .putString("address", normalized).putBoolean("enabled", true).remove("stop_notice").commit()
        context.startForegroundService(Intent(context, BatteryService::class.java))
        context.startForegroundService(Intent(context, ReceiverService::class.java).putExtra("address", address))
    }

    fun disable(context: Context) {
        prefs(context).edit().putBoolean("enabled", false).putStringSet("peers_v2", emptySet()).apply()
    }

    fun requestStop(context: Context, address: String? = null) {
        val targets = if (address == null) savedPeers(context) else setOf(address)
        if (targets.isEmpty()) { stop(context); return }
        // Commit local intent first so process death cannot silently restore a stopped peer.
        targets.forEach { target ->
            removePeer(context, target)
            BridgeStatus.peerStopped(context, target, confirmed = false)
        }
        try {
            targets.forEach { target ->
                context.startForegroundService(Intent(context, ReceiverService::class.java)
                    .setAction(ReceiverService.ACTION_STOP_PEER).putExtra("address", target))
            }
        } catch (_: Exception) {
            targets.forEach { removePeer(context, it) }
            if (savedPeers(context).isEmpty()) stop(context)
        }
    }

    internal const val STOP_UNCONFIRMED = "이 기기는 중지했어요. 상대 기기의 중지 여부를 확인하지 못했어요. 상대 기기에서도 직접 중지해 주세요."

    internal fun stopNotice(context: Context): String? = prefs(context).getString("stop_notice", null)

    fun stop(context: Context) {
        disable(context)
        context.stopService(Intent(context, BatteryService::class.java))
        context.stopService(Intent(context, ReceiverService::class.java))
    }
}
