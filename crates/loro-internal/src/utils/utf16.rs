pub fn count_utf16_chars(utf8_str: &[u8]) -> usize {
    let mut utf16_count = 0;

    let mut iter = utf8_str.iter();

    while let Some(&byte) = iter.next() {
        if byte & 0b1000_0000 == 0 {
            utf16_count += 1;
        } else if byte & 0b1110_0000 == 0b1100_0000 {
            let _ = iter.next();

            utf16_count += 1;
        } else if byte & 0b1111_0000 == 0b1110_0000 {
            let _ = iter.next();
            let _ = iter.next();

            utf16_count += 1;
        } else if byte & 0b1111_1000 == 0b1111_0000 {
            let u = ((byte & 0b0000_0111) as u32) << 18
                | ((iter.next().unwrap() & 0b0011_1111) as u32) << 12;

            let _ = iter.next();
            let _ = iter.next();
            if u >= 0x10000 {
                utf16_count += 2;
            } else {
                utf16_count += 1;
            }
        } else {
            unreachable!()
        }
    }

    utf16_count
}

// TODO: FIXME: Tests
/// Count unicode chars in a utf8 string
pub fn count_unicode_chars(s: &[u8]) -> usize {
    std::str::from_utf8(s).unwrap().chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii() {
        let input = "Hello, world!";
        let expected = 13;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }

    #[test]
    fn test_2_byte() {
        let input = "Привет, мир!";
        let expected = 12;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }

    #[test]
    fn test_3_byte() {
        let input = "こんにちは世界";
        let expected = 7;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }

    #[test]
    fn test_4_byte() {
        let input = "👋🌍";
        let expected = 4;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }

    #[test]
    fn test_crazy_emoji_bytes() {
        let input = "👩‍👩‍👧‍👧👨‍👨‍👧";
        let expected = 19;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }

    #[test]
    fn test_empty_string() {
        let input = "";
        let expected = 0;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }

    #[test]
    fn test_single_char() {
        let input = "a";
        let expected = 1;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }

    #[test]
    fn test_utf8_with_bom() {
        let input = "\u{FEFF}Hello, world!"; // UTF-8 with BOM
        let expected = 14;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }

    #[test]
    fn test_long_string() {
        let input = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let expected = 52;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }

    #[test]
    fn test_utf8_with_null_char() {
        let input = "Hello\u{0000}world!";
        let expected = 12;
        assert_eq!(count_utf16_chars(input.as_bytes()), expected);
    }
}
