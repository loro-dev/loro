use std::io::{self, Write};

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
    match compression_type {
        CompressionType::None => {
            out.write_all(&data).unwrap();
            Ok(())
        }
        CompressionType::LZ4 => {
            let mut decoder = lz4_flex::frame::FrameDecoder::new(data.as_ref());
            io::copy(&mut decoder, out)
                .map_err(|e| LoroError::DecodeError(e.to_string().into()))?;
            Ok(())
        }
    }
}
