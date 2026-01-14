pub const LORO_XXH32_SEED: UInt = 0x4F524F4C

const PRIME32_1: UInt = 0x9E3779B1
const PRIME32_2: UInt = 0x85EBCA77
const PRIME32_3: UInt = 0xC2B2AE3D
const PRIME32_4: UInt = 0x27D4EB2F
const PRIME32_5: UInt = 0x165667B1

fn rotl32(x: UInt, r: Int) -> UInt {
  (x << r) | (x >> (32 - r))
}

fn mul32(a: UInt, b: UInt) -> UInt {
  (a.to_uint64() * b.to_uint64()).to_uint()
}

fn read_u32_le(bytes: BytesView, offset: Int) -> UInt raise DecodeError {
  if offset < 0 || offset + 4 > bytes.length() {
    raise DecodeError("xxhash32: out of bounds")
  }
  let b0 = bytes[offset + 0].to_uint()
  let b1 = bytes[offset + 1].to_uint()
  let b2 = bytes[offset + 2].to_uint()
  let b3 = bytes[offset + 3].to_uint()
  b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

fn round(acc: UInt, input: UInt) -> UInt {
  let mut acc = acc
  acc = acc + mul32(input, PRIME32_2)
  acc = rotl32(acc, 13)
  acc = mul32(acc, PRIME32_1)
  acc
}

pub fn xxhash32(data: BytesView, seed: UInt) -> UInt raise DecodeError {
  let len = data.length()
  let mut offset = 0
  let mut h32: UInt = 0

  if len >= 16 {
    let mut v1 = seed + PRIME32_1 + PRIME32_2
    let mut v2 = seed + PRIME32_2
    let mut v3 = seed
    let mut v4 = seed - PRIME32_1

    let limit = len - 16
    while offset <= limit {
      v1 = round(v1, read_u32_le(data, offset)); offset = offset + 4
      v2 = round(v2, read_u32_le(data, offset)); offset = offset + 4
      v3 = round(v3, read_u32_le(data, offset)); offset = offset + 4
      v4 = round(v4, read_u32_le(data, offset)); offset = offset + 4
    }
    h32 = rotl32(v1, 1) + rotl32(v2, 7) + rotl32(v3, 12) + rotl32(v4, 18)
  } else {
    h32 = seed + PRIME32_5
  }

  h32 = h32 + len.reinterpret_as_uint()

  while offset + 4 <= len {
    h32 = h32 + mul32(read_u32_le(data, offset), PRIME32_3)
    h32 = mul32(rotl32(h32, 17), PRIME32_4)
    offset = offset + 4
  }

  while offset < len {
    h32 = h32 + mul32(data[offset].to_uint(), PRIME32_5)
    h32 = mul32(rotl32(h32, 11), PRIME32_1)
    offset = offset + 1
  }

  // avalanche
  h32 = h32 ^ (h32 >> 15)
  h32 = mul32(h32, PRIME32_2)
  h32 = h32 ^ (h32 >> 13)
  h32 = mul32(h32, PRIME32_3)
  h32 = h32 ^ (h32 >> 16)
  h32
}
