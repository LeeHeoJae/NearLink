package ae.heej.nearlink

import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

enum class SendState { STOPPED, STARTING, ADVERTISING, WAITING_FOR_BLUETOOTH, ERROR }
enum class ReceiveState { STOPPED, SEARCHING, CONNECTING, READING, CONNECTED, RETRYING, WAITING_FOR_BLUETOOTH, ERROR }

data class LastReading(val peerId: String, val peerName: String, val level: Int, val timeMillis: Long, val charging: Boolean? = null, val plugged: Boolean? = null)
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
)

private fun BridgeSnapshot.withEvent(description: String): BridgeSnapshot =
    copy(events = (events + StatusEvent(System.currentTimeMillis(), description)).takeLast(8))

internal fun BridgeSnapshot.acceptReading(reading: LastReading, connected: Boolean = true): BridgeSnapshot =
    if (peerId != reading.peerId || reading.level !in 0..100) this
    else if (connected) copy(receiveState = ReceiveState.CONNECTED, receiveDetail = null, lastReading = reading)
    else copy(lastReading = reading)

internal fun BridgeSnapshot.visibleReading(): LastReading? =
    lastReading?.takeIf { peerId == null || it.peerId == peerId }

/** Pairing/advertising alone is not onboarding completion; a valid battery read is. */
internal fun BridgeSnapshot.hasCompletedSetup(): Boolean = visibleReading() != null

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
        mutable.update { it.copy(peerId = peerId, peerName = peerName) }
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
            })
        }
    }

    fun receiveStopped() {
        if (mutable.value.receiveState != ReceiveState.ERROR) receive(ReceiveState.STOPPED)
    }

    @Synchronized fun recordReading(context: Context, peerId: String, peerName: String, level: Int, charging: Boolean? = null, plugged: Boolean? = null, connected: Boolean = true) {
        if (level !in 0..100) return
        val reading = LastReading(peerId, peerName, level, System.currentTimeMillis(), charging, plugged)
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
