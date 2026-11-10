package ae.heej.nearlink

/** A small margin lets the peer's independently timed acceptance window expire. */
internal object RecoveryTiming {
    private const val COOLDOWN_MS = 600_000L
    private const val PEER_MARGIN_MS = 5_000L

    fun canAccept(now: Long, lastAccepted: Long): Boolean =
        lastAccepted == 0L || now - lastAccepted >= COOLDOWN_MS

    fun outgoingDelay(now: Long, lastAttempt: Long): Long =
        if (lastAttempt == 0L) 0L else (lastAttempt + COOLDOWN_MS + PEER_MARGIN_MS - now).coerceAtLeast(0L)
}
