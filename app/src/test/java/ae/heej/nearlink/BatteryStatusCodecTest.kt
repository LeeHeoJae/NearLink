package ae.heej.nearlink

import org.junit.Assert.*
import org.junit.Test

class BatteryStatusCodecTest {
    @Test fun chargingUsesStandardLittleEndianPowerState() {
        val bytes = BatteryStatusCodec.encode(65, 1, wired = true, wireless = false)
        assertArrayEquals(byteArrayOf(2, 0x23, 0, 65), bytes)
        assertEquals(BatteryStatusValue(65, true, true), BatteryStatusCodec.decode(bytes))
    }

    @Test fun externalPowerDoesNotImplyCharging() {
        assertEquals(BatteryStatusValue(100, false, true),
            BatteryStatusCodec.decode(BatteryStatusCodec.encode(100, 3, true, false)))
        assertEquals(BatteryStatusValue(80, false, true),
            BatteryStatusCodec.decode(BatteryStatusCodec.encode(80, 3, false, true)))
        assertEquals(BatteryStatusValue(50, false, false),
            BatteryStatusCodec.decode(BatteryStatusCodec.encode(50, 2, false, false)))
    }

    @Test fun optionalIdentifierAndUnknownFieldsAreHandled() {
        assertEquals(BatteryStatusValue(42, true, true),
            BatteryStatusCodec.decode(byteArrayOf(3, 0x29, 0, 0x34, 0x12, 42)))
        assertEquals(BatteryStatusValue(null, null, null),
            BatteryStatusCodec.decode(byteArrayOf(0, 0x15, 0)))
        assertEquals(BatteryStatusValue(null, null, false),
            BatteryStatusCodec.decode(byteArrayOf(2, 0, 0, 50)))
    }

    @Test fun malformedPayloadsAreRejected() {
        for (bytes in listOf(byteArrayOf(), byteArrayOf(2, 1), byteArrayOf(2, 1, 0),
            byteArrayOf(3, 1, 0, 1), byteArrayOf(2, 1, 0, 101), byteArrayOf(6, 1, 0, 50))) {
            assertNull(BatteryStatusCodec.decode(bytes))
        }
    }
}
