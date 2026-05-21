#[inline]
pub(super) fn is_compound_operator_ascii(
    first_byte: u8,
    second_byte: u8,
    valid_second_chars: &[u8],
) -> bool {
    if !valid_second_chars.contains(&second_byte) {
        return false;
    }

    match (first_byte, second_byte) {
        // Assignment operators.
        (b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'.', b'=') => true,

        // Comparison operators.
        (b'<' | b'>' | b'=' | b'!', b'=') => true,

        // Pattern operators.
        (b'=' | b'!', b'~') => true,

        // Increment/decrement.
        (b'+', b'+') | (b'-', b'-') => true,

        // Logical operators.
        (b'&', b'&') | (b'|', b'|') => true,

        // Shift operators.
        (b'<', b'<') | (b'>', b'>') => true,

        // Other compound operators.
        (b'*', b'*')
        | (b'/', b'/')
        | (b'-' | b'=', b'>')
        | (b'.', b'.')
        | (b'~', b'~')
        | (b':', b':') => true,

        _ => false,
    }
}
