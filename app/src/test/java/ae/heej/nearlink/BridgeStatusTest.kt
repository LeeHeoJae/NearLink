package ae.heej.nearlink

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.assertFalse
import org.junit.Test

class BridgeStatusTest {
    private val reading = LastReading("first", "첫 기기", 65, 1234)

    @Test fun preparationCountdownUsesServiceDeadlineAndExpiresWithExistingPeers() {
        val snapshot = BridgeSnapshot(sendState = SendState.ADVERTISING,
            preparationState = PreparationState.OPEN, preparationUntil = 121_000L,
            peers = mapOf("pc" to PeerSnapshot("pc", sendOnly = true)))
        assertEquals(120L, snapshot.preparationSecondsRemaining(1_000L))
        // Reopening the screen must use elapsed time instead of restarting a local timer.
        assertEquals(30L, snapshot.preparationSecondsRemaining(91_000L))
        assertEquals(1L, snapshot.preparationSecondsRemaining(120_999L))
        assertEquals(0L, snapshot.preparationSecondsRemaining(121_000L))
        assertEquals(0L, snapshot.preparationSecondsRemaining(125_000L))
        assertTrue(snapshot.hasCompletedSetup())
    }

    @Test fun closedPreparationNeverShowsTimeEvenIfDeadlineWasRetained() {
        PreparationState.entries.filter { it != PreparationState.OPEN }.forEach { state ->
            assertEquals(0L, BridgeSnapshot(preparationState = state, preparationUntil = 121_000L)
                .preparationSecondsRemaining(1_000L))
        }
    }

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

    @Test fun readingBeforeSetupCompletionDoesNotClaimConnection() {
        val pending = BridgeSnapshot(peerId = "first", receiveState = ReceiveState.READING)
        val result = pending.acceptReading(reading, connected = false)
        assertEquals(ReceiveState.READING, result.receiveState)
        assertEquals(reading, result.lastReading)
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
    @Test fun readingsFromDifferentPeersStayIndependent() {
        val second = LastReading("second", "둘째 기기", 24, 5678)
        val snapshot = BridgeSnapshot().withPeerReading(reading, true).withPeerReading(second, false)
        assertEquals(reading, snapshot.peers["first"]?.lastReading)
        assertEquals(ReceiveState.CONNECTED, snapshot.peers["first"]?.receiveState)
        assertEquals(second, snapshot.peers["second"]?.lastReading)
        assertEquals(ReceiveState.STOPPED, snapshot.peers["second"]?.receiveState)
        assertEquals(snapshot, snapshot.withPeerReading(second.copy(level = 101), true))
        assertTrue(snapshot.hasCompletedSetup())
    }

    @Test fun changingOnePeersConnectionDoesNotChangeAnotherPeer() {
        BridgeStatus.selectPeer("isolation-a", "A")
        BridgeStatus.selectPeer("isolation-b", "B")
        BridgeStatus.receivePeer("isolation-a", ReceiveState.CONNECTED)
        BridgeStatus.receivePeer("isolation-b", ReceiveState.RETRYING, "다시 시도")
        assertEquals(ReceiveState.CONNECTED, BridgeStatus.state.value.peers["isolation-a"]?.receiveState)
        assertEquals(ReceiveState.RETRYING, BridgeStatus.state.value.peers["isolation-b"]?.receiveState)
        BridgeStatus.receivePeer("isolation-b", ReceiveState.STOPPED)
        assertEquals(ReceiveState.CONNECTED, BridgeStatus.state.value.peers["isolation-a"]?.receiveState)
    }

    @Test fun registeredSendOnlyPeerDoesNotNeedAReverseBatteryReading() {
        val peer = PeerSnapshot("pc", "Windows PC", receiveState = ReceiveState.SEND_ONLY, sendOnly = true)
        val snapshot = BridgeSnapshot(peers = mapOf("pc" to peer))
        assertTrue(snapshot.hasCompletedSetup())
        assertNull(snapshot.peers["pc"]?.lastReading)
        assertTrue(snapshot.copy(peers = mapOf("pc" to peer.copy(receiveState = ReceiveState.STOPPED))).hasCompletedSetup())
        assertFalse(BridgeSnapshot(peers = mapOf("pc" to PeerSnapshot("pc"))).hasCompletedSetup())
    }
}
