package ae.heej.nearlink

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build

/** User intent is persisted separately from historical readings and live service state. */
object BridgeSession {
    private fun prefs(context: Context) = context.getSharedPreferences("bridge_session", Context.MODE_PRIVATE)

    fun savedPeer(context: Context): String? = prefs(context).let {
        if (it.getBoolean("enabled", false)) it.getString("address", null) else null
    }

    fun hasPermissions(context: Context): Boolean = if (Build.VERSION.SDK_INT >= 31) {
        listOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_SCAN,
            Manifest.permission.BLUETOOTH_ADVERTISE).all { context.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED }
    } else context.checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED

    fun start(context: Context, address: String) {
        prefs(context).edit().putString("address", address).putBoolean("enabled", true).remove("stop_notice").commit()
        context.startForegroundService(Intent(context, BatteryService::class.java))
        context.startForegroundService(Intent(context, ReceiverService::class.java).putExtra("address", address))
    }

    fun disable(context: Context) {
        prefs(context).edit().putBoolean("enabled", false).apply()
    }

    fun requestStop(context: Context) {
        val hadPeer = savedPeer(context) != null
        prefs(context).edit().putBoolean("enabled", false)
            .apply { if (hadPeer) putString("stop_notice", STOP_UNCONFIRMED) else remove("stop_notice") }.commit()
        if (hadPeer) BridgeStatus.send(SendState.STOPPED, STOP_UNCONFIRMED)
        context.stopService(Intent(context, BatteryService::class.java))
        if (hadPeer) {
            try {
                context.startForegroundService(Intent(context, ReceiverService::class.java).setAction(ReceiverService.ACTION_STOP_PEER))
                return
            } catch (_: Exception) { }
        }
        stop(context)
    }

    internal const val STOP_UNCONFIRMED = "이 기기는 중지했어요. 상대 기기의 중지 여부를 확인하지 못했어요. 상대 기기에서도 직접 중지해 주세요."

    internal fun stopNotice(context: Context): String? = prefs(context).getString("stop_notice", null)

    fun stop(context: Context) {
        disable(context)
        context.stopService(Intent(context, BatteryService::class.java))
        context.stopService(Intent(context, ReceiverService::class.java))
    }
}
