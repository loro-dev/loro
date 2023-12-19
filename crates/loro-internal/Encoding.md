# Loro Encoding Format

## Header

The header has 22 bytes.

- Magic Bytes: The encoding starts with `loro` as magic bytes.
- Checksum: MD5 checksum of the encoded data, including the header. The checksum is encoded as a 16-byte array. The `checksum` and `magic bytes` fields are trimmed when calculating the checksum.
- Encoding Method (2 bytes, big endian): Multiple encoding methods are available for a specific encoding version.

## Encoding Methods

### Snapshot

### Rle Update

