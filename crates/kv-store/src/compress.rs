use bytes::Bytes;
use loro_common::LoroError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    None,
    LZ4,
}

impl CompressionType {
    pub fn is_none(&self) -> bool {
        matches!(self, CompressionType::None)
    }
}

impl TryFrom<u8> for CompressionType {
    type Error = LoroError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(CompressionType::None),
            1 => Ok(CompressionType::LZ4),
            _ => Err(LoroError::DecodeError(
                format!("Invalid compression type: {}", value).into(),
            )),
        }
    }
}

impl From<CompressionType> for u8 {
    fn from(value: CompressionType) -> Self {
        match value {
            CompressionType::None => 0,
            CompressionType::LZ4 => 1,
        }
    }
}

pub fn compress(data: &[u8], compression_type: CompressionType) -> Vec<u8> {
    match compression_type {
        CompressionType::None => unreachable!(),
        CompressionType::LZ4 => lz4_flex::compress_prepend_size(data),
    }
}

pub fn decompress(data: Bytes, compression_type: CompressionType) -> Result<Bytes, LoroError> {
    match compression_type {
        CompressionType::None => unreachable!(),
        CompressionType::LZ4 => {
            let decompressed_data = lz4_flex::decompress_size_prepended(&data)
                .map_err(|e| LoroError::DecodeError(e.to_string().into()))?;
            Ok(Bytes::from(decompressed_data))
        }
    }
}
