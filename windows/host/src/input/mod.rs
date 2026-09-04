#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;
/// USB HID keyboard page 0x07 -> Windows scan code set 1. High bit encodes E0.
pub fn scan_code(hid: u16) -> Option<u16> {
    const LETTERS: [u16; 26] = [
        0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18,
        0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
    ];
    match hid {
        4..=29 => Some(LETTERS[(hid - 4) as usize]),
        30..=38 => Some(hid - 28),
        39 => Some(0x0b),
        40 => Some(0x1c),
        41 => Some(0x01),
        42 => Some(0x0e),
        43 => Some(0x0f),
        44 => Some(0x39),
        45 => Some(0x0c),
        46 => Some(0x0d),
        47 => Some(0x1a),
        48 => Some(0x1b),
        49 => Some(0x2b),
        51 => Some(0x27),
        52 => Some(0x28),
        53 => Some(0x29),
        54 => Some(0x33),
        55 => Some(0x34),
        56 => Some(0x35),
        57 => Some(0x3a),
        58..=67 => Some(hid + 1),
        68 => Some(0x57),
        69 => Some(0x58),
        73 => Some(0x152),
        74 => Some(0x147),
        75 => Some(0x149),
        76 => Some(0x153),
        77 => Some(0x14f),
        78 => Some(0x151),
        79 => Some(0x14d),
        80 => Some(0x14b),
        81 => Some(0x150),
        82 => Some(0x148),
        83 => Some(0x45),
        84 => Some(0x135),
        85 => Some(0x37),
        86 => Some(0x4a),
        87 => Some(0x4e),
        88 => Some(0x11c),
        89..=97 => {
            Some([0x4f, 0x50, 0x51, 0x4b, 0x4c, 0x4d, 0x47, 0x48, 0x49][(hid - 89) as usize])
        }
        98 => Some(0x52),
        99 => Some(0x53),
        224 => Some(0x1d),
        225 => Some(0x2a),
        226 => Some(0x38),
        227 => Some(0x15b),
        228 => Some(0x11d),
        229 => Some(0x36),
        230 => Some(0x138),
        231 => Some(0x15c),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_keys_and_modifiers() {
        assert_eq!(scan_code(4), Some(0x1e));
        assert_eq!(scan_code(79), Some(0x14d));
        assert_eq!(scan_code(228), Some(0x11d));
        assert_eq!(scan_code(65535), None);
    }
}
