//! Wire-compatible with Android SessionHandshake and BatteryStatusCodec.
use windows::core::GUID;

pub const BAS: GUID = GUID::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb);
pub const LEVEL: GUID = GUID::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb);
pub const STATUS: GUID = GUID::from_u128(0x00002bed_0000_1000_8000_00805f9b34fb);
pub const CONTROL: GUID = GUID::from_u128(0x734b7301_53b1_4cf0_8d2e_d4b5c742b65b);
pub const SESSION: GUID = GUID::from_u128(0x734b7302_53b1_4cf0_8d2e_d4b5c742b65b);
pub const BRIDGE: GUID = GUID::from_u128(0x734b7300_53b1_4cf0_8d2e_d4b5c742b65b);
pub const JOIN_RECEIVER: u8 = 5;

#[derive(Debug, PartialEq)]
pub struct BatteryStatus {
    pub level: Option<u8>,
    pub charging: Option<bool>,
    pub plugged: Option<bool>,
}

pub fn battery_status(bytes: &[u8]) -> Option<BatteryStatus> {
    if bytes.len() < 3 {
        return None;
    }
    let flags = bytes[0];
    let power = u16::from_le_bytes([bytes[1], bytes[2]]);
    let mut offset = 3 + if flags & 1 != 0 { 2 } else { 0 };
    if bytes.len() < offset {
        return None;
    }
    let level = if flags & 2 != 0 {
        let level = *bytes.get(offset)?;
        if level > 100 {
            return None;
        }
        offset += 1;
        Some(level)
    } else {
        None
    };
    if flags & 4 != 0 && bytes.len() <= offset {
        return None;
    }
    let present = power & 1 != 0;
    let charging = if !present {
        None
    } else {
        match (power >> 5) & 3 {
            1 => Some(true),
            2 | 3 => Some(false),
            _ => None,
        }
    };
    let wired = (power >> 1) & 3;
    let wireless = (power >> 3) & 3;
    let plugged = if wired == 1 || wireless == 1 {
        Some(true)
    } else if wired == 0 && wireless == 0 {
        Some(false)
    } else {
        None
    };
    Some(BatteryStatus {
        level: if present { level } else { None },
        charging,
        plugged,
    })
}

pub fn hello(request: u64) -> Vec<u8> {
    let mut packet = vec![1, 1];
    packet.extend_from_slice(&request.to_le_bytes());
    packet
}

pub fn ready(bytes: &[u8], request: u64, previous_epoch: Option<u64>) -> Option<u64> {
    if request == 0 || bytes.len() != 18 || bytes[..2] != [1, 2] {
        return None;
    }
    if u64::from_le_bytes(bytes[2..10].try_into().ok()?) != request {
        return None;
    }
    let epoch = u64::from_le_bytes(bytes[10..18].try_into().ok()?);
    (epoch != 0 && previous_epoch.is_none_or(|old| old == epoch)).then_some(epoch)
}

pub fn ack(request: u64, epoch: u64) -> Vec<u8> {
    let mut packet = vec![1, 3];
    packet.extend_from_slice(&request.to_le_bytes());
    packet.extend_from_slice(&epoch.to_le_bytes());
    packet
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_matches_android_and_rejects_stale_server() {
        assert_eq!(
            hello(0x0807060504030201),
            vec![1, 1, 1, 2, 3, 4, 5, 6, 7, 8]
        );
        let mut packet = ack(7, 9);
        packet[1] = 2;
        assert_eq!(ready(&packet, 7, None), Some(9));
        assert_eq!(ready(&packet, 8, None), None);
        assert_eq!(ready(&packet, 7, Some(10)), None);
        assert_eq!(ready(&packet[..17], 7, None), None);
        packet[0] = 2;
        assert_eq!(ready(&packet, 7, None), None);
        assert_eq!(ack(7, 9).len(), 18);
    }
    #[test]
    fn battery_status_handles_presence_optional_fields_and_unknowns() {
        assert_eq!(
            battery_status(&[2, 35, 0, 73]),
            Some(BatteryStatus {
                level: Some(73),
                charging: Some(true),
                plugged: Some(true)
            })
        );
        assert_eq!(
            battery_status(&[3, 65, 0, 9, 0, 42]).unwrap().level,
            Some(42)
        );
        assert_eq!(battery_status(&[2, 0, 0, 42]).unwrap().level, None);
        assert_eq!(battery_status(&[2, 5, 0, 42]).unwrap().plugged, None);
        for bad in [
            &[2, 1, 0][..],
            &[2, 1, 0, 101],
            &[3, 1, 0, 0],
            &[6, 1, 0, 42],
        ] {
            assert_eq!(battery_status(bad), None);
        }
    }
}
