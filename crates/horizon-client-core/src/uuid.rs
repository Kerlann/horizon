//! UUIDs as vanilla's four-int form, to and from their text form.

pub fn uuid_text(u: [i32; 4]) -> String {
    uuid_text_into(u, &mut [0; 36]).to_owned()
}

/// Canonical lowercase UUID text in caller-owned storage, without allocation.
/// Every four-int bit pattern is valid; only ASCII hex and separators are emitted.
pub fn uuid_text_into(u: [i32; 4], buffer: &mut [u8; 36]) -> &str {
    let mut at = 0;
    for digit in 0..32 {
        if matches!(digit, 8 | 12 | 16 | 20) {
            buffer[at] = b'-';
            at += 1;
        }
        let word = u[digit / 8] as u32;
        let nibble = (word >> (4 * (7 - digit % 8))) & 15;
        buffer[at] = b"0123456789abcdef"[nibble as usize];
        at += 1;
    }
    std::str::from_utf8(buffer).expect("UUID formatter emits only ASCII")
}
pub fn uuid_from_text(s: &str) -> Option<[i32; 4]> {
    let mut hex = [0u8; 32];
    let mut len = 0;
    for b in s.bytes() {
        if b == b'-' {
            continue;
        }
        if !b.is_ascii_hexdigit() || len >= 32 {
            return None;
        }
        hex[len] = b;
        len += 1;
    }
    if len != 32 {
        return None;
    }
    let hex = std::str::from_utf8(&hex).ok()?;
    let mut out = [0i32; 4];
    for (i, v) in out.iter_mut().enumerate() {
        *v = u32::from_str_radix(&hex[i * 8..i * 8 + 8], 16).ok()? as i32;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_from_text_rejects_multibyte_and_non_hex() {
        assert_eq!(uuid_from_text("€€€€€€€€€€aa"), None);
        assert_eq!(uuid_from_text("€€€€€€€€€€aa----"), None);
        assert_eq!(uuid_from_text("+0000001-0000-0000-0000-000000000000"), None);
        assert_eq!(uuid_from_text("00000000-0000-0000-0000-00000000000g"), None);
        assert_eq!(uuid_from_text(""), None);
    }

    #[test]
    fn canonical_uuid_text_uses_all_bits_and_overwrites_the_buffer() {
        let mut buffer = [b'?'; 36];
        assert_eq!(
            uuid_text_into([i32::MIN, -1, 0x01234567, -0x76543211], &mut buffer),
            "80000000-ffff-ffff-0123-456789abcdef"
        );
        assert_eq!(
            uuid_text_into([0, 1, 2, 3], &mut buffer),
            "00000000-0000-0001-0000-000200000003"
        );
        assert_eq!(
            uuid_text([i32::MAX; 4]),
            "7fffffff-7fff-ffff-7fff-ffff7fffffff"
        );
    }

    #[test]
    fn uuid_from_text_parses_hex_and_dashes() {
        let u = [0x01234567, -0x76543211, 0x01234567, -0x76543211];
        let text = uuid_text(u);
        assert_eq!(uuid_from_text(&text), Some(u));
        let undashed: String = text.chars().filter(|c| *c != '-').collect();
        assert_eq!(uuid_from_text(&undashed), Some(u));
    }
}
