use loro_common::{LoroError, LoroResult};

pub(crate) fn get_u32_le(bytes: &[u8]) -> LoroResult<(u32, &[u8])> {
    if bytes.len() < 4 {
        return Err(LoroError::DecodeError("Invalid bytes".into()));
    }
    let ans = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    Ok((ans, &bytes[4..]))
}

pub(crate) fn get_u8_le(bytes: &[u8]) -> LoroResult<(u8, &[u8])> {
    if bytes.is_empty() {
        return Err(LoroError::DecodeError("Invalid bytes".into()));
    }
    Ok((bytes[0], &bytes[1..]))
}

pub(crate) fn get_u16_le(bytes: &[u8]) -> LoroResult<(u16, &[u8])> {
    if bytes.len() < 2 {
        return Err(LoroError::DecodeError("Invalid bytes".into()));
    }
    let ans = u16::from_le_bytes(bytes[..2].try_into().unwrap());
    Ok((ans, &bytes[2..]))
}
