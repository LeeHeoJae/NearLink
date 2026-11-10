package ae.heej.nearlink

import org.junit.Assert.*
import org.junit.Test

class SessionHandshakeTest {
    private var now = 0L
    private fun server(epoch: Long = 99) = SessionHandshake.Server(epoch) { now }
    private fun packet(kind: Byte, request: Long = 7, epoch: Long = 99) = SessionHandshake.Packet(kind, request, epoch)

    @Test fun packetsFitDefaultMtuAndRejectMalformedVersionsLengthsAndIds() {
        for (kind in 1..3) {
            val original = packet(kind.toByte(), epoch = if (kind == 1) 0 else 99)
            val bytes = SessionHandshake.encode(original)
            assertTrue(bytes.size <= 20)
            assertEquals(original, SessionHandshake.decode(bytes))
            assertNull(SessionHandshake.decode(bytes + 0))
            assertNull(SessionHandshake.decode(bytes.copyOf().also { it[0] = 2 }))
        }
        assertNull(SessionHandshake.decode(SessionHandshake.encode(packet(SessionHandshake.HELLO, request = 0))))
        assertNull(SessionHandshake.decode(SessionHandshake.encode(packet(SessionHandshake.READY, epoch = 0))))
    }

    @Test fun acknowledgementRequiresSamePeerRequestEpochAndSubscriptions() {
        val s = server()
        assertTrue(s.hello("peer", packet(SessionHandshake.HELLO)))
        assertFalse(s.acknowledge("other", packet(SessionHandshake.ACK), true))
        assertFalse(s.acknowledge("peer", packet(SessionHandshake.ACK, request = 8), true))
        assertFalse(s.acknowledge("peer", packet(SessionHandshake.ACK, epoch = 98), true))
        assertFalse(s.acknowledge("peer", packet(SessionHandshake.ACK), false))
        assertTrue(s.acknowledge("peer", packet(SessionHandshake.ACK), true))
        assertTrue(s.acknowledge("peer", packet(SessionHandshake.ACK), true))
    }

    @Test fun expiredPendingAndPreviousServerOrRequestCannotCompleteHandshake() {
        val s = server()
        s.hello("peer", packet(SessionHandshake.HELLO))
        now = 20_000
        assertFalse(s.acknowledge("peer", packet(SessionHandshake.ACK), true))
        assertNull(s.response("peer"))
        s.hello("peer", packet(SessionHandshake.HELLO, request = 8))
        assertFalse(s.acknowledge("peer", packet(SessionHandshake.ACK), true))
        val restarted = server(100)
        restarted.hello("peer", packet(SessionHandshake.HELLO))
        assertFalse(restarted.acknowledge("peer", packet(SessionHandshake.ACK), true))
        s.forget("peer")
        assertNull(s.response("peer"))
    }

    @Test fun connectedStateRequiresWriteSuccessAndNotificationAfterAcknowledgementStarts() {
        val client = SessionHandshake.Client(7)
        assertNull(client.acknowledgement())
        assertFalse(client.acceptReady(SessionHandshake.encode(packet(SessionHandshake.READY, request = 8))))
        assertTrue(client.acceptReady(SessionHandshake.encode(packet(SessionHandshake.READY))))
        client.notified() // Subscription notification before ACK is not proof of completed setup.
        client.acknowledgement()
        client.written()
        assertFalse(client.ready)
        client.notified()
        assertTrue(client.ready)
        assertFalse(client.acceptReady(SessionHandshake.encode(packet(SessionHandshake.READY, epoch = 100))))
    }

    @Test fun notificationBeforeWriteCallbackIsRetainedAndEstablishedSessionSurvivesHeartbeatInterval() {
        val client = SessionHandshake.Client(7)
        val s = server()
        s.hello("peer", SessionHandshake.decode(client.hello())!!)
        assertTrue(client.acceptReady(s.response("peer")!!))
        assertTrue(s.acknowledge("peer", SessionHandshake.decode(client.acknowledgement()!!)!!, true))
        client.notified()
        assertFalse(client.ready)
        client.written()
        assertTrue(client.ready)
        now = 60_000
        assertTrue(client.acceptReady(s.response("peer")!!))
    }
}
