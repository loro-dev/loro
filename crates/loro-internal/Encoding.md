# Loro Encoding Format

## Header

At least 22 bytes.

- Magic Bytes: The encoding starts with `loro` as magic bytes.
- Checksum: MD5 checksum of the encoded data, including the header. The checksum is encoded as a 16-byte array. When calculating the checksum, the `checksum` and `magic bytes` fields are trimmed.
- Encoding Method (1 byte): There are multiple encoding methods available for a specific encoding version.
- Encoding Version (LEB128 unsigned number): The version of the encoding format.

## Encoding Methods

### Snapshot

### Rle Update

