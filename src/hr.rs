//! Parsing of the Bluetooth SIG Heart Rate Measurement characteristic (0x2A37).

/// One heart-rate measurement as sent by the band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measurement {
    pub bpm: u16,
    /// `None` when the sensor does not report skin contact,
    /// otherwise whether the band is currently worn.
    pub contact: Option<bool>,
}

const FLAG_VALUE_U16: u8 = 0b0000_0001;
const FLAG_CONTACT_DETECTED: u8 = 0b0000_0010;
const FLAG_CONTACT_SUPPORTED: u8 = 0b0000_0100;

/// Parses a raw Heart Rate Measurement value.
///
/// Layout: `flags`, then the heart rate as `u8` or little-endian `u16`
/// (flag bit 0). Optional trailing fields (energy expended, RR intervals) are ignored.
pub fn parse(data: &[u8]) -> Option<Measurement> {
    let (&flags, rest) = data.split_first()?;
    let bpm = if flags & FLAG_VALUE_U16 != 0 {
        u16::from_le_bytes([*rest.first()?, *rest.get(1)?])
    } else {
        u16::from(*rest.first()?)
    };
    let contact =
        (flags & FLAG_CONTACT_SUPPORTED != 0).then_some(flags & FLAG_CONTACT_DETECTED != 0);
    Some(Measurement { bpm, contact })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u8_value_without_contact() {
        assert_eq!(
            parse(&[0x00, 72]),
            Some(Measurement {
                bpm: 72,
                contact: None
            })
        );
    }

    #[test]
    fn u16_value() {
        assert_eq!(
            parse(&[0x01, 0x2C, 0x01]),
            Some(Measurement {
                bpm: 300,
                contact: None
            })
        );
    }

    #[test]
    fn contact_bits() {
        assert_eq!(parse(&[0x06, 80]).unwrap().contact, Some(true));
        assert_eq!(parse(&[0x04, 80]).unwrap().contact, Some(false));
        // bit 1 alone is meaningless without "supported"
        assert_eq!(parse(&[0x02, 80]).unwrap().contact, None);
    }

    #[test]
    fn ignores_rr_intervals() {
        assert_eq!(parse(&[0x10, 65, 0x00, 0x04]).unwrap().bpm, 65);
    }

    #[test]
    fn truncated() {
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&[0x00]), None);
        assert_eq!(parse(&[0x01, 70]), None);
    }
}
