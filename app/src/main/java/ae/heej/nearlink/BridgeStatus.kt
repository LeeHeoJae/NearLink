package ae.heej.nearlink

import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

enum class SendState { STOPPED, STARTING, ADVERTISING, WAITING_FOR_BLUETOOTH, ERROR }
enum class ReceiveState { STOPPED, SEARCHING, CONNECTING, READING, CONNECTED, RETRYING, WAITING_FOR_BLUETOOTH, ERROR, SEND_ONLY }

data class LastReading(val peerId: String, val peerName: String, val level: Int, val timeMillis: Long, val charging: Boolean? = null, val plugged: Boolean? = null)
data class PeerSnapshot(
    val peerId: String,
    val peerName: String = "선택한 기기",
    val receiveState: ReceiveState = ReceiveState.STOPPED,
    val detail: String? = null,
    val lastReading: LastReading? = null,
    val sendOnly: Boolean = false,
)
data class StatusEvent(val timeMillis: Long, val description: String)
data class BridgeSnapshot(
    val sendState: SendState = SendState.STOPPED,
    val sendDetail: String? = null,
    val receiveState: ReceiveState = ReceiveState.STOPPED,
    val receiveDetail: String? = null,
    val peerId: String? = null,
    val peerName: String? = null,
    val lastReading: LastReading? = null,
    val events: List<StatusEvent> = emptyList(),
    val peers: Map<String, PeerSnapshot> = emptyMap(),
)

private fun BridgeSnapshot.withEvent(description: String): BridgeSnapshot =
    copy(events = (events + StatusEvent(System.currentTimeMillis(), description)).takeLast(8))

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
            mutable.update { it.copy(peers = it.peers + (address to PeerSnapshot(address, reading?.peerName ?: "선택한 기기", lastReading = reading,
                detail = if (histories.getBoolean("$address.stop_unconfirmed", false)) BridgeSession.STOP_UNCONFIRMED else null,
                sendOnly = BridgeSession.sendsOnlyTo(context, address)))) }
            if (BridgeSession.contains(context, address) && BridgeSession.sendsOnlyTo(context, address)) {
                sendOnly(context, address, histories.getString("$address.name", null) ?: "Windows PC")
            }
        }
        BridgeSession.stopNotice(context)?.let { notice -> mutable.update { it.copy(sendDetail = notice) } }
        initialized = true
    }

    fun send(state: SendState, detail: String? = null) {
        mutable.update {
            val next = it.copy(sendState = state, sendDetail = detail)
            if (it.sendState == state && it.sendDetail == detail) next
            else next.withEvent("보내기 · " + when (state) {
                SendState.STOPPED -> "중지"
                SendState.STARTING -> "준비 중"
                SendState.ADVERTISING -> "준비 완료"
                SendState.WAITING_FOR_BLUETOOTH -> "블루투스 대기"
                SendState.ERROR -> "오류"
            })
        }
    }

    fun sendStopped() {
        if (mutable.value.sendState != SendState.ERROR && mutable.value.sendState != SendState.STOPPED) send(SendState.STOPPED)
    }

    @Synchronized fun selectPeer(peerId: String, peerName: String) {
        mutable.update { snapshot ->
            val peer = (snapshot.peers[peerId] ?: PeerSnapshot(peerId)).copy(peerName = peerName, sendOnly = false)
            snapshot.copy(peerId = peerId, peerName = peerName, peers = snapshot.peers + (peerId to peer))
        }
    }

    fun receivePeer(peerId: String, state: ReceiveState, detail: String? = null) {
        mutable.update { snapshot ->
            val peer = (snapshot.peers[peerId] ?: PeerSnapshot(peerId)).copy(receiveState = state, detail = detail)
            snapshot.copy(peers = snapshot.peers + (peerId to peer))
        }
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
        context.applicationContext.getSharedPreferences("bridge_peer_readings", Context.MODE_PRIVATE).edit {
            if (confirmed) remove("$peerId.stop_unconfirmed") else putBoolean("$peerId.stop_unconfirmed", true)
        }
        receivePeer(peerId, ReceiveState.STOPPED, if (confirmed) "연결을 중지했어요." else BridgeSession.STOP_UNCONFIRMED)
    }

    fun receive(state: ReceiveState, detail: String? = null) {
        mutable.update {
            val next = it.copy(receiveState = state, receiveDetail = detail)
            if (it.receiveState == state && it.receiveDetail == detail) next
            else next.withEvent("받기 · " + when (state) {
                ReceiveState.STOPPED -> "중지"
                ReceiveState.SEARCHING -> "기기 검색"
                ReceiveState.CONNECTING -> "연결 중"
                ReceiveState.READING -> "배터리 확인 중"
                ReceiveState.CONNECTED -> "읽기 성공"
                ReceiveState.RETRYING -> "재시도 대기"
                ReceiveState.WAITING_FOR_BLUETOOTH -> "블루투스 대기"
                ReceiveState.ERROR -> "오류"
                ReceiveState.SEND_ONLY -> "송신 전용"
            })
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
        if (mutable.value.peerId != peerId) return
        mutable.update {
            val next = it.acceptReading(reading, connected)
            if (next === it || !connected || it.receiveState == ReceiveState.CONNECTED) next else next.withEvent("받기 · 읽기 성공")
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
