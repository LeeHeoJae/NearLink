package ae.heej.nearlink

/** Control requests are accepted only on an encrypted, bonded link. */
internal object BridgeControl {
    enum class Command { JOIN, STOP, REJECT }

    fun accept(value: ByteArray, authenticated: Boolean, currentPeer: Boolean, prepared: Boolean): Command {
        if (!authenticated || value.size != 1) return Command.REJECT
        return when (value[0].toInt()) {
            1 -> if (currentPeer || prepared) Command.JOIN else Command.REJECT
            2 -> if (currentPeer) Command.STOP else Command.REJECT
            else -> Command.REJECT
        }
    }
}
