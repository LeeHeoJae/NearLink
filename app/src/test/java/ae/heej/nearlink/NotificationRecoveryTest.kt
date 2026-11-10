package ae.heej.nearlink

import org.junit.Assert.*
import org.junit.Test

class NotificationRecoveryTest {
    @Test fun transientTimeoutsAllowLateCallbackBeforeRecreatingServer() {
        val recovery = NotificationRecovery()
        assertEquals(15_000L, recovery.timeoutDelay())
        assertEquals(30_000L, recovery.timeoutDelay())
        assertNull(recovery.timeoutDelay())
        recovery.success()
        assertEquals(15_000L, recovery.timeoutDelay())
    }
    @Test fun busyRetriesBackOffAndSuccessResetsBudget() {
        val recovery = NotificationRecovery()
        assertEquals(listOf(1000L, 2000L, 4000L, 8000L, 16000L, 30000L, 30000L),
            List(7) { recovery.failureDelay() })
        recovery.success()
        assertEquals(1000L, recovery.failureDelay())
    }
}
