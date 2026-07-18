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
  const input = [
    ...MARKER_DOMAIN,
    ...lengthPrefixed(encodeContainerId(parent)),
    ...lengthPrefixed(textEncoder.encode(key)),
    rawType,
  ];
  const digest = crc32(Uint8Array.from(input)) & 0x00ff_ffff;
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
  if (!MARKER_MAGIC.every((byte, index) => value[index] === byte)) return undefined;
  const type = containerTypeFromRawByte(value[4]!);
  if (typeof type !== "string") return undefined;
  const expected = mergeableMarker(parent, key, type);
  return expected.every((byte, index) => value[index] === byte) ? type : undefined;
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

function lengthPrefixed(value: Uint8Array): number[] {
  const output: number[] = [];
  let length = value.length;
  do {
    let byte = length & 0x7f;
    length = Math.floor(length / 0x80);
    if (length > 0) byte |= 0x80;
    output.push(byte);
  } while (length > 0);
  output.push(...value);
  return output;
}

function crc32(bytes: Uint8Array): number {
  let crc = 0xffff_ffff;
  for (const byte of bytes) {
    crc = (crc ^ byte) >>> 0;
    for (let bit = 0; bit < 8; bit += 1) {
      const mask = -(crc & 1);
      crc = ((crc >>> 1) ^ (0xedb8_8320 & mask)) >>> 0;
    }
  }
  return ~crc >>> 0;
}
