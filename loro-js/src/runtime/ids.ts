import {
  ContainerType as CodecContainerType,
  type ContainerId as CodecContainerId,
  type ContainerType as CodecContainerTypeValue,
  type Id as CodecId,
} from "../codec/types";
import type {
  ContainerID,
  ContainerType,
  OpId,
  PeerID,
  PeerIdInput,
  TreeID,
} from "./types";

const U64_MAX = 0xffff_ffff_ffff_ffffn;

export function parsePeerId(peer: PeerIdInput): bigint {
  let value: bigint;
  try {
    value = typeof peer === "bigint" ? peer : BigInt(peer);
  } catch {
    throw new TypeError("peer id must be an unsigned 64-bit integer");
  }
  if (value < 0n || value > U64_MAX) {
    throw new RangeError(`peer id is out of range: ${String(peer)}`);
  }
  return value;
}

export function peerIdToString(peer: bigint): PeerID {
  return peer.toString() as PeerID;
}

export function parseOpId(id: OpId): CodecId {
  assertCounter(id.counter);
  return { peer: parsePeerId(id.peer), counter: id.counter };
}

export function formatOpId(id: CodecId): OpId {
  return { peer: peerIdToString(id.peer), counter: id.counter };
}

export function formatTreeId(id: CodecId): TreeID {
  return `${id.counter}@${id.peer}` as TreeID;
}

export function idStrToId(id: TreeID): OpId {
  return formatOpId(parseTreeId(id));
}

export function parseTreeId(id: string): CodecId {
  const separator = id.indexOf("@");
  if (separator <= 0 || id.indexOf("@", separator + 1) !== -1) {
    throw new TypeError(`invalid tree id: ${id}`);
  }
  const counter = Number(id.slice(0, separator));
  assertCounter(counter);
  return { peer: parsePeerId(id.slice(separator + 1)), counter };
}

export function codecTypeToPublic(type: CodecContainerTypeValue): ContainerType {
  switch (type) {
    case CodecContainerType.Text:
      return "Text";
    case CodecContainerType.Map:
      return "Map";
    case CodecContainerType.List:
      return "List";
    case CodecContainerType.Tree:
      return "Tree";
    case CodecContainerType.MovableList:
      return "MovableList";
    case CodecContainerType.Counter:
      return "Counter";
    default:
      throw new TypeError(`unsupported container type ${type.value}`);
  }
}

export function publicTypeToCodec(type: ContainerType): CodecContainerTypeValue {
  switch (type) {
    case "Text":
      return CodecContainerType.Text;
    case "Map":
      return CodecContainerType.Map;
    case "List":
      return CodecContainerType.List;
    case "Tree":
      return CodecContainerType.Tree;
    case "MovableList":
      return CodecContainerType.MovableList;
    case "Counter":
      return CodecContainerType.Counter;
  }
}

export function formatContainerId(id: CodecContainerId): ContainerID {
  const type = codecTypeToPublic(id.containerType);
  return (
    id.kind === "root"
      ? `cid:root-${id.name}:${type}`
      : `cid:${id.counter}@${id.peer}:${type}`
  ) as ContainerID;
}

export function parseContainerId(id: string): CodecContainerId {
  if (!id.startsWith("cid:")) {
    throw new TypeError(`invalid container id: ${id}`);
  }
  const typeSeparator = id.lastIndexOf(":");
  if (typeSeparator <= 4) {
    throw new TypeError(`invalid container id: ${id}`);
  }
  const type = id.slice(typeSeparator + 1) as ContainerType;
  const containerType = publicTypeToCodec(type);
  const body = id.slice(4, typeSeparator);
  if (body.startsWith("root-")) {
    return { kind: "root", name: body.slice(5), containerType };
  }
  const operationId = parseTreeId(body);
  return { kind: "normal", ...operationId, containerType };
}

export function newContainerID(id: OpId, type: ContainerType): ContainerID {
  return formatContainerId({
    kind: "normal",
    ...parseOpId(id),
    containerType: publicTypeToCodec(type),
  });
}

export function newRootContainerID(name: string, type: ContainerType): ContainerID {
  return formatContainerId({
    kind: "root",
    name,
    containerType: publicTypeToCodec(type),
  });
}

export function isContainerId(value: string): value is ContainerID {
  try {
    parseContainerId(value);
    return true;
  } catch {
    return false;
  }
}

export function idsEqual(left: CodecId, right: CodecId): boolean {
  return left.peer === right.peer && left.counter === right.counter;
}

export function containerIdsEqual(
  left: CodecContainerId,
  right: CodecContainerId,
): boolean {
  return formatContainerId(left) === formatContainerId(right);
}

function assertCounter(counter: number): void {
  if (!Number.isSafeInteger(counter) || counter < 0 || counter > 0x7fff_ffff) {
    throw new RangeError(`counter is out of range: ${counter}`);
  }
}
