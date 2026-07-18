# LZ4 frames in current Loro snapshots

Verified against code 2026-07-17 at commit
`fd5a1fdab79142302f0c0fbceb8807128ec6d9cd` and locked dependency
`lz4_flex 0.11.5`.

LZ4 is used only for individual SSTable block bodies inside mode-3 snapshot
sections. Mode-4 FastUpdates change blocks are never LZ4-compressed. The
surrounding SSTable format, compression flag, block boundary, and outer block
checksum are specified in [encoding.md section 4](./encoding.md#4-sstable-used-by-snapshot-sections).

This document distinguishes three layers that all use the word “block”:

1. a Loro SSTable block;
2. the one LZ4 frame stored as that block's body when compression type is 1;
3. one or more LZ4 data blocks inside that frame.

They do not share lengths or checksums.

## 1. Loro compression selection

The SSTable metadata compression value is:

| Value | Meaning |
|---:|---|
| 0 | body stored directly |
| 1 | body stored as an LZ4 frame |
| any other value | invalid |

For a normal SSTable block, the candidate input to compression is the complete
uncompressed entry-data/offset-table/count body. For a large-value block, it is
the raw value bytes. Loro asks for LZ4 by default, constructs one complete
frame, and compares the complete frame length with the uncompressed input
length:

```text
if lz4_frame_len > raw_len:
    store raw bytes and compression = 0
else:
    store the LZ4 frame and compression = 1
```

Equality keeps LZ4. After this choice, Loro appends
`u32le xxHash32(stored_body, seed=0x4f524f4c)`. That four-byte Loro checksum is
outside the LZ4 frame and must be removed before passing the frame to an LZ4
decoder.

Compression enum, `FrameEncoder::new`, and `FrameDecoder::new`:
[`compress.rs`](../crates/kv-store/src/compress.rs#L7-L69).
Normal/large fallback and outer checksum:
[`block.rs`](../crates/kv-store/src/block.rs#L25-L116).
Outer checksum verification:
[`SSTable::check_block_checksum`](../crates/kv-store/src/sstable.rs#L489-L518).

## 2. Canonical frame profile written by Loro

Loro calls `lz4_flex::frame::FrameEncoder::new`, performs one `write_all` with
the whole candidate body, and calls `finish`. With lz4_flex 0.11.5 defaults,
the resulting frame uses:

| Frame property | Canonical value |
|---|---|
| frame format | modern LZ4 frame |
| block mode | independent |
| content size | absent |
| dictionary ID | absent |
| LZ4 block checksum | absent |
| LZ4 content checksum | absent |
| legacy format | false |
| block maximum | Auto, resolved from the single write length |

Therefore canonical `FLG` is always `0x60`: version bits `01`, independent
blocks set, and every optional flag clear.

Auto resolves the BD byte from the input length used to open the frame:

| Candidate raw length | Max LZ4 data-block size | BD |
|---:|---:|---:|
| `<= 64 KiB` | 64 KiB | `0x40` |
| `64 KiB < len <= 256 KiB` | 256 KiB | `0x50` |
| `> 256 KiB` | 4 MiB | `0x70` |

Auto does not choose the 1 MiB code `0x60`. Inputs above 4 MiB are split into
multiple independent data blocks of at most 4 MiB.

Dependency pin:
[`Cargo.lock`](../Cargo.lock#L1938-L1944).
The crate archive's `.cargo_vcs_info.json` pins source revision
`4c4ba15a4ce3ba3f0125177a0e4bba39f3d3a1e7`.
Locked dependency source symbols:
`lz4_flex-0.11.5/src/frame/header.rs::{BlockSize::from_buf_length,
FrameInfo::default, FrameInfo::write}` and
`src/frame/compress.rs::{FrameEncoder::new, begin_frame, Write::write,
write_block, end_frame}`.

## 3. Exact frame grammar

For current canonical output:

```text
u32le 0x184d2204                    # bytes 04 22 4d 18
u8    FLG                           # always 60
u8    BD                            # 40, 50, or 70
u8    HC                            # header checksum below

repeat:
    u32le block_info
    if block_info == 0:
        break                       # end mark; no content checksum follows
    stored_len = block_info & 0x7fff_ffff
    is_raw     = block_info & 0x8000_0000 != 0
    u8 data[stored_len]
```

There are no optional descriptor bytes between BD and HC and no LZ4 checksum
after a data block or the end mark in Loro's canonical profile.

The low 31 bits of `block_info` are the stored **data length**. They are not
always a compressed length: when bit 31 is set, the same length describes raw
uncompressed bytes. A zero `block_info` is the frame terminator.

### 3.1 Header checksum

LZ4's one-byte header checksum is:

```text
HC = (xxHash32(descriptor_bytes, seed=0) >> 8) & 0xff
```

For canonical Loro frames, `descriptor_bytes` is exactly `[FLG, BD]`. The
common 64 KiB header is therefore:

```text
04 22 4d 18 60 40 82
```

This seed is zero and the stored value is one byte. It must not be confused
with Loro's four-byte SSTable checksum using seed `0x4f524f4c`.

Locked source:
`lz4_flex-0.11.5/src/frame/header.rs::FrameInfo::{write,read}`.

### 3.2 Compressed versus raw LZ4 data blocks

For each frame data block, lz4_flex first attempts raw LZ4 block compression.
It writes compressed data only when:

```text
compressed_data_len < uncompressed_data_len
```

Otherwise it sets bit 31 and writes the original bytes. This per-data-block
choice is inside an LZ4 frame and is independent of Loro's outer whole-frame
fallback in section 1. A frame can therefore contain a mixture of compressed
and raw LZ4 data blocks and still have SSTable compression type 1.

Locked source:
`lz4_flex-0.11.5/src/frame/compress.rs::FrameEncoder::write_block`.

## 4. Raw LZ4 block payload

When bit 31 is clear, `data` is a raw LZ4 block made of sequences:

```text
u8 token

literal_len = token >> 4
if literal_len == 15:
    repeat:
        u8 extra
        literal_len += extra
    until extra != 255

u8 literals[literal_len]

if input is now exhausted:
    this is the final literal-only sequence; stop

u16le match_offset

match_len = 4 + (token & 0x0f)
if (token & 0x0f) == 15:
    repeat:
        u8 extra
        match_len += extra
    until extra != 255

copy match_len bytes from output[-match_offset]
```

The match copy is allowed to overlap its own newly written output. A decoder
must reject a zero offset, an offset beyond the available prefix for an
independent block, truncated extension/literal/offset bytes, integer overflow,
or output beyond the BD maximum.

The canonical lz4_flex compressor follows the LZ4 terminal restrictions:

- the last sequence contains literals only;
- the last five uncompressed bytes are literals; and
- the final match starts at least 12 bytes before the uncompressed block end.

Consequently an independent block shorter than 13 bytes cannot be emitted in
compressed form. It is stored as a raw LZ4 data block, after which Loro may
still discard the entire frame because of whole-frame overhead.

Locked dependency source:
`lz4_flex-0.11.5/src/block/{mod.rs,compress.rs,decompress.rs}`, specifically
`MFLIMIT`, `LAST_LITERALS`, `handle_last_literals`, `read_integer_ptr`, and
`decompress_internal`.

## 5. Decoder validation

A decoder for canonical Loro output can require the profile in section 2. A
decoder intentionally matching the current `FrameDecoder` may accept a wider
standard LZ4 frame, but that broader input support is not a writer rule.

For a canonical Loro SSTable block:

1. Use the SSTable metadata offsets to isolate the stored body plus its final
   four-byte Loro checksum.
2. Verify the Loro checksum over the stored body and remove it.
3. If compression type is 0, use the stored body directly.
4. If compression type is 1, verify magic, FLG version/reserved bits, BD
   reserved bits and supported maximum, and HC before allocating from BD.
5. For every LZ4 data block, validate the stored length against both remaining
   input and the BD maximum. Raw blocks copy exactly that many bytes; compressed
   blocks use section 4 and may output at most the BD maximum.
6. Require an end mark. Canonical output has no content checksum or second
   frame after it.
7. Apply the normal- or large-SSTable-block validation from encoding.md to the
   decompressed bytes.

The locked `FrameDecoder` parses optional content size and validates per-block
checksums while reading each block. Only when it reaches a valid end mark does
it compare the content size and validate an optional content checksum. It also
supports linked blocks and rejects dictionary IDs. The early-EOF paths below
bypass those end-mark content-size and content-checksum checks. These paths
matter only if an implementation deliberately accepts non-canonical frames.

The current dependency reader is wider in the following observable ways:

- it accepts legacy magic `0x184c2102` and selects the legacy 8 MiB block
  profile;
- an empty stored body is accepted as empty decompressed output. For a slice
  input of exactly four bytes, all byte values are also accepted as empty
  output: a non-legacy value reaches EOF while fetching the descriptor before
  magic validation, while legacy magic is accepted and then reaches EOF before
  a block;
- if reading the next four-byte block-info word returns `UnexpectedEof`, it
  returns decoder EOF. Consequently, after a complete data block it accepts a
  missing `u32le(0)` end mark, and it also swallows a one- to three-byte partial
  block-info tail; and
- any successfully decoded data block that produces zero bytes makes
  `Read::read` report EOF immediately, so Loro accepts the output and ignores
  any remaining frame bytes. Examples include uncompressed block-info
  `0x80000000`, and a compressed block of length one whose sole token is
  `00..0f`; and
- after a valid end mark it clears the current frame and returns EOF. Loro's
  `io::copy` stops at that first EOF and performs no remaining-input check, so
  arbitrary trailing bytes or a concatenated second frame are ignored.

These are current-reader tolerances, not canonical writer rules. A strict
canonical decoder should still enforce steps 4 through 6 above.

Locked dependency reader source:
`lz4_flex-0.11.5/src/frame/header.rs::FrameInfo::read` lines 275-300 and
`src/frame/decompress.rs::{read_frame_info,read_block,read_more,Read::read}`
lines 109-145, 220-239, and 305-365 at revision
`4c4ba15a4ce3ba3f0125177a0e4bba39f3d3a1e7`.
Zero-output compressed-block path:
`lz4_flex-0.11.5/src/block/decompress.rs` lines 240-365 at the same revision.
Loro call site:
[`compress.rs::CompressionType::decompress`](../crates/kv-store/src/compress.rs#L54-L69).

### 5.1 Checksum domains

| Checksum | Current Loro writer emits it? | Seed | Covered bytes |
|---|---|---:|---|
| LZ4 HC | yes | 0 | FLG + BD; optional descriptor bytes would also be included |
| LZ4 per-data-block checksum | no | 0 | stored compressed or raw LZ4 data only |
| LZ4 content checksum | no | 0 | concatenated uncompressed frame content |
| Loro SSTable block checksum | yes | `0x4f524f4c` | the complete stored body: raw bytes or complete LZ4 frame |

The first three are LZ4-frame mechanisms implemented by lz4_flex/twox-hash.
The last is Loro's xxhash-rust checksum and is specified in
[encoding-xxhash32.md](./encoding-xxhash32.md).

## 6. Writer checklist

A writer reproducing the current format should:

1. compress only SSTable block bodies, never FastUpdates blocks;
2. use one modern independent LZ4 frame with FLG `0x60` and Auto BD selection;
3. omit content size, dictionary ID, LZ4 block checksums, and LZ4 content
   checksum;
4. choose compressed versus raw separately for every LZ4 data block using a
   strict smaller-than comparison;
5. finish the frame with `u32le(0)`;
6. discard the whole frame only when it is strictly larger than the raw input;
7. record compression type 0 or 1 in SSTable metadata; and
8. append the Loro-seeded checksum over exactly the selected stored body.

For byte-for-byte reproduction rather than wire-compatible output, use the
locked lz4_flex 0.11.5 compressor; valid LZ4 encoders are free to choose
different matches and therefore need not produce identical compressed bytes.
