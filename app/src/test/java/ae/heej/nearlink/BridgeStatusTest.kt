package ae.heej.nearlink

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.assertFalse
import org.junit.Test

class BridgeStatusTest {
    private val reading = LastReading("first", "첫 기기", 65, 1234)

    @Test fun serviceCleanupKeepsUnconfirmedStopNoticeVisible() {
        BridgeStatus.send(SendState.STOPPED, "중지 여부 확인 필요")
        BridgeStatus.sendStopped()
        assertEquals("중지 여부 확인 필요", BridgeStatus.state.value.sendDetail)
        BridgeStatus.send(SendState.STARTING)
        assertNull(BridgeStatus.state.value.sendDetail)
    }

    @Test fun readingIsAcceptedOnlyForSelectedPeer() {
        val selected = BridgeSnapshot(peerId = "first", receiveState = ReceiveState.READING)
        val result = selected.acceptReading(reading)
        assertEquals(ReceiveState.CONNECTED, result.receiveState)
        assertEquals(reading, result.visibleReading())

        val switched = result.copy(peerId = "second", receiveState = ReceiveState.SEARCHING)
        assertNull(switched.visibleReading())
        assertEquals(switched, switched.acceptReading(reading))
    }

    @Test fun invalidBatteryValueDoesNotReplaceLastReading() {
        val connected = BridgeSnapshot(peerId = "first", lastReading = reading)
        assertEquals(connected, connected.acceptReading(reading.copy(level = 255)))
        assertEquals(connected, connected.acceptReading(reading.copy(level = -1)))
    }

    @Test fun historicalReadingCanBeShownWithoutAnActivePeer() {
        val stopped = BridgeSnapshot(lastReading = reading)
        assertEquals(ReceiveState.STOPPED, stopped.receiveState)
        assertEquals(reading, stopped.visibleReading())
    }

    @Test fun preparationAndPairingDoNotCompleteSetup() {
        assertFalse(BridgeSnapshot().hasCompletedSetup())
        ReceiveState.entries.forEach { state ->
            assertFalse(BridgeSnapshot(sendState = SendState.ADVERTISING, peerId = "first", receiveState = state).hasCompletedSetup())
        }
    }

    @Test fun firstReadCompletesSetupAndSurvivesDisconnectOrStop() {
        val connected = BridgeSnapshot(peerId = "first").acceptReading(reading)
        assertTrue(connected.hasCompletedSetup())
        ReceiveState.entries.forEach { state ->
            assertTrue(connected.copy(receiveState = state).hasCompletedSetup())
        }
    }

    @Test fun restoredHistoryCompletesSetupWithoutEnablingServices() {
        val restored = BridgeSnapshot(lastReading = reading)
        assertTrue(restored.hasCompletedSetup())
        assertEquals(ReceiveState.STOPPED, restored.receiveState)
        assertEquals(SendState.STOPPED, restored.sendState)
    }

    @Test fun switchingPeerRequiresANewReadBeforeManagement() {
        val switched = BridgeSnapshot(peerId = "second", lastReading = reading, receiveState = ReceiveState.SEARCHING)
        assertFalse(switched.hasCompletedSetup())
        assertFalse(switched.acceptReading(reading).hasCompletedSetup())
        val received = switched.acceptReading(reading.copy(peerId = "second", peerName = "새 기기"))
        assertTrue(received.hasCompletedSetup())
        assertEquals("새 기기", received.visibleReading()?.peerName)
    }
}
