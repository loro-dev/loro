use std::io::{self, Read, Write};

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
                format!("Invalid compression type: {value}").into(),
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

pub fn compress(w: &mut Vec<u8>, data: &[u8], compression_type: CompressionType) {
    match compression_type {
        CompressionType::None => {
            w.write_all(data).unwrap();
        }
        CompressionType::LZ4 => {
            let mut encoder = lz4_flex::frame::FrameEncoder::new(w);
            encoder.write_all(data).unwrap();
            let _w = encoder.finish().unwrap();
        }
    }
}

pub fn decompress(
    out: &mut Vec<u8>,
    data: Bytes,
    compression_type: CompressionType,
) -> Result<(), LoroError> {
    decompress_with_limit(out, data, compression_type, usize::MAX)
}

/// Like [`decompress`], but fails if the data decompresses to more than
/// `max_len` bytes. It stops decompressing at that point, so `out` grows by at
/// most `max_len + 1` bytes.
pub fn decompress_with_limit(
    out: &mut Vec<u8>,
    data: Bytes,
    compression_type: CompressionType,
    max_len: usize,
) -> Result<(), LoroError> {
    let too_large = || {
        LoroError::DecodeError(format!("Decompressed block is larger than {max_len} bytes").into())
    };
    match compression_type {
        CompressionType::None => {
            if data.len() > max_len {
                return Err(too_large());
            }
            out.write_all(&data).unwrap();
            Ok(())
        }
        CompressionType::LZ4 => {
            let decoder = lz4_flex::frame::FrameDecoder::new(data.as_ref());
            // One byte past the limit tells a block of exactly `max_len` bytes
            // from a larger one.
            let limit = u64::try_from(max_len).unwrap_or(u64::MAX).saturating_add(1);
            let len = io::copy(&mut decoder.take(limit), out)
                .map_err(|e| LoroError::DecodeError(e.to_string().into()))?;
            if len > max_len as u64 {
                return Err(too_large());
            }
            Ok(())
        }
    }
}
