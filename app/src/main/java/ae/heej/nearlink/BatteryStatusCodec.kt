package ae.heej.nearlink

/** Bluetooth GSS Battery Level Status (0x2BED), little endian. */
data class BatteryStatusValue(val level: Int?, val charging: Boolean?, val plugged: Boolean?)

object BatteryStatusCodec {
    fun encode(level: Int, chargeState: Int, wired: Boolean, wireless: Boolean): ByteArray {
        require(level in 0..100 && chargeState in 0..3)
        val power = 1 or (if (wired) 2 else 0) or (if (wireless) 8 else 0) or (chargeState shl 5)
        // Only Battery Level Present; unsupported charge level/type/fault remain unknown.
        return byteArrayOf(2, power.toByte(), (power shr 8).toByte(), level.toByte())
    }

    fun decode(value: ByteArray): BatteryStatusValue? {
        if (value.size < 3) return null
        val flags = value[0].toInt() and 255
        val power = (value[1].toInt() and 255) or ((value[2].toInt() and 255) shl 8)
        var offset = 3 + if (flags and 1 != 0) 2 else 0
        if (value.size < offset) return null
        val level = if (flags and 2 != 0) {
            if (value.size <= offset) return null
            (value[offset++].toInt() and 255).also { if (it !in 0..100) return null }
        } else null
        if (flags and 4 != 0 && value.size <= offset) return null
        val present = power and 1 != 0
        val charging = if (!present) null else when ((power shr 5) and 3) {
            1 -> true
            2, 3 -> false
            else -> null
        }
        val wired = (power shr 1) and 3
        val wireless = (power shr 3) and 3
        val plugged = when {
            wired == 1 || wireless == 1 -> true
            wired == 0 && wireless == 0 -> false
            else -> null
        }
        return BatteryStatusValue(if (present) level else null, charging, plugged)
    }
}
