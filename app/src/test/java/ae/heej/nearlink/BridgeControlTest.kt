package ae.heej.nearlink

import org.junit.Assert.assertEquals
import org.junit.Test

class BridgeControlTest {
    @Test fun controlRequestsRequireAuthenticatedBond() {
        for (command in 1..3) {
            assertEquals(BridgeControl.Command.REJECT, BridgeControl.accept(byteArrayOf(command.toByte()), false, true, true))
        }
    }

    @Test fun joiningRequiresCurrentPeerOrActivePreparation() {
        assertEquals(BridgeControl.Command.JOIN, BridgeControl.accept(byteArrayOf(1), true, false, true))
        assertEquals(BridgeControl.Command.JOIN, BridgeControl.accept(byteArrayOf(1), true, true, false))
        assertEquals(BridgeControl.Command.REJECT, BridgeControl.accept(byteArrayOf(1), true, false, false))
    }

    @Test fun onlyCurrentPeerCanStopAndMalformedCommandsAreRejected() {
        assertEquals(BridgeControl.Command.STOP, BridgeControl.accept(byteArrayOf(2), true, true, false))
        assertEquals(BridgeControl.Command.REJECT, BridgeControl.accept(byteArrayOf(2), true, false, true))
        assertEquals(BridgeControl.Command.REJECT, BridgeControl.accept(byteArrayOf(), true, true, true))
        assertEquals(BridgeControl.Command.REJECT, BridgeControl.accept(byteArrayOf(2, 1), true, true, true))
        assertEquals(BridgeControl.Command.REJECT, BridgeControl.accept(byteArrayOf(4), true, true, true))
    }

    @Test fun recoveryRequiresAuthenticatedCurrentPeer() {
        assertEquals(BridgeControl.Command.RECOVER, BridgeControl.accept(byteArrayOf(3), true, true, false))
        assertEquals(BridgeControl.Command.REJECT, BridgeControl.accept(byteArrayOf(3), true, false, true))
        assertEquals(BridgeControl.Command.REJECT, BridgeControl.accept(byteArrayOf(3), false, true, false))
    }
}
