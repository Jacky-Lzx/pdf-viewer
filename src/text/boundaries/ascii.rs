// UAX #29 rules for ASCII: letters/digits/underscore, spaces, CRLF and
// the letter/numeric punctuation bridges. All other ASCII bytes stand alone.
// The caller sends mixed Unicode text through unicode-segmentation instead.
pub(super) fn fill(bytes: &[u8], bits: &mut [u64]) {
    debug_assert!(bytes.is_ascii());
    for offset in 1..bytes.len() {
        let left = bytes[offset - 1];
        let right = bytes[offset];
        let joined = (word(left) && word(right))
            || (left == b' ' && right == b' ')
            || (left == b'\r' && right == b'\n')
            || (offset + 1 < bytes.len() && bridge(left, right, bytes[offset + 1]))
            || (offset > 1 && bridge(bytes[offset - 2], left, right));
        if !joined {
            bits[offset / 64] |= 1 << (offset % 64);
        }
    }
}

fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn bridge(left: u8, middle: u8, right: u8) -> bool {
    match middle {
        b':' => left.is_ascii_alphabetic() && right.is_ascii_alphabetic(),
        b',' | b';' => left.is_ascii_digit() && right.is_ascii_digit(),
        b'.' | b'\'' => {
            (left.is_ascii_alphabetic() && right.is_ascii_alphabetic())
                || (left.is_ascii_digit() && right.is_ascii_digit())
        }
        _ => false,
    }
}
