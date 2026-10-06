use crate::{Error, Result};
pub(crate) fn slice(b: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    b.get(
        at..at
            .checked_add(len)
            .ok_or_else(|| Error::invalid("range overflow", at))?,
    )
    .ok_or_else(|| Error::invalid("truncated data", at))
}
pub(crate) fn u16_at(b: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(slice(b, at, 2)?.try_into().unwrap()))
}
pub(crate) fn u32_at(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(slice(b, at, 4)?.try_into().unwrap()))
}
pub(crate) fn u64_at(b: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(slice(b, at, 8)?.try_into().unwrap()))
}
pub(crate) fn utf16(b: &[u8]) -> Result<String> {
    if !b.len().is_multiple_of(2) {
        return Err(Error::invalid("UTF-16 length", 0));
    }
    let words: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .take_while(|v| *v != 0)
        .collect();
    String::from_utf16(&words).map_err(|_| Error::invalid("UTF-16 encoding", 0))
}
