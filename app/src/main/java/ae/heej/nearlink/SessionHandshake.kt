package ae.heej.nearlink

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Versioned packets fit the minimum ATT payload (20 bytes). Authentication stays in GATT. */
internal object SessionHandshake {
    private const val VERSION: Byte = 1
    const val HELLO: Byte = 1
    const val READY: Byte = 2
    const val ACK: Byte = 3
    data class Packet(val kind: Byte, val request: Long, val epoch: Long = 0)

    fun encode(packet: Packet): ByteArray = ByteBuffer.allocate(if (packet.kind == HELLO) 10 else 18)
        .order(ByteOrder.LITTLE_ENDIAN).put(VERSION).put(packet.kind).putLong(packet.request)
        .apply { if (packet.kind != HELLO) putLong(packet.epoch) }.array()

    fun decode(value: ByteArray): Packet? {
        if (value.size != 10 && value.size != 18) return null
        val b = ByteBuffer.wrap(value).order(ByteOrder.LITTLE_ENDIAN)
        if (b.get() != VERSION) return null
        val kind = b.get()
        if (kind !in HELLO..ACK || value.size != if (kind == HELLO) 10 else 18) return null
        return Packet(kind, b.long, if (kind == HELLO) 0 else b.long).takeIf {
            it.request != 0L && (kind == HELLO || it.epoch != 0L)
        }
    }

    class Client(val request: Long) {
        private var epoch: Long? = null
        private var ackStarted = false
        private var ackWritten = false
        private var notification = false
        val ready: Boolean get() = ackWritten && notification
        fun hello(): ByteArray = encode(Packet(HELLO, request))
        fun acceptReady(value: ByteArray): Boolean {
            val packet = decode(value) ?: return false
            if (packet.kind != READY || packet.request != request || (epoch != null && epoch != packet.epoch)) return false
            epoch = packet.epoch
            return true
        }
        fun acknowledgement(): ByteArray? {
            val current = epoch ?: return null
            ackStarted = true
            return encode(Packet(ACK, request, current))
        }
        fun written() { if (ackStarted) ackWritten = true }
        fun notified() { if (ackStarted) notification = true }
    }

    class Server(val epoch: Long, private val now: () -> Long) {
        private data class Pending(val request: Long, val expires: Long, var acknowledged: Boolean = false)
        private val peers = mutableMapOf<String, Pending>()
        @Synchronized fun hello(peer: String, packet: Packet): Boolean {
            if (packet.kind != HELLO || packet.request == 0L) return false
            peers[peer] = Pending(packet.request, now() + 20_000)
            return true
        }
        @Synchronized fun response(peer: String): ByteArray? {
            val pending = peers[peer] ?: return null
            if (!pending.acknowledged && now() >= pending.expires) { peers.remove(peer); return null }
            return encode(Packet(READY, pending.request, epoch))
        }
        @Synchronized fun acknowledge(peer: String, packet: Packet, subscribed: Boolean): Boolean {
            val pending = peers[peer] ?: return false
            if (!subscribed || packet.kind != ACK || packet.request != pending.request || packet.epoch != epoch ||
                (!pending.acknowledged && now() >= pending.expires)) return false
            pending.acknowledged = true
            return true
        }
        @Synchronized fun forget(peer: String) { peers.remove(peer) }
    }
}
