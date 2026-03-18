/// Decode tmux control mode octal encoding.
/// Characters < ASCII 32 and `\` are replaced with `\NNN` (3-digit octal).
pub fn decode_octal(input: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3].is_ascii_digit()
        {
            let val =
                (bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + (bytes[i + 3] - b'0');
            result.push(val);
            i += 4;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    result
}

/// Encode bytes to tmux control mode octal encoding.
pub fn encode_octal(input: &[u8]) -> String {
    let mut result = String::with_capacity(input.len() * 2);
    for &b in input {
        if b < 32 || b == b'\\' {
            result.push_str(&format!("\\{:03o}", b));
        } else {
            result.push(b as char);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_simple() {
        assert_eq!(decode_octal("hello"), b"hello");
    }

    #[test]
    fn test_decode_newline() {
        assert_eq!(decode_octal("hello\\015\\012"), b"hello\r\n");
    }

    #[test]
    fn test_decode_backslash() {
        assert_eq!(decode_octal("path\\134file"), b"path\\file");
    }

    #[test]
    fn test_decode_escape() {
        assert_eq!(decode_octal("\\033[32m"), b"\x1b[32m");
    }

    #[test]
    fn test_roundtrip() {
        let original = b"\x1b[32mhello\x1b[0m\r\nworld\\path";
        let encoded = encode_octal(original);
        let decoded = decode_octal(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_decode_empty() {
        assert_eq!(decode_octal(""), b"");
    }

    #[test]
    fn test_decode_partial_octal() {
        // Not enough digits — treat as literal
        assert_eq!(decode_octal("\\01"), b"\\01");
    }
}
