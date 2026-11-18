package ae.heej.nearlink

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent

/** One channel per purpose; the old experiment channels are removed on first use. */
internal object Notifications {
    const val CONNECTION_CHANNEL = "connection"
    const val ALERT_CHANNEL = "battery_alerts"
    const val SENDER_ID = 1
    const val RECEIVER_ID = 2

    fun ensureChannels(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        listOf("probe", "receiver").forEach { manager.deleteNotificationChannel(it) }
        manager.createNotificationChannel(NotificationChannel(CONNECTION_CHANNEL, "기기 연결 유지",
            NotificationManager.IMPORTANCE_LOW).apply { description = "배터리를 주고받는 동안 표시돼요." })
        manager.createNotificationChannel(NotificationChannel(ALERT_CHANNEL, "기기 배터리 알림",
            NotificationManager.IMPORTANCE_DEFAULT).apply { description = "연결한 기기의 배터리 부족과 충전 완료를 알려요." })
    }

    private fun openApp(context: Context): PendingIntent = PendingIntent.getActivity(context, 0,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)

    private fun stopAll(context: Context): Notification.Action {
        val intent = PendingIntent.getService(context, 1,
            Intent(context, BatteryService::class.java).setAction(BatteryService.ACTION_STOP_ALL),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        return Notification.Action.Builder(null, "모든 연결 중지", intent).build()
    }

    fun connection(context: Context, title: String, text: String): Notification {
        ensureChannels(context)
        return Notification.Builder(context, CONNECTION_CHANNEL)
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setContentTitle(title)
            .setContentText(text)
            .setContentIntent(openApp(context))
            .addAction(stopAll(context))
            .setOnlyAlertOnce(true)
            .setOngoing(true)
            .build()
    }

    fun update(context: Context, id: Int, notification: Notification) {
        try { context.getSystemService(NotificationManager::class.java).notify(id, notification) }
        catch (_: SecurityException) { }
    }

    fun alert(context: Context, peerId: String, title: String, text: String) {
        ensureChannels(context)
        update(context, 1000 + (peerId.hashCode() and 0xFFFF), Notification.Builder(context, ALERT_CHANNEL)
            .setSmallIcon(android.R.drawable.stat_sys_warning)
            .setContentTitle(title)
            .setContentText(text)
            .setContentIntent(openApp(context))
            .setAutoCancel(true)
            .build())
    }
}

/** Summary for the receiving service: connected devices with their current reading. */
internal fun BridgeSnapshot.receiverSummary(): String {
    val connected = peers.values.filter { it.receiveState == ReceiveState.CONNECTED && it.lastReading != null }
        .sortedBy { it.displayName }
    if (connected.isNotEmpty()) return connected.joinToString(" · ") { "${it.displayName} ${it.lastReading!!.level}%" }
    val waiting = peers.values.count { it.receiveState != ReceiveState.STOPPED && it.receiveState != ReceiveState.SEND_ONLY }
    return if (waiting > 0) "기기 ${waiting}대의 연결을 기다리고 있어요." else "연결한 기기가 없어요."
}
