use std::io::{Read, Write};

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
            // The ratio profile picks matches for smaller output. The frame
            // is still canonical (docs/encoding-lz4.md).
            let mut encoder = lz4rip::frame::FrameEncoder::new(w).ratio_profile();
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
            lz4rip::frame::FrameDecoder::new(data.as_ref())
                .read_to_end(out)
                .map_err(|e| LoroError::DecodeError(e.to_string().into()))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LZ4 bodies keep the canonical frame profile of docs/encoding-lz4.md
    /// (FLG 0x60, Auto BD, no checksums), which the JS and MoonBit codecs
    /// read, and decode back.
    #[test]
    fn lz4_bodies_use_the_canonical_frame_profile() {
        for (len, bd) in [
            (1_000, 0x40),
            (4_096, 0x40),
            (100_000, 0x50),
            (300_000, 0x70),
        ] {
            let data: Vec<u8> = (0..len)
                .flat_map(|i: u32| format!("key{} value{}\n", i % 211, i % 7).into_bytes())
                .take(len as usize)
                .collect();
            let mut body = Vec::new();
            compress(&mut body, &data, CompressionType::LZ4);
            assert_eq!(&body[..6], &[0x04, 0x22, 0x4d, 0x18, 0x60, bd], "len {len}");
            if bd == 0x40 {
                assert_eq!(body[6], 0x82, "header checksum, len {len}");
            }
            assert_eq!(&body[body.len() - 4..], &[0; 4], "end mark, len {len}");
            assert!(body.len() < data.len(), "len {len}");
            let mut out = Vec::new();
            decompress(&mut out, Bytes::from(body), CompressionType::LZ4).unwrap();
            assert_eq!(out, data, "len {len}");
        }
    }
}
