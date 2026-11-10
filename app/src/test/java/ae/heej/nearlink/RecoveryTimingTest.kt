package ae.heej.nearlink

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RecoveryTimingTest {
    @Test fun outgoingChangesWaitUntilThePeerCanAcceptAgain() {
        assertEquals(0L, RecoveryTiming.outgoingDelay(100_000, 0))
        assertEquals(604_000L, RecoveryTiming.outgoingDelay(101_000, 100_000))
        assertEquals(1L, RecoveryTiming.outgoingDelay(704_999, 100_000))
        assertEquals(0L, RecoveryTiming.outgoingDelay(705_000, 100_000))
    }

    @Test fun peerRequestsAreRateLimited() {
        assertTrue(RecoveryTiming.canAccept(100_000, 0))
        assertFalse(RecoveryTiming.canAccept(699_999, 100_000))
        assertTrue(RecoveryTiming.canAccept(700_000, 100_000))
    }
}
