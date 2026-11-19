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
        manager.createNotificationChannel(NotificationChannel(CONNECTION_CHANNEL, context.getString(R.string.channel_connection_name),
            NotificationManager.IMPORTANCE_LOW).apply { description = context.getString(R.string.channel_connection_desc) })
        manager.createNotificationChannel(NotificationChannel(ALERT_CHANNEL, context.getString(R.string.channel_alert_name),
            NotificationManager.IMPORTANCE_DEFAULT).apply { description = context.getString(R.string.channel_alert_desc) })
    }

    private fun openApp(context: Context): PendingIntent = PendingIntent.getActivity(context, 0,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)

    private fun stopAll(context: Context): Notification.Action {
        val intent = PendingIntent.getService(context, 1,
            Intent(context, BatteryService::class.java).setAction(BatteryService.ACTION_STOP_ALL),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        return Notification.Action.Builder(null, context.getString(R.string.stop_all_connections), intent).build()
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
internal fun BridgeSnapshot.receiverSummary(): UiText {
    val connected = peers.values.filter { it.receiveState == ReceiveState.CONNECTED && it.lastReading != null }
        .sortedBy { it.displayName }
    if (connected.isNotEmpty()) return UiText.Raw(connected.joinToString(" · ") { "${it.displayName} ${it.lastReading!!.level}%" })
    val waiting = peers.values.count { it.receiveState != ReceiveState.STOPPED && it.receiveState != ReceiveState.SEND_ONLY }
    return if (waiting > 0) UiText.Plural(R.plurals.summary_waiting, waiting) else ui(R.string.summary_none)
}
