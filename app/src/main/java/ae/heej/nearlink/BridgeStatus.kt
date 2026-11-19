package ae.heej.nearlink

import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

enum class SendState { STOPPED, STARTING, ADVERTISING, WAITING_FOR_BLUETOOTH, ERROR }
enum class PreparationState { IDLE, OPEN, ACCEPTED, EXPIRED }
enum class ReceiveState { STOPPED, SEARCHING, CONNECTING, READING, CONNECTED, RETRYING, WAITING_FOR_BLUETOOTH, ERROR, SEND_ONLY }

data class LastReading(val peerId: String, val peerName: String, val level: Int, val timeMillis: Long, val charging: Boolean? = null, val plugged: Boolean? = null)
data class PeerSnapshot(
    val peerId: String,
    /** Empty until the Bluetooth name is known; shown as a localized placeholder meanwhile. */
    val peerName: String = "",
    val receiveState: ReceiveState = ReceiveState.STOPPED,
    val detail: UiText? = null,
    val lastReading: LastReading? = null,
    val sendOnly: Boolean = false,
    /** A name shown only in this app; Bluetooth and the Samsung widget keep the device name. */
    val alias: String? = null,
) {
    val displayName: String get() = alias ?: peerName
    fun displayText(): UiText = displayName.takeIf { it.isNotBlank() }?.let { UiText.Raw(it) } ?: ui(R.string.selected_device)
}
data class StatusEvent(val timeMillis: Long, val description: UiText)

/** Relative reception age for quick scanning; the exact time stays available alongside it. */
internal fun relativeAge(now: Long, then: Long): UiText {
    val minutes = (now - then).coerceAtLeast(0L) / 60_000L
    return when {
        minutes < 1 -> ui(R.string.age_just_now)
        minutes < 60 -> ui(R.string.age_minutes, minutes)
        minutes < 24 * 60 -> ui(R.string.age_hours, minutes / 60)
        minutes < 48 * 60 -> ui(R.string.age_yesterday)
        else -> UiText.Plural(R.plurals.age_days, (minutes / (24 * 60)).toInt())
    }
}
data class BridgeSnapshot(
    val sendState: SendState = SendState.STOPPED,
    val sendDetail: UiText? = null,
    val receiveState: ReceiveState = ReceiveState.STOPPED,
    val receiveDetail: UiText? = null,
    val peerId: String? = null,
    val peerName: String? = null,
    val lastReading: LastReading? = null,
    val events: List<StatusEvent> = emptyList(),
    val peers: Map<String, PeerSnapshot> = emptyMap(),
    val preparationState: PreparationState = PreparationState.IDLE,
    val preparationUntil: Long = 0L,
)

/** Uses the service's monotonic deadline, including time spent outside the activity. */
internal fun BridgeSnapshot.preparationSecondsRemaining(now: Long): Long =
    if (preparationState == PreparationState.OPEN) ((preparationUntil - now).coerceAtLeast(0L) + 999L) / 1000L else 0L

private fun BridgeSnapshot.withEvent(description: UiText): BridgeSnapshot =
    copy(events = (events + StatusEvent(System.currentTimeMillis(), description)).takeLast(20))

internal fun peerStateText(state: ReceiveState): UiText = ui(when (state) {
    ReceiveState.STOPPED -> R.string.peer_state_stopped
    ReceiveState.SEARCHING -> R.string.peer_state_searching
    ReceiveState.CONNECTING -> R.string.peer_state_connecting
    ReceiveState.READING -> R.string.peer_state_reading
    ReceiveState.CONNECTED -> R.string.peer_state_connected
    ReceiveState.RETRYING -> R.string.peer_state_retrying
    ReceiveState.WAITING_FOR_BLUETOOTH -> R.string.peer_state_waiting_bt
    ReceiveState.ERROR -> R.string.peer_state_error
    ReceiveState.SEND_ONLY -> R.string.peer_state_send_only
})

private fun sendEventText(state: SendState): UiText = ui(R.string.event_send, ui(when (state) {
    SendState.STOPPED -> R.string.send_state_stopped
    SendState.STARTING -> R.string.send_state_starting
    SendState.ADVERTISING -> R.string.send_state_advertising
    SendState.WAITING_FOR_BLUETOOTH -> R.string.send_state_waiting_bt
    SendState.ERROR -> R.string.send_state_error
}))

private fun receiveEventText(state: ReceiveState): UiText = ui(R.string.event_receive, ui(when (state) {
    ReceiveState.STOPPED -> R.string.recv_event_stopped
    ReceiveState.SEARCHING -> R.string.recv_event_searching
    ReceiveState.CONNECTING -> R.string.recv_event_connecting
    ReceiveState.READING -> R.string.recv_event_reading
    ReceiveState.CONNECTED -> R.string.recv_event_connected
    ReceiveState.RETRYING -> R.string.recv_event_retrying
    ReceiveState.WAITING_FOR_BLUETOOTH -> R.string.recv_event_waiting_bt
    ReceiveState.ERROR -> R.string.recv_event_error
    ReceiveState.SEND_ONLY -> R.string.recv_event_send_only
}))

/** Records each device's own state change so multi-device history stays attributable. */
internal fun BridgeSnapshot.withPeerState(peerId: String, state: ReceiveState, detail: UiText?): BridgeSnapshot {
    if (peerId !in peers && state == ReceiveState.STOPPED) return this
    val old = peers[peerId] ?: PeerSnapshot(peerId)
    val next = copy(peers = peers + (peerId to old.copy(receiveState = state, detail = detail)))
    return if (old.receiveState == state) next else next.withEvent(ui(R.string.event_peer_state, old.displayText(), peerStateText(state)))
}

internal fun BridgeSnapshot.withoutPeer(peerId: String): BridgeSnapshot = copy(
    peers = peers - peerId,
    peerId = this.peerId?.takeIf { it != peerId },
    peerName = if (this.peerId == peerId) null else peerName,
    lastReading = lastReading?.takeIf { it.peerId != peerId },
)

internal fun BridgeSnapshot.acceptReading(reading: LastReading, connected: Boolean = true): BridgeSnapshot =
    if (peerId != reading.peerId || reading.level !in 0..100) this
    else if (connected) copy(receiveState = ReceiveState.CONNECTED, receiveDetail = null, lastReading = reading)
    else copy(lastReading = reading)

internal fun BridgeSnapshot.visibleReading(): LastReading? =
    lastReading?.takeIf { peerId == null || it.peerId == peerId }

internal fun BridgeSnapshot.withPeerReading(reading: LastReading, connected: Boolean): BridgeSnapshot {
    if (reading.level !in 0..100) return this
    val peer = peers[reading.peerId] ?: PeerSnapshot(reading.peerId, reading.peerName)
    return copy(peers = peers + (reading.peerId to peer.copy(peerName = reading.peerName, lastReading = reading,
        receiveState = if (connected) ReceiveState.CONNECTED else peer.receiveState)))
}

/** Receiving needs a battery read; an explicitly registered send-only peer needs no reverse read. */
internal fun BridgeSnapshot.hasCompletedSetup(): Boolean = visibleReading() != null || peers.values.any { it.lastReading != null || it.sendOnly }

/** Services and the activity run in the same process; only the historical reading is persisted. */
object BridgeStatus {
    private val mutable = MutableStateFlow(BridgeSnapshot())
    val state = mutable.asStateFlow()
    private var initialized = false

    @Synchronized fun initialize(context: Context) {
        if (initialized) return
        val prefs = context.applicationContext.getSharedPreferences("bridge_status", Context.MODE_PRIVATE)
        val peerId = prefs.getString("peer_id", null)
        val name = prefs.getString("peer_name", null)
        val level = prefs.getInt("level", -1)
        val time = prefs.getLong("time", 0)
        if (peerId != null && name != null && level in 0..100 && time > 0) {
            mutable.update { it.copy(lastReading = LastReading(peerId, name, level, time,
                if (prefs.contains("charging")) prefs.getBoolean("charging", false) else null,
                if (prefs.contains("plugged")) prefs.getBoolean("plugged", false) else null)) }
        }
        val histories = context.applicationContext.getSharedPreferences("bridge_peer_readings", Context.MODE_PRIVATE)
        val addresses = BridgeSession.savedPeers(context) + listOfNotNull(peerId) + histories.all.keys.mapNotNull { key ->
            when {
                key.endsWith(".level") -> key.removeSuffix(".level")
                key.endsWith(".stop_unconfirmed") -> key.removeSuffix(".stop_unconfirmed")
                else -> null
            }
        }
        addresses.forEach { address ->
            val storedName = histories.getString("$address.name", null)
            val storedLevel = histories.getInt("$address.level", -1)
            val storedTime = histories.getLong("$address.time", 0)
            val old = mutable.value.lastReading?.takeIf { it.peerId == address }
            val reading = if (storedName != null && storedLevel in 0..100 && storedTime > 0)
                LastReading(address, storedName, storedLevel, storedTime,
                    if (histories.contains("$address.charging")) histories.getBoolean("$address.charging", false) else null,
                    if (histories.contains("$address.plugged")) histories.getBoolean("$address.plugged", false) else null)
                else old
            mutable.update { it.copy(peers = it.peers + (address to PeerSnapshot(address, reading?.peerName ?: "", lastReading = reading,
                detail = if (histories.getBoolean("$address.stop_unconfirmed", false)) ui(R.string.stop_unconfirmed) else null,
                sendOnly = BridgeSession.sendsOnlyTo(context, address),
                alias = histories.getString("$address.alias", null)))) }
            if (BridgeSession.contains(context, address) && BridgeSession.sendsOnlyTo(context, address)) {
                sendOnly(context, address, histories.getString("$address.name", null) ?: "Windows PC")
            }
        }
        BridgeSession.stopNotice(context)?.let { notice -> mutable.update { it.copy(sendDetail = UiText.Raw(notice)) } }
        initialized = true
    }

    fun send(state: SendState, detail: UiText? = null) {
        mutable.update {
            val next = it.copy(sendState = state, sendDetail = detail)
            if (it.sendState == state && it.sendDetail == detail) next
            else next.withEvent(sendEventText(state))
        }
    }

    fun sendStopped() {
        if (mutable.value.sendState != SendState.ERROR && mutable.value.sendState != SendState.STOPPED) send(SendState.STOPPED)
    }

    fun preparation(state: PreparationState, until: Long = 0L) {
        mutable.update { it.copy(preparationState = state, preparationUntil = until) }
    }

    @Synchronized fun selectPeer(peerId: String, peerName: String) {
        mutable.update { snapshot ->
            val peer = (snapshot.peers[peerId] ?: PeerSnapshot(peerId)).copy(peerName = peerName, sendOnly = false)
            snapshot.copy(peerId = peerId, peerName = peerName, peers = snapshot.peers + (peerId to peer))
        }
    }

    fun receivePeer(peerId: String, state: ReceiveState, detail: UiText? = null) {
        mutable.update { it.withPeerState(peerId, state, detail) }
    }

    /** Blank clears the alias. The name sent over Bluetooth and shown by Samsung is unchanged. */
    @Synchronized fun rename(context: Context, peerId: String, alias: String) {
        val cleaned = alias.filterNot { it.isISOControl() }.trim().take(48).ifEmpty { null }
        context.applicationContext.getSharedPreferences("bridge_peer_readings", Context.MODE_PRIVATE).edit {
            if (cleaned == null) remove("$peerId.alias") else putString("$peerId.alias", cleaned)
        }
        mutable.update { snapshot ->
            val peer = snapshot.peers[peerId] ?: return@update snapshot
            snapshot.copy(peers = snapshot.peers + (peerId to peer.copy(alias = cleaned)))
        }
    }

    /** Removes a stopped device and its stored readings. Bluetooth pairing is kept. */
    @Synchronized fun forget(context: Context, peerId: String) {
        BridgeSession.forget(context, peerId)
        val app = context.applicationContext
        app.getSharedPreferences("bridge_peer_readings", Context.MODE_PRIVATE).edit {
            listOf("name", "level", "time", "charging", "plugged", "stop_unconfirmed", "alias").forEach { remove("$peerId.$it") }
        }
        val status = app.getSharedPreferences("bridge_status", Context.MODE_PRIVATE)
        if (status.getString("peer_id", null).equals(peerId, ignoreCase = true)) status.edit { clear() }
        PeerAlerts.forget(peerId)
        mutable.update { it.withoutPeer(peerId) }
    }

    fun sendOnly(context: Context, peerId: String, peerName: String) {
        val histories = context.applicationContext.getSharedPreferences("bridge_peer_readings", Context.MODE_PRIVATE)
        if (histories.getString("$peerId.name", null) != peerName) {
            histories.edit().putString("$peerId.name", peerName).apply()
        }
        mutable.update { snapshot ->
            val peer = (snapshot.peers[peerId] ?: PeerSnapshot(peerId)).copy(peerName = peerName,
                receiveState = ReceiveState.SEND_ONLY, sendOnly = true, lastReading = null, detail = null)
            snapshot.copy(peers = snapshot.peers + (peerId to peer))
        }
    }

    fun peerStopped(context: Context, peerId: String, confirmed: Boolean) {
        // A late stop callback must not bring back a device the user already removed.
        if (peerId !in mutable.value.peers) return
        context.applicationContext.getSharedPreferences("bridge_peer_readings", Context.MODE_PRIVATE).edit {
            if (confirmed) remove("$peerId.stop_unconfirmed") else putBoolean("$peerId.stop_unconfirmed", true)
        }
        receivePeer(peerId, ReceiveState.STOPPED, ui(if (confirmed) R.string.peer_stopped_confirmed else R.string.stop_unconfirmed))
    }

    fun receive(state: ReceiveState, detail: UiText? = null) {
        mutable.update {
            val next = it.copy(receiveState = state, receiveDetail = detail)
            if (it.receiveState == state && it.receiveDetail == detail) next
            else next.withEvent(receiveEventText(state))
        }
    }

    fun receiveStopped() {
        if (mutable.value.receiveState != ReceiveState.ERROR) receive(ReceiveState.STOPPED)
    }

    @Synchronized fun recordReading(context: Context, peerId: String, peerName: String, level: Int, charging: Boolean? = null, plugged: Boolean? = null, connected: Boolean = true) {
        if (level !in 0..100) return
        val reading = LastReading(peerId, peerName, level, System.currentTimeMillis(), charging, plugged)
        mutable.update { it.withPeerReading(reading, connected) }
        context.applicationContext.getSharedPreferences("bridge_peer_readings", Context.MODE_PRIVATE).edit {
            putString("$peerId.name", peerName)
            putInt("$peerId.level", level)
            putLong("$peerId.time", reading.timeMillis)
            if (charging == null) remove("$peerId.charging") else putBoolean("$peerId.charging", charging)
            if (plugged == null) remove("$peerId.plugged") else putBoolean("$peerId.plugged", plugged)
        }
        // Only live readings may alert; a restored or initial value is not news.
        if (connected) PeerAlerts.onReading(context, peerId, mutable.value.peers[peerId]?.displayName ?: peerName, level, charging)
        if (mutable.value.peerId != peerId) return
        mutable.update {
            val next = it.acceptReading(reading, connected)
            if (next === it || !connected || it.receiveState == ReceiveState.CONNECTED) next else next.withEvent(receiveEventText(ReceiveState.CONNECTED))
        }
        context.applicationContext.getSharedPreferences("bridge_status", Context.MODE_PRIVATE).edit {
            putString("peer_id", peerId)
            putString("peer_name", peerName)
            putInt("level", level)
            putLong("time", reading.timeMillis)
            if (charging == null) remove("charging") else putBoolean("charging", charging)
            if (plugged == null) remove("plugged") else putBoolean("plugged", plugged)
        }
    }
}
