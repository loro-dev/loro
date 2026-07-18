import {
  containerTypeFromRawByte,
  containerTypeToRawByte,
  encodeContainerId,
} from "../codec/container-id";
import {
  ContainerType,
  type ContainerId,
  type ContainerType as CodecContainerType,
} from "../codec/types";

const textEncoder = new TextEncoder();
const MERGEABLE_PREFIX = "🤝:";
const MARKER_MAGIC = Uint8Array.of(0, 0x4c, 0x4d, 1);
const MARKER_DOMAIN = textEncoder.encode("loro.mergeable.marker.v1");

export function newMergeableContainerId(
  parent: ContainerId,
  key: string,
  containerType: CodecContainerType,
): ContainerId {
  if (parent.containerType !== ContainerType.Map) {
    throw new TypeError("mergeable child parent must be a map");
  }
  const parentPayload =
    parent.kind === "root" && parent.name.startsWith(MERGEABLE_PREFIX)
      ? parent.name.slice(MERGEABLE_PREFIX.length)
      : parent.kind === "root"
        ? `$${escapeSegment(parent.name)}`
        : `@${parent.peer.toString(36)}:${signedBase36(parent.counter)}`;
  return {
    kind: "root",
    name: `${MERGEABLE_PREFIX}${parentPayload}>${escapeSegment(key)}`,
    containerType,
  };
}

export function mergeableMarker(
  parent: ContainerId,
  key: string,
  containerType: CodecContainerType,
): Uint8Array {
  const rawType = containerTypeToRawByte(containerType);
  const parentBytes = encodeContainerId(parent);
  const keyBytes = textEncoder.encode(key);
  const input = new Uint8Array(
    MARKER_DOMAIN.length +
      varintLength(parentBytes.length) +
      parentBytes.length +
      varintLength(keyBytes.length) +
      keyBytes.length +
      1,
  );
  let offset = MARKER_DOMAIN.length;
  input.set(MARKER_DOMAIN, 0);
  offset = writeVarint(input, offset, parentBytes.length);
  input.set(parentBytes, offset);
  offset += parentBytes.length;
  offset = writeVarint(input, offset, keyBytes.length);
  input.set(keyBytes, offset);
  offset += keyBytes.length;
  input[offset] = rawType;
  const digest = crc32(input) & 0x00ff_ffff;
  return Uint8Array.of(
    ...MARKER_MAGIC,
    rawType,
    (digest >>> 16) & 0xff,
    (digest >>> 8) & 0xff,
    digest & 0xff,
  );
}

export function parseMergeableMarker(
  parent: ContainerId,
  key: string,
  value: unknown,
): CodecContainerType | undefined {
  if (!(value instanceof Uint8Array) || value.length !== 8) return undefined;
  for (let index = 0; index < MARKER_MAGIC.length; index += 1) {
    if (value[index] !== MARKER_MAGIC[index]) return undefined;
  }
  const type = containerTypeFromRawByte(value[4]!);
  if (typeof type !== "string") return undefined;
  const expected = mergeableMarker(parent, key, type);
  for (let index = 0; index < expected.length; index += 1) {
    if (value[index] !== expected[index]) return undefined;
  }
  return type;
}

export function isMergeableContainerId(id: ContainerId): boolean {
  return id.kind === "root" && id.name.startsWith(MERGEABLE_PREFIX);
}

function escapeSegment(value: string): string {
  let output = "";
  for (const character of value) {
    if (character === "\\") output += "\\\\";
    else if (character === ">") output += "\\>";
    else if (character === "/") output += "\\s";
    else if (character === "\0") output += "\\0";
    else output += character;
  }
  return output;
}

function signedBase36(value: number): string {
  return value < 0 ? `-${(-value).toString(36)}` : value.toString(36);
}

function varintLength(value: number): number {
  let length = 1;
  while (value >= 0x80) {
    value = Math.floor(value / 0x80);
    length += 1;
  }
  return length;
}

function writeVarint(output: Uint8Array, offset: number, value: number): number {
  do {
    let byte = value & 0x7f;
    value = Math.floor(value / 0x80);
    if (value > 0) byte |= 0x80;
    output[offset] = byte;
    offset += 1;
  } while (value > 0);
  return offset;
}

let crc32Table: Int32Array | undefined;

function getCrc32Table(): Int32Array {
  if (crc32Table === undefined) {
    const table = new Int32Array(256);
    for (let byte = 0; byte < 256; byte += 1) {
      let crc = byte;
      for (let bit = 0; bit < 8; bit += 1) {
        crc = (crc >>> 1) ^ (0xedb8_8320 & -(crc & 1));
      }
      table[byte] = crc;
    }
    crc32Table = table;
  }
  return crc32Table;
}

function crc32(bytes: Uint8Array): number {
  const table = getCrc32Table();
  let crc = 0xffff_ffff;
  for (let index = 0; index < bytes.length; index += 1) {
    crc = (crc >>> 8) ^ table[(crc ^ bytes[index]!) & 0xff]!;
  }
  return ~crc >>> 0;
}
