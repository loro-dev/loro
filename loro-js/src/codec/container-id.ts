import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError } from "./errors";
import { readUlebNumber, writeUleb128 } from "./leb128";
import { PostcardReader, PostcardWriter } from "./postcard";
import {
  ContainerType,
  type ContainerId,
  type ContainerType as ContainerTypeValue,
  type KnownContainerType,
  type UnknownContainerType,
} from "./types";

const textDecoder = new TextDecoder("utf-8", { fatal: true });
const textEncoder = new TextEncoder();

export function unknownContainerType(value: number): UnknownContainerType {
  assertU8(value, "unknown container type");
  return { kind: "unknown", value };
}

export function containerTypeToRawByte(type: ContainerTypeValue): number {
  switch (type) {
    case ContainerType.Map:
      return 0;
    case ContainerType.List:
      return 1;
    case ContainerType.Text:
      return 2;
    case ContainerType.Tree:
      return 3;
    case ContainerType.MovableList:
      return 4;
    case ContainerType.Counter:
      return 5;
    default:
      assertU8(type.value, "unknown container type");
      return type.value;
  }
}

export function containerTypeFromRawByte(value: number): ContainerTypeValue {
  switch (value) {
    case 0:
      return ContainerType.Map;
    case 1:
      return ContainerType.List;
    case 2:
      return ContainerType.Text;
    case 3:
      return ContainerType.Tree;
    case 4:
      return ContainerType.MovableList;
    case 5:
      return ContainerType.Counter;
    default:
      return { kind: "unknown", value };
  }
}

export function containerTypeToHistoricalByte(type: ContainerTypeValue): number {
  switch (type) {
    case ContainerType.Text:
      return 0;
    case ContainerType.Map:
      return 1;
    case ContainerType.List:
      return 2;
    case ContainerType.MovableList:
      return 3;
    case ContainerType.Tree:
      return 4;
    case ContainerType.Counter:
      return 5;
    default:
      assertU8(type.value, "unknown container type");
      return type.value;
  }
}

export function containerTypeFromHistoricalByte(value: number): ContainerTypeValue {
  switch (value) {
    case 0:
      return ContainerType.Text;
    case 1:
      return ContainerType.Map;
    case 2:
      return ContainerType.List;
    case 3:
      return ContainerType.MovableList;
    case 4:
      return ContainerType.Tree;
    case 5:
      return ContainerType.Counter;
    default:
      return { kind: "unknown", value };
  }
}

export function decodeContainerId(bytes: Uint8Array): ContainerId {
  if (bytes.length === 0) {
    throw new LoroDecodeError("container ID is empty");
  }
  const reader = new ByteReader(bytes);
  const first = reader.readU8();
  const containerType = containerTypeFromRawByte(first & 0x7f);
  if ((first & 0x80) !== 0) {
    const nameOffset = reader.position;
    const length = readUlebNumber(reader, 0x7fff_ffff);
    let name: string;
    try {
      name = textDecoder.decode(reader.readBytes(length));
    } catch (error) {
      if (error instanceof LoroDecodeError) {
        throw error;
      }
      throw new LoroDecodeError("invalid UTF-8 container name", nameOffset);
    }
    reader.assertEnd("trailing root container ID bytes");
    return { kind: "root", name, containerType };
  }
  if (reader.remaining !== 12) {
    throw new LoroDecodeError("invalid normal container ID length", reader.position);
  }
  const peer = reader.readU64LE();
  const counter = reader.readU32LE() | 0;
  reader.assertEnd("trailing normal container ID bytes");
  return { kind: "normal", peer, counter, containerType };
}

export function encodeContainerId(id: ContainerId): Uint8Array {
  const rawType = containerTypeToRawByte(id.containerType);
  if (rawType > 0x7f) {
    throw new LoroEncodeError(`raw container type is out of range: ${rawType}`);
  }
  const writer = new ByteWriter();
  if (id.kind === "root") {
    const name = textEncoder.encode(id.name);
    writer.writeU8(rawType | 0x80);
    writeUleb128(writer, name.length);
    writer.writeBytes(name);
    return writer.toUint8Array();
  }
  assertNormalContainerId(id);
  writer.writeU8(rawType);
  writer.writeU64LE(id.peer);
  writer.writeU32LE(id.counter >>> 0);
  return writer.toUint8Array();
}

export function readPostcardContainerId(reader: PostcardReader): ContainerId {
  const tag = reader.readU32();
  if (tag === 0) {
    return {
      kind: "root",
      name: reader.readString(),
      containerType: containerTypeFromHistoricalByte(reader.readU8()),
    };
  }
  if (tag === 1) {
    return {
      kind: "normal",
      peer: reader.readU64(),
      counter: reader.readI32(),
      containerType: containerTypeFromHistoricalByte(reader.readU8()),
    };
  }
  throw new LoroDecodeError("invalid postcard ContainerID tag", reader.input.position);
}

export function writePostcardContainerId(writer: PostcardWriter, id: ContainerId): void {
  if (id.kind === "root") {
    writer.writeU32(0);
    writer.writeString(id.name);
  } else {
    assertNormalContainerId(id);
    writer.writeU32(1);
    writer.writeU64(id.peer);
    writer.writeI32(id.counter);
  }
  writer.writeU8(containerTypeToHistoricalByte(id.containerType));
}

export function decodePostcardContainerId(bytes: Uint8Array): ContainerId {
  const reader = new PostcardReader(bytes);
  const id = readPostcardContainerId(reader);
  reader.assertEnd();
  return id;
}

export function encodePostcardContainerId(id: ContainerId): Uint8Array {
  const writer = new PostcardWriter();
  writePostcardContainerId(writer, id);
  return writer.toUint8Array();
}

export function readPostcardOptionalContainerId(
  reader: PostcardReader,
): ContainerId | undefined {
  const tag = reader.readU32();
  if (tag === 0) {
    return undefined;
  }
  if (tag === 1) {
    return readPostcardContainerId(reader);
  }
  throw new LoroDecodeError(
    "invalid postcard Option<ContainerID> tag",
    reader.input.position,
  );
}

export function writePostcardOptionalContainerId(
  writer: PostcardWriter,
  id: ContainerId | undefined,
): void {
  if (id === undefined) {
    writer.writeU32(0);
    return;
  }
  writer.writeU32(1);
  writePostcardContainerId(writer, id);
}

export function decodePostcardOptionalContainerId(
  bytes: Uint8Array,
): ContainerId | undefined {
  const reader = new PostcardReader(bytes);
  const id = readPostcardOptionalContainerId(reader);
  reader.assertEnd();
  return id;
}

export function encodePostcardOptionalContainerId(
  id: ContainerId | undefined,
): Uint8Array {
  const writer = new PostcardWriter();
  writePostcardOptionalContainerId(writer, id);
  return writer.toUint8Array();
}

function assertNormalContainerId(id: Extract<ContainerId, { kind: "normal" }>): void {
  if (id.peer < 0n || id.peer > 0xffff_ffff_ffff_ffffn) {
    throw new LoroEncodeError(`container peer ID is out of range: ${id.peer}`);
  }
  if (
    !Number.isSafeInteger(id.counter) ||
    id.counter < -0x8000_0000 ||
    id.counter > 0x7fff_ffff
  ) {
    throw new LoroEncodeError(`container counter is out of range: ${id.counter}`);
  }
}

function assertU8(value: number, name: string): void {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xff) {
    throw new LoroEncodeError(`${name} is out of range: ${value}`);
  }
}

export function isKnownContainerType(
  type: ContainerTypeValue,
): type is KnownContainerType {
  return typeof type === "string";
}
