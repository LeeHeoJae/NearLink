package ae.heej.nearlink

/** Keep one outstanding send until its callback arrives; never overlap a timed-out send. */
internal class NotificationRecovery {
    private var timeouts = 0
    private var failures = 0
    fun timeoutDelay(): Long? = when (++timeouts) { 1 -> 15_000L; 2 -> 30_000L; else -> null }
    fun failureDelay(): Long = minOf(30_000L, 1_000L shl minOf(failures++, 5))
    fun success() { timeouts = 0; failures = 0 }
    fun clear() { timeouts = 0; failures = 0 }
}
