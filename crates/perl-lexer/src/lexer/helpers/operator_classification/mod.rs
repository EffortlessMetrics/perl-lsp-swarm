mod ascii;
mod unicode;

const COMPOUND_SECOND_CHARS: &[u8] = b"=<>&|+->.~*:";

#[inline]
pub(crate) fn is_compound_operator(first: char, second: char) -> bool {
    if first.is_ascii() && second.is_ascii() {
        return ascii::is_compound_operator_ascii(first as u8, second as u8, COMPOUND_SECOND_CHARS);
    }

    unicode::is_compound_operator_unicode(first, second)
}
