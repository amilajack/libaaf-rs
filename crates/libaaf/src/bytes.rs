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
    let mut result = String::with_capacity(b.len() / 2);
    append_utf16(b, &mut result)?;
    Ok(result)
}
pub(crate) fn append_utf16(b: &[u8], result: &mut String) -> Result<()> {
    if !b.len().is_multiple_of(2) {
        return Err(Error::invalid("UTF-16 length", 0));
    }
    let words = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| u16::from_le_bytes(*v))
        .take_while(|v| *v != 0);
    for ch in char::decode_utf16(words) {
        result.push(ch.map_err(|_| Error::invalid("UTF-16 encoding", 0))?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf16_decodes_scalars_and_rejects_unpaired_surrogates() {
        for text in ["", "Audio 1", "é漢🎶"] {
            let bytes: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
            assert_eq!(utf16(&bytes).unwrap(), text);
        }
        assert!(utf16(&[0]).is_err());
        assert!(utf16(&[0, 0xd8]).is_err());
        assert!(utf16(&[0, 0xdc]).is_err());
        assert_eq!(utf16(&[65, 0, 0, 0, 0, 0xd8]).unwrap(), "A");
    }
}
