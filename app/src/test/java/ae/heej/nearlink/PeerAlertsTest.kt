package ae.heej.nearlink

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PeerAlertsTest {
    private val settings = PeerAlertSettings()

    @Test fun lowAlertFiresOnceAndRearmsAboveHysteresis() {
        val latch = AlertLatch()
        assertNull(latch.update(19, true, settings))
        assertEquals(PeerAlert.LOW, latch.update(19, false, settings))
        assertNull(latch.update(18, false, settings))
        assertNull(latch.update(25, false, settings))
        latch.update(26, false, settings)
        assertEquals(PeerAlert.LOW, latch.update(20, null, settings))
    }

    @Test fun fullAlertRearmsBelowNinetyFive() {
        val latch = AlertLatch()
        assertEquals(PeerAlert.FULL, latch.update(100, true, settings))
        assertNull(latch.update(100, false, settings))
        latch.update(94, false, settings)
        assertEquals(PeerAlert.FULL, latch.update(100, true, settings))
    }

    @Test fun disabledAlertsStayQuietButStillLatch() {
        val latch = AlertLatch()
        val quiet = settings.copy(low = false, full = false)
        assertNull(latch.update(10, false, quiet))
        assertNull(latch.update(10, false, settings))
    }

    @Test fun receiverSummaryListsConnectedDevicesOnly() {
        val snapshot = BridgeSnapshot(peers = mapOf(
            "a" to PeerSnapshot("a", "태블릿", ReceiveState.CONNECTED, lastReading = LastReading("a", "태블릿", 40, 1)),
            "b" to PeerSnapshot("b", "휴대폰", ReceiveState.CONNECTED, lastReading = LastReading("b", "휴대폰", 85, 1), alias = "업무폰"),
            "c" to PeerSnapshot("c", "꺼진 기기", ReceiveState.RETRYING, lastReading = LastReading("c", "꺼진 기기", 5, 1)),
        ))
        assertEquals("업무폰 85% · 태블릿 40%", snapshot.receiverSummary())
        assertEquals("기기 1대의 연결을 기다리고 있어요.", BridgeSnapshot(peers = mapOf("c" to snapshot.peers["c"]!!)).receiverSummary())
        assertEquals("연결한 기기가 없어요.", BridgeSnapshot().receiverSummary())
    }
}
