import { ByteReader, ByteWriter, bytesEqual, compareBytes } from "./bytes";
import {
  containerTypeFromRawByte,
  containerTypeToRawByte,
  decodeContainerId,
  encodeContainerId,
  readPostcardOptionalContainerId,
  writePostcardOptionalContainerId,
} from "./container-id";
import { decodeAssert, encodeAssert } from "./errors";
import { U64_MAX, readUleb128, readUlebNumber, writeUleb128 } from "./leb128";
import { PostcardReader, PostcardWriter } from "./postcard";
import { decodePositionArena, encodePositionArena } from "./position-arena";
import {
  decodeBoolRle,
  decodeColumnarVecMaybeWrapped,
  decodeDeltaRleI32,
  decodeDeltaRleU32,
  decodeDeltaRleUsize,
  encodeBoolRle,
  encodeColumnarVec,
  encodeColumnarVecWrapped,
  encodeDeltaRleI32,
  encodeDeltaRleU32,
  encodeDeltaRleUsize,
  takeColumnarVec,
} from "./serde-columnar";
import {
  decodeSstable,
  encodeSstable,
  type DecodeSstableOptions,
  type EncodeSstableOptions,
} from "./sstable";
import {
  ContainerType,
  type ContainerId,
  type ContainerType as ContainerTypeValue,
  type EncodedLoroValue,
  type Frontiers,
  type UnknownContainerType,
} from "./types";
import {
  readPostcardValue,
  readPostcardValueMap,
  readPostcardValues,
  writePostcardValue,
  writePostcardValueMap,
  writePostcardValues,
} from "./value";
import { decodePostcardFrontiers, encodePostcardFrontiers } from "./version";

const MAX_STATE_ITEMS = 10_000_000;
const EMPTY_STATE_SENTINEL = Uint8Array.of(0x45);
const FRONTIERS_KEY = Uint8Array.of(0x66, 0x72);
const textEncoder = new TextEncoder();

export interface MapStateMetadata {
  readonly key: string;
  readonly peerIndex: bigint;
  readonly lamport: bigint;
}

export interface MapStateSnapshot {
  readonly kind: typeof ContainerType.Map;
  readonly values: readonly (readonly [string, EncodedLoroValue])[];
  readonly deletedKeys: readonly string[];
  readonly peers: readonly bigint[];
  readonly metadata: readonly MapStateMetadata[];
}

export interface ListStateItemId {
  readonly peerIndex: bigint;
  readonly counter: number;
  readonly lamportSub: number;
}

export interface ListStateSnapshot {
  readonly kind: typeof ContainerType.List;
  readonly values: readonly EncodedLoroValue[];
  readonly peers: readonly bigint[];
  readonly ids: readonly ListStateItemId[];
}

export interface TextStateSpan {
  readonly peerIndex: bigint;
  readonly counter: number;
  readonly lamportSub: number;
  readonly length: number;
}

export interface TextStateMark {
  readonly keyIndex: number;
  readonly value: EncodedLoroValue;
  readonly info: number;
}

export interface TextStateSnapshot {
  readonly kind: typeof ContainerType.Text;
  readonly text: string;
  readonly peers: readonly bigint[];
  readonly spans: readonly TextStateSpan[];
  readonly keys: readonly string[];
  readonly marks: readonly TextStateMark[];
}

export interface TreeStateNode {
  readonly peerIndex: bigint;
  readonly counter: number;
  readonly parentIndexPlusTwo: bigint;
  readonly lastSetPeerIndex: bigint;
  readonly lastSetCounter: number;
  readonly lastSetLamportSub: number;
  readonly fractionalIndexIndex: number;
}

export interface TreeStateSnapshot {
  readonly kind: typeof ContainerType.Tree;
  readonly peers: readonly bigint[];
  readonly nodes: readonly TreeStateNode[];
  readonly positions: readonly Uint8Array[];
  readonly reserved: Uint8Array;
}

export interface MovableListStateItem {
  readonly invisibleListItems: bigint;
  readonly positionIdEqualsElementId: boolean;
  readonly elementIdEqualsLastSetId: boolean;
}

export interface MovableListStateLamportId {
  readonly peerIndex: bigint;
  readonly lamport: number;
}

export interface MovableListStateSnapshot {
  readonly kind: typeof ContainerType.MovableList;
  readonly values: readonly EncodedLoroValue[];
  readonly peers: readonly bigint[];
  readonly items: readonly MovableListStateItem[];
  readonly listItemIds: readonly ListStateItemId[];
  readonly elementIds: readonly MovableListStateLamportId[];
  readonly lastSetIds: readonly MovableListStateLamportId[];
}

export interface CounterStateSnapshot {
  readonly kind: typeof ContainerType.Counter;
  /** Raw IEEE-754 bits. Keeping bits avoids changing NaN payloads. */
  readonly bits: bigint;
}

export interface UnknownStateSnapshot {
  readonly kind: UnknownContainerType;
  readonly payload: Uint8Array;
}

export type ContainerStateSnapshot =
  | MapStateSnapshot
  | ListStateSnapshot
  | TextStateSnapshot
  | TreeStateSnapshot
  | MovableListStateSnapshot
  | CounterStateSnapshot
  | UnknownStateSnapshot;

export interface ContainerStateWrapper {
  readonly containerType: ContainerTypeValue;
  readonly depth: bigint;
  readonly parent: ContainerId | undefined;
  readonly state: ContainerStateSnapshot;
}

export interface StateSnapshotContainerEntry {
  readonly id: ContainerId;
  readonly wrapper: ContainerStateWrapper;
}

export type StateSnapshotStore =
  | { readonly kind: "absent" }
  | { readonly kind: "empty" }
  | {
      readonly kind: "sstable";
      readonly frontiers: Frontiers | undefined;
      readonly containers: readonly StateSnapshotContainerEntry[];
    };

export function decodeMapStateSnapshot(bytes: Uint8Array): MapStateSnapshot {
  const reader = new PostcardReader(bytes);
  const values = readPostcardValueMap(reader);
  const deletedKeys = readStringVector(reader, "map deleted keys");
  const peers = readPeerTable(reader.input);
  const keys = collectSortedMapKeysForDecode(values, deletedKeys);
  const peerCount = BigInt(peers.length);
  const metadata: MapStateMetadata[] = [];
  for (const key of keys) {
    const peerIndex = readUleb128(reader.input, U64_MAX);
    const lamport = readUleb128(reader.input, U64_MAX);
    assertDecodedPeerIndex(peerIndex, peerCount, "map metadata");
    metadata.push({ key, peerIndex, lamport });
  }
  reader.assertEnd();
  sortByUtf8Key(values, (entry) => entry[0]);
  sortByUtf8Key(deletedKeys, (key) => key);
  return { kind: ContainerType.Map, values, deletedKeys, peers, metadata };
}

export function encodeMapStateSnapshot(state: MapStateSnapshot): Uint8Array {
  const values = [...state.values];
  sortByUtf8Key(values, (entry) => entry[0]);
  const deletedKeys = [...state.deletedKeys];
  sortByUtf8Key(deletedKeys, (key) => key);
  const keys = collectSortedMapKeysForEncode(values, deletedKeys);
  const metadataByKey = new Map<string, MapStateMetadata>();
  for (const metadata of state.metadata) {
    encodeAssert(
      !metadataByKey.has(metadata.key),
      `duplicate map metadata key ${metadata.key}`,
    );
    metadataByKey.set(metadata.key, metadata);
  }
  encodeAssert(metadataByKey.size === keys.length, "map metadata length mismatch");

  const output = new ByteWriter();
  const postcard = new PostcardWriter(output);
  writePostcardValueMap(postcard, values);
  postcard.writeArray(deletedKeys, (writer, key) => writer.writeString(key));
  writePeerTable(output, state.peers);
  const peerCount = BigInt(state.peers.length);
  for (const key of keys) {
    const metadata = metadataByKey.get(key);
    encodeAssert(metadata !== undefined, `missing map metadata for key ${key}`);
    assertEncodedPeerIndex(metadata.peerIndex, peerCount, "map metadata");
    writeUleb128(output, metadata.peerIndex);
    writeUleb128(output, metadata.lamport);
  }
  return output.toUint8Array();
}

export function decodeListStateSnapshot(bytes: Uint8Array): ListStateSnapshot {
  const reader = new PostcardReader(bytes);
  const values = readPostcardValues(reader);
  const peers = readPeerTable(reader.input);
  const columns = decodeColumnarVecMaybeWrapped(reader.input.readRemaining());
  decodeAssert(columns.length === 3, "list state must have three ID columns");
  const peerIndices = decodeDeltaRleUsize(columns[0]!);
  const counters = decodeDeltaRleI32(columns[1]!);
  const lamportSubs = decodeDeltaRleI32(columns[2]!);
  decodeAssert(
    peerIndices.length === values.length &&
      counters.length === values.length &&
      lamportSubs.length === values.length,
    "list state ID length mismatch",
  );
  const peerCount = BigInt(peers.length);
  const ids = peerIndices.map((peerIndex, index) => {
    assertDecodedPeerIndex(peerIndex, peerCount, "list state ID");
    return {
      peerIndex,
      counter: counters[index]!,
      lamportSub: lamportSubs[index]!,
    };
  });
  return { kind: ContainerType.List, values, peers, ids };
}

export function encodeListStateSnapshot(state: ListStateSnapshot): Uint8Array {
  encodeAssert(state.ids.length === state.values.length, "list state ID length mismatch");
  const peerCount = BigInt(state.peers.length);
  for (const id of state.ids) {
    assertEncodedPeerIndex(id.peerIndex, peerCount, "list state ID");
  }
  const ids = state.ids;
  const peerIndices: bigint[] = new Array(ids.length);
  const counters: number[] = new Array(ids.length);
  const lamportSubs: number[] = new Array(ids.length);
  for (let index = 0; index < ids.length; index += 1) {
    const id = ids[index]!;
    peerIndices[index] = id.peerIndex;
    counters[index] = id.counter;
    lamportSubs[index] = id.lamportSub;
  }
  const output = new ByteWriter();
  const postcard = new PostcardWriter(output);
  writePostcardValues(postcard, state.values);
  writePeerTable(output, state.peers);
  output.writeBytes(
    encodeColumnarVecWrapped([
      encodeDeltaRleUsize(peerIndices),
      encodeDeltaRleI32(counters),
      encodeDeltaRleI32(lamportSubs),
    ]),
  );
  return output.toUint8Array();
}

export function decodeTextStateSnapshot(bytes: Uint8Array): TextStateSnapshot {
  const reader = new PostcardReader(bytes);
  const text = reader.readString();
  const peers = readPeerTable(reader.input);
  decodeAssert(reader.readUsize() === 3, "text state must have three encoded fields");
  const columns = readColumnarVecFrom(reader.input);
  decodeAssert(columns.length === 4, "text state must have four span columns");
  const peerIndices = decodeDeltaRleUsize(columns[0]!);
  const counters = decodeDeltaRleI32(columns[1]!);
  const lamportSubs = decodeDeltaRleI32(columns[2]!);
  const lengths = decodeDeltaRleI32(columns[3]!);
  decodeAssert(
    peerIndices.length === lengths.length &&
      counters.length === lengths.length &&
      lamportSubs.length === lengths.length,
    "text state span column length mismatch",
  );
  const peerCount = BigInt(peers.length);
  const spans = lengths.map((length, index) => {
    const peerIndex = peerIndices[index]!;
    assertDecodedPeerIndex(peerIndex, peerCount, "text state span");
    return {
      peerIndex,
      counter: counters[index]!,
      lamportSub: lamportSubs[index]!,
      length,
    };
  });
  const keys = readStringVector(reader, "text mark keys");
  const markCount = reader.readUsize();
  decodeAssert(markCount <= MAX_STATE_ITEMS, "too many text marks");
  const marks: TextStateMark[] = [];
  for (let index = 0; index < markCount; index += 1) {
    decodeAssert(reader.readUsize() === 3, "text mark must have three encoded fields");
    const keyIndex = reader.readUsize();
    const value = readPostcardValue(reader);
    const info = reader.readU8();
    decodeAssert(keyIndex < keys.length, "text mark key index out of range");
    marks.push({ keyIndex, value, info });
  }
  reader.assertEnd();
  validateDecodedText(text, spans, marks);
  return { kind: ContainerType.Text, text, peers, spans, keys, marks };
}

export function encodeTextStateSnapshot(state: TextStateSnapshot): Uint8Array {
  validateEncodedText(state);
  const spans = state.spans;
  const peerIndices: bigint[] = new Array(spans.length);
  const counters: number[] = new Array(spans.length);
  const lamportSubs: number[] = new Array(spans.length);
  const lengths: number[] = new Array(spans.length);
  for (let index = 0; index < spans.length; index += 1) {
    const span = spans[index]!;
    peerIndices[index] = span.peerIndex;
    counters[index] = span.counter;
    lamportSubs[index] = span.lamportSub;
    lengths[index] = span.length;
  }
  const output = new ByteWriter();
  const postcard = new PostcardWriter(output);
  postcard.writeString(state.text);
  writePeerTable(output, state.peers);
  postcard.writeUsize(3);
  output.writeBytes(
    encodeColumnarVec([
      encodeDeltaRleUsize(peerIndices),
      encodeDeltaRleI32(counters),
      encodeDeltaRleI32(lamportSubs),
      encodeDeltaRleI32(lengths),
    ]),
  );
  postcard.writeArray(state.keys, (writer, key) => writer.writeString(key));
  postcard.writeUsize(state.marks.length);
  for (const mark of state.marks) {
    postcard.writeUsize(3);
    postcard.writeUsize(mark.keyIndex);
    writePostcardValue(postcard, mark.value);
    postcard.writeU8(mark.info);
  }
  return output.toUint8Array();
}

export function decodeTreeStateSnapshot(bytes: Uint8Array): TreeStateSnapshot {
  const input = new ByteReader(bytes);
  const peers = readPeerTable(input);
  const reader = new PostcardReader(input);
  decodeAssert(reader.readUsize() === 4, "tree state must have four encoded fields");
  const idColumns = readColumnarVecFrom(input);
  decodeAssert(idColumns.length === 2, "tree state must have two node ID columns");
  const nodePeerIndices = decodeDeltaRleUsize(idColumns[0]!);
  const nodeCounters = decodeDeltaRleI32(idColumns[1]!);
  decodeAssert(
    nodePeerIndices.length === nodeCounters.length,
    "tree state node ID column length mismatch",
  );

  const nodeColumns = readColumnarVecFrom(input);
  decodeAssert(nodeColumns.length === 5, "tree state must have five node columns");
  const parentIndices = decodeDeltaRleUsize(nodeColumns[0]!);
  const lastSetPeerIndices = decodeDeltaRleUsize(nodeColumns[1]!);
  const lastSetCounters = decodeDeltaRleI32(nodeColumns[2]!);
  const lastSetLamportSubs = decodeDeltaRleI32(nodeColumns[3]!);
  const fractionalIndexIndices = decodePostcardUsizeVector(nodeColumns[4]!);
  const length = nodePeerIndices.length;
  decodeAssert(
    parentIndices.length === length &&
      lastSetPeerIndices.length === length &&
      lastSetCounters.length === length &&
      lastSetLamportSubs.length === length &&
      fractionalIndexIndices.length === length,
    "tree state node column length mismatch",
  );
  const positions = decodePositionArena(reader.readBytes());
  const reserved = reader.readBytes();
  reader.assertEnd();
  const peerCount = BigInt(peers.length);
  const parentIndexBound = BigInt(length + 1);
  const nodes = nodePeerIndices.map((peerIndex, index) => {
    const lastSetPeerIndex = lastSetPeerIndices[index]!;
    const parentIndexPlusTwo = parentIndices[index]!;
    const fractionalIndexIndex = fractionalIndexIndices[index]!;
    assertDecodedPeerIndex(peerIndex, peerCount, "tree node ID");
    assertDecodedPeerIndex(lastSetPeerIndex, peerCount, "tree last-set ID");
    decodeAssert(
      parentIndexPlusTwo <= parentIndexBound,
      "tree parent index out of range",
    );
    decodeAssert(
      fractionalIndexIndex < positions.length,
      "tree fractional index out of range",
    );
    return {
      peerIndex,
      counter: nodeCounters[index]!,
      parentIndexPlusTwo,
      lastSetPeerIndex,
      lastSetCounter: lastSetCounters[index]!,
      lastSetLamportSub: lastSetLamportSubs[index]!,
      fractionalIndexIndex,
    };
  });
  return { kind: ContainerType.Tree, peers, nodes, positions, reserved };
}

export function encodeTreeStateSnapshot(state: TreeStateSnapshot): Uint8Array {
  validateEncodedTree(state);
  const nodes = state.nodes;
  const nodePeerIndices: bigint[] = new Array(nodes.length);
  const nodeCounters: number[] = new Array(nodes.length);
  const parentIndices: bigint[] = new Array(nodes.length);
  const lastSetPeerIndices: bigint[] = new Array(nodes.length);
  const lastSetCounters: number[] = new Array(nodes.length);
  const lastSetLamportSubs: number[] = new Array(nodes.length);
  const fractionalIndexIndices: number[] = new Array(nodes.length);
  for (let index = 0; index < nodes.length; index += 1) {
    const node = nodes[index]!;
    nodePeerIndices[index] = node.peerIndex;
    nodeCounters[index] = node.counter;
    parentIndices[index] = node.parentIndexPlusTwo;
    lastSetPeerIndices[index] = node.lastSetPeerIndex;
    lastSetCounters[index] = node.lastSetCounter;
    lastSetLamportSubs[index] = node.lastSetLamportSub;
    fractionalIndexIndices[index] = node.fractionalIndexIndex;
  }
  const output = new ByteWriter();
  writePeerTable(output, state.peers);
  const postcard = new PostcardWriter(output);
  postcard.writeUsize(4);
  output.writeBytes(
    encodeColumnarVec([
      encodeDeltaRleUsize(nodePeerIndices),
      encodeDeltaRleI32(nodeCounters),
    ]),
  );
  output.writeBytes(
    encodeColumnarVec([
      encodeDeltaRleUsize(parentIndices),
      encodeDeltaRleUsize(lastSetPeerIndices),
      encodeDeltaRleI32(lastSetCounters),
      encodeDeltaRleI32(lastSetLamportSubs),
      encodePostcardUsizeVector(fractionalIndexIndices),
    ]),
  );
  postcard.writeBytes(encodePositionArena(state.positions, { encodeEmpty: true }));
  postcard.writeBytes(state.reserved);
  return output.toUint8Array();
}

export function decodeMovableListStateSnapshot(
  bytes: Uint8Array,
): MovableListStateSnapshot {
  const reader = new PostcardReader(bytes);
  const values = readPostcardValues(reader);
  const peers = readPeerTable(reader.input);
  decodeAssert(
    reader.readUsize() === 4,
    "movable-list state must have four encoded fields",
  );
  const itemColumns = readColumnarVecFrom(reader.input);
  decodeAssert(itemColumns.length === 3, "movable-list state item column count");
  const invisibleListItems = decodeDeltaRleUsize(itemColumns[0]!);
  const positionIdEqualsElementId = decodeBoolRle(itemColumns[1]!);
  const elementIdEqualsLastSetId = decodeBoolRle(itemColumns[2]!);
  decodeAssert(
    invisibleListItems.length === positionIdEqualsElementId.length &&
      invisibleListItems.length === elementIdEqualsLastSetId.length,
    "movable-list state item column length mismatch",
  );

  const listIdColumns = readColumnarVecFrom(reader.input);
  decodeAssert(listIdColumns.length === 3, "movable-list list ID column count");
  const listPeerIndices = decodeDeltaRleUsize(listIdColumns[0]!);
  const listCounters = decodeDeltaRleI32(listIdColumns[1]!);
  const listLamportSubs = decodeDeltaRleI32(listIdColumns[2]!);
  decodeAssert(
    listPeerIndices.length === listCounters.length &&
      listPeerIndices.length === listLamportSubs.length,
    "movable-list list ID column length mismatch",
  );

  const elementIdColumns = readColumnarVecFrom(reader.input);
  decodeAssert(elementIdColumns.length === 2, "movable-list element ID column count");
  const elementPeerIndices = decodeDeltaRleUsize(elementIdColumns[0]!);
  const elementLamports = decodeDeltaRleU32(elementIdColumns[1]!);
  decodeAssert(
    elementPeerIndices.length === elementLamports.length,
    "movable-list element ID column length mismatch",
  );

  const lastSetIdColumns = readColumnarVecFrom(reader.input);
  decodeAssert(lastSetIdColumns.length === 2, "movable-list last-set ID column count");
  const lastSetPeerIndices = decodeDeltaRleUsize(lastSetIdColumns[0]!);
  const lastSetLamports = decodeDeltaRleU32(lastSetIdColumns[1]!);
  decodeAssert(
    lastSetPeerIndices.length === lastSetLamports.length,
    "movable-list last-set ID column length mismatch",
  );
  reader.assertEnd();

  const items = invisibleListItems.map((invisibleListItemsForItem, index) => ({
    invisibleListItems: invisibleListItemsForItem,
    positionIdEqualsElementId: positionIdEqualsElementId[index]!,
    elementIdEqualsLastSetId: elementIdEqualsLastSetId[index]!,
  }));
  const listItemIds = listPeerIndices.map((peerIndex, index) => ({
    peerIndex,
    counter: listCounters[index]!,
    lamportSub: listLamportSubs[index]!,
  }));
  const elementIds = elementPeerIndices.map((peerIndex, index) => ({
    peerIndex,
    lamport: elementLamports[index]!,
  }));
  const lastSetIds = lastSetPeerIndices.map((peerIndex, index) => ({
    peerIndex,
    lamport: lastSetLamports[index]!,
  }));
  validateDecodedMovableList(
    values.length,
    BigInt(peers.length),
    items,
    listItemIds,
    elementIds,
    lastSetIds,
  );
  return {
    kind: ContainerType.MovableList,
    values,
    peers,
    items,
    listItemIds,
    elementIds,
    lastSetIds,
  };
}

export function encodeMovableListStateSnapshot(
  state: MovableListStateSnapshot,
): Uint8Array {
  validateEncodedMovableList(state);
  const items = state.items;
  const invisibleListItems: bigint[] = new Array(items.length);
  const positionIdEqualsElementId: boolean[] = new Array(items.length);
  const elementIdEqualsLastSetId: boolean[] = new Array(items.length);
  for (let index = 0; index < items.length; index += 1) {
    const item = items[index]!;
    invisibleListItems[index] = item.invisibleListItems;
    positionIdEqualsElementId[index] = item.positionIdEqualsElementId;
    elementIdEqualsLastSetId[index] = item.elementIdEqualsLastSetId;
  }
  const listItemIds = state.listItemIds;
  const listPeerIndices: bigint[] = new Array(listItemIds.length);
  const listCounters: number[] = new Array(listItemIds.length);
  const listLamportSubs: number[] = new Array(listItemIds.length);
  for (let index = 0; index < listItemIds.length; index += 1) {
    const id = listItemIds[index]!;
    listPeerIndices[index] = id.peerIndex;
    listCounters[index] = id.counter;
    listLamportSubs[index] = id.lamportSub;
  }
  const elementIds = state.elementIds;
  const elementPeerIndices: bigint[] = new Array(elementIds.length);
  const elementLamports: number[] = new Array(elementIds.length);
  for (let index = 0; index < elementIds.length; index += 1) {
    const id = elementIds[index]!;
    elementPeerIndices[index] = id.peerIndex;
    elementLamports[index] = id.lamport;
  }
  const lastSetIds = state.lastSetIds;
  const lastSetPeerIndices: bigint[] = new Array(lastSetIds.length);
  const lastSetLamports: number[] = new Array(lastSetIds.length);
  for (let index = 0; index < lastSetIds.length; index += 1) {
    const id = lastSetIds[index]!;
    lastSetPeerIndices[index] = id.peerIndex;
    lastSetLamports[index] = id.lamport;
  }
  const output = new ByteWriter();
  const postcard = new PostcardWriter(output);
  writePostcardValues(postcard, state.values);
  writePeerTable(output, state.peers);
  postcard.writeUsize(4);
  output.writeBytes(
    encodeColumnarVec([
      encodeDeltaRleUsize(invisibleListItems),
      encodeBoolRle(positionIdEqualsElementId),
      encodeBoolRle(elementIdEqualsLastSetId),
    ]),
  );
  output.writeBytes(
    encodeColumnarVec([
      encodeDeltaRleUsize(listPeerIndices),
      encodeDeltaRleI32(listCounters),
      encodeDeltaRleI32(listLamportSubs),
    ]),
  );
  output.writeBytes(
    encodeColumnarVec([
      encodeDeltaRleUsize(elementPeerIndices),
      encodeDeltaRleU32(elementLamports),
    ]),
  );
  output.writeBytes(
    encodeColumnarVec([
      encodeDeltaRleUsize(lastSetPeerIndices),
      encodeDeltaRleU32(lastSetLamports),
    ]),
  );
  return output.toUint8Array();
}

export function decodeCounterStateSnapshot(bytes: Uint8Array): CounterStateSnapshot {
  decodeAssert(bytes.length === 8, "counter state must contain exactly eight bytes");
  return { kind: ContainerType.Counter, bits: new ByteReader(bytes).readU64LE() };
}

export function encodeCounterStateSnapshot(state: CounterStateSnapshot): Uint8Array {
  const output = new ByteWriter(8);
  output.writeU64LE(state.bits);
  return output.toUint8Array();
}

export function decodeContainerStateSnapshot(
  containerType: ContainerTypeValue,
  bytes: Uint8Array,
): ContainerStateSnapshot {
  switch (containerType) {
    case ContainerType.Map:
      return decodeMapStateSnapshot(bytes);
    case ContainerType.List:
      return decodeListStateSnapshot(bytes);
    case ContainerType.Text:
      return decodeTextStateSnapshot(bytes);
    case ContainerType.Tree:
      return decodeTreeStateSnapshot(bytes);
    case ContainerType.MovableList:
      return decodeMovableListStateSnapshot(bytes);
    case ContainerType.Counter:
      return decodeCounterStateSnapshot(bytes);
    default:
      return { kind: containerType, payload: bytes.slice() };
  }
}

export function encodeContainerStateSnapshot(state: ContainerStateSnapshot): Uint8Array {
  if (typeof state.kind !== "string") {
    return state.payload.slice();
  }
  switch (state.kind) {
    case ContainerType.Map:
      return encodeMapStateSnapshot(state);
    case ContainerType.List:
      return encodeListStateSnapshot(state);
    case ContainerType.Text:
      return encodeTextStateSnapshot(state);
    case ContainerType.Tree:
      return encodeTreeStateSnapshot(state);
    case ContainerType.MovableList:
      return encodeMovableListStateSnapshot(state);
    case ContainerType.Counter:
      return encodeCounterStateSnapshot(state);
  }
}

export function decodeContainerStateWrapper(bytes: Uint8Array): ContainerStateWrapper {
  const input = new ByteReader(bytes);
  decodeAssert(input.remaining > 0, "container state wrapper is empty");
  const containerType = containerTypeFromRawByte(input.readU8());
  const depth = readUleb128(input, U64_MAX);
  const postcard = new PostcardReader(input);
  const parent = readPostcardOptionalContainerId(postcard);
  const state = decodeContainerStateSnapshot(containerType, input.readRemaining());
  return { containerType, depth, parent, state };
}

export function encodeContainerStateWrapper(wrapper: ContainerStateWrapper): Uint8Array {
  encodeAssert(
    sameContainerType(wrapper.containerType, wrapper.state.kind),
    "container wrapper type does not match its state",
  );
  const output = new ByteWriter();
  output.writeU8(containerTypeToRawByte(wrapper.containerType));
  writeUleb128(output, wrapper.depth);
  writePostcardOptionalContainerId(new PostcardWriter(output), wrapper.parent);
  output.writeBytes(encodeContainerStateSnapshot(wrapper.state));
  return output.toUint8Array();
}

export function decodeStateSnapshotStore(
  bytes: Uint8Array,
  options?: DecodeSstableOptions,
): StateSnapshotStore {
  if (bytes.length === 0) {
    return { kind: "absent" };
  }
  if (bytesEqual(bytes, EMPTY_STATE_SENTINEL)) {
    return { kind: "empty" };
  }
  const containers: StateSnapshotContainerEntry[] = [];
  let frontiers: Frontiers | undefined;
  for (const entry of decodeSstable(bytes, options)) {
    if (bytesEqual(entry.key, FRONTIERS_KEY)) {
      decodeAssert(frontiers === undefined, "duplicate state frontiers entry");
      frontiers = decodePostcardFrontiers(entry.value);
      continue;
    }
    const id = decodeContainerId(entry.key);
    const wrapper = decodeContainerStateWrapper(entry.value);
    decodeAssert(
      sameContainerType(id.containerType, wrapper.containerType),
      "state container key and wrapper types differ",
    );
    containers.push({ id, wrapper });
  }
  return { kind: "sstable", frontiers, containers };
}

export function encodeStateSnapshotStore(
  store: StateSnapshotStore,
  options?: EncodeSstableOptions,
): Uint8Array {
  if (store.kind === "absent") {
    return new Uint8Array();
  }
  if (store.kind === "empty") {
    return EMPTY_STATE_SENTINEL.slice();
  }
  const entries = store.containers.map(({ id, wrapper }) => {
    encodeAssert(
      sameContainerType(id.containerType, wrapper.containerType),
      "state container key and wrapper types differ",
    );
    return { key: encodeContainerId(id), value: encodeContainerStateWrapper(wrapper) };
  });
  if (store.frontiers !== undefined) {
    entries.push({ key: FRONTIERS_KEY, value: encodePostcardFrontiers(store.frontiers) });
  }
  encodeAssert(entries.length > 0, "an SSTable state store must not be empty");
  return encodeSstable(entries, options);
}

function readPeerTable(reader: ByteReader): bigint[] {
  const length = readUlebNumber(reader, MAX_STATE_ITEMS);
  decodeAssert(
    length <= Math.floor(reader.remaining / 8),
    "state peer table is truncated",
  );
  const peers: bigint[] = [];
  for (let index = 0; index < length; index += 1) {
    peers.push(reader.readU64LE());
  }
  return peers;
}

function writePeerTable(writer: ByteWriter, peers: readonly bigint[]): void {
  encodeAssert(peers.length <= MAX_STATE_ITEMS, "state peer table is too large");
  writeUleb128(writer, peers.length);
  for (const peer of peers) {
    writer.writeU64LE(peer);
  }
}

function readColumnarVecFrom(reader: ByteReader): Uint8Array[] {
  const remaining = reader.bytes.subarray(
    reader.position,
    reader.position + reader.remaining,
  );
  const [columns, rest] = takeColumnarVec(remaining);
  reader.skip(remaining.length - rest.length);
  return columns;
}

function readStringVector(reader: PostcardReader, label: string): string[] {
  const length = reader.readUsize();
  decodeAssert(length <= MAX_STATE_ITEMS, `${label} is too large`);
  decodeAssert(length <= reader.input.remaining, `${label} exceeds remaining input`);
  const values: string[] = [];
  for (let index = 0; index < length; index += 1) {
    values.push(reader.readString());
  }
  return values;
}

function decodePostcardUsizeVector(bytes: Uint8Array): number[] {
  const reader = new PostcardReader(bytes);
  const length = reader.readUsize();
  decodeAssert(length <= MAX_STATE_ITEMS, "postcard usize vector is too large");
  decodeAssert(length <= reader.input.remaining, "postcard usize vector is truncated");
  const values: number[] = [];
  for (let index = 0; index < length; index += 1) {
    values.push(reader.readUsize());
  }
  reader.assertEnd();
  return values;
}

function encodePostcardUsizeVector(values: readonly number[]): Uint8Array {
  const writer = new PostcardWriter();
  writer.writeUsize(values.length);
  for (const value of values) {
    writer.writeUsize(value);
  }
  return writer.toUint8Array();
}

function collectSortedMapKeysForDecode(
  values: readonly (readonly [string, EncodedLoroValue])[],
  deletedKeys: readonly string[],
): string[] {
  const keys = mergeMapKeys(values, deletedKeys);
  for (let index = 1; index < keys.length; index += 1) {
    decodeAssert(keys[index] !== keys[index - 1], "duplicate map state key");
  }
  return keys;
}

function collectSortedMapKeysForEncode(
  values: readonly (readonly [string, EncodedLoroValue])[],
  deletedKeys: readonly string[],
): string[] {
  const keys = mergeMapKeys(values, deletedKeys);
  for (let index = 1; index < keys.length; index += 1) {
    encodeAssert(keys[index] !== keys[index - 1], "duplicate map state key");
  }
  return keys;
}

function mergeMapKeys(
  values: readonly (readonly [string, EncodedLoroValue])[],
  deletedKeys: readonly string[],
): string[] {
  const keys: string[] = new Array(values.length + deletedKeys.length);
  let offset = 0;
  for (const [key] of values) {
    keys[offset] = key;
    offset += 1;
  }
  for (const key of deletedKeys) {
    keys[offset] = key;
    offset += 1;
  }
  sortByUtf8Key(keys, (key) => key);
  return keys;
}

/**
 * Sorts `items` in place by the UTF-8 byte order of their keys (interop
 * requires UTF-8 byte order, not UTF-16 code unit order). Each key is encoded
 * exactly once instead of once per comparison.
 */
function sortByUtf8Key<T>(items: T[], keyOf: (item: T) => string): void {
  const decorated = items.map((item) => ({
    bytes: textEncoder.encode(keyOf(item)),
    item,
  }));
  decorated.sort((left, right) => compareBytes(left.bytes, right.bytes));
  for (let index = 0; index < decorated.length; index += 1) {
    items[index] = decorated[index]!.item;
  }
}

function assertDecodedPeerIndex(index: bigint, peerCount: bigint, label: string): void {
  decodeAssert(index < peerCount, `${label} peer index out of range`);
}

function assertEncodedPeerIndex(index: bigint, peerCount: bigint, label: string): void {
  encodeAssert(index >= 0n && index < peerCount, `${label} peer index out of range`);
}

function validateDecodedText(
  text: string,
  spans: readonly TextStateSpan[],
  marks: readonly TextStateMark[],
): void {
  let markCount = 0;
  let textLength = 0;
  for (const span of spans) {
    if (span.length === 0) {
      markCount += 1;
    } else if (span.length > 0) {
      textLength += span.length;
    }
  }
  decodeAssert(markCount === marks.length, "text state mark count mismatch");
  decodeAssert(
    unicodeScalarLength(text) === textLength,
    "text state Unicode length mismatch",
  );
}

function validateEncodedText(state: TextStateSnapshot): void {
  let markCount = 0;
  let textLength = 0;
  const peerCount = BigInt(state.peers.length);
  for (const span of state.spans) {
    assertEncodedPeerIndex(span.peerIndex, peerCount, "text state span");
    if (span.length === 0) {
      markCount += 1;
    } else if (span.length > 0) {
      textLength += span.length;
    }
  }
  encodeAssert(markCount === state.marks.length, "text state mark count mismatch");
  encodeAssert(
    unicodeScalarLength(state.text) === textLength,
    "text state Unicode length mismatch",
  );
  for (const mark of state.marks) {
    encodeAssert(
      mark.keyIndex >= 0 && mark.keyIndex < state.keys.length,
      "text mark key index out of range",
    );
  }
}

function unicodeScalarLength(value: string): number {
  let length = 0;
  for (let index = 0; index < value.length; index += 1) {
    const codeUnit = value.charCodeAt(index);
    if (
      codeUnit >= 0xd800 &&
      codeUnit <= 0xdbff &&
      index + 1 < value.length &&
      value.charCodeAt(index + 1) >= 0xdc00 &&
      value.charCodeAt(index + 1) <= 0xdfff
    ) {
      index += 1;
    }
    length += 1;
  }
  return length;
}

function validateEncodedTree(state: TreeStateSnapshot): void {
  const peerCount = BigInt(state.peers.length);
  const parentIndexBound = BigInt(state.nodes.length + 1);
  for (const node of state.nodes) {
    assertEncodedPeerIndex(node.peerIndex, peerCount, "tree node ID");
    assertEncodedPeerIndex(node.lastSetPeerIndex, peerCount, "tree last-set ID");
    encodeAssert(
      node.parentIndexPlusTwo >= 0n && node.parentIndexPlusTwo <= parentIndexBound,
      "tree parent index out of range",
    );
    encodeAssert(
      Number.isSafeInteger(node.fractionalIndexIndex) &&
        node.fractionalIndexIndex >= 0 &&
        node.fractionalIndexIndex < state.positions.length,
      "tree fractional index out of range",
    );
  }
}

function validateDecodedMovableList(
  valueCount: number,
  peerCount: bigint,
  items: readonly MovableListStateItem[],
  listItemIds: readonly ListStateItemId[],
  elementIds: readonly MovableListStateLamportId[],
  lastSetIds: readonly MovableListStateLamportId[],
): void {
  const visibleCount = items.length === 0 ? 0 : items.length - 1;
  decodeAssert(valueCount === visibleCount, "movable-list visible value count mismatch");
  // Decoded usize values are non-negative, so the Number running total stays
  // exact whenever it could equal an actual array length.
  let expectedListIds = visibleCount;
  for (const item of items) {
    expectedListIds += Number(item.invisibleListItems);
  }
  decodeAssert(
    expectedListIds === listItemIds.length,
    "movable-list list ID count mismatch",
  );
  let expectedElementIds = 0;
  let expectedLastSetIds = 0;
  for (let index = 1; index < items.length; index += 1) {
    if (!items[index]!.positionIdEqualsElementId) {
      expectedElementIds += 1;
    }
    if (!items[index]!.elementIdEqualsLastSetId) {
      expectedLastSetIds += 1;
    }
  }
  decodeAssert(
    elementIds.length === expectedElementIds,
    "movable-list element ID count mismatch",
  );
  decodeAssert(
    lastSetIds.length === expectedLastSetIds,
    "movable-list last-set ID count mismatch",
  );
  for (const id of listItemIds) {
    assertDecodedPeerIndex(id.peerIndex, peerCount, "movable-list ID");
  }
  for (const id of elementIds) {
    assertDecodedPeerIndex(id.peerIndex, peerCount, "movable-list ID");
  }
  for (const id of lastSetIds) {
    assertDecodedPeerIndex(id.peerIndex, peerCount, "movable-list ID");
  }
}

function validateEncodedMovableList(state: MovableListStateSnapshot): void {
  const visibleCount = state.items.length === 0 ? 0 : state.items.length - 1;
  encodeAssert(
    state.values.length === visibleCount,
    "movable-list visible value count mismatch",
  );
  // Items were asserted non-negative, so the Number running total stays exact
  // whenever it could equal an actual array length.
  let expectedListIds = visibleCount;
  for (const item of state.items) {
    encodeAssert(
      item.invisibleListItems >= 0n,
      "movable-list invisible count is negative",
    );
    expectedListIds += Number(item.invisibleListItems);
  }
  encodeAssert(
    expectedListIds === state.listItemIds.length,
    "movable-list list ID count mismatch",
  );
  let expectedElementIds = 0;
  let expectedLastSetIds = 0;
  for (let index = 1; index < state.items.length; index += 1) {
    if (!state.items[index]!.positionIdEqualsElementId) {
      expectedElementIds += 1;
    }
    if (!state.items[index]!.elementIdEqualsLastSetId) {
      expectedLastSetIds += 1;
    }
  }
  encodeAssert(
    state.elementIds.length === expectedElementIds,
    "movable-list element ID count mismatch",
  );
  encodeAssert(
    state.lastSetIds.length === expectedLastSetIds,
    "movable-list last-set ID count mismatch",
  );
  const peerCount = BigInt(state.peers.length);
  for (const id of state.listItemIds) {
    assertEncodedPeerIndex(id.peerIndex, peerCount, "movable-list ID");
  }
  for (const id of state.elementIds) {
    assertEncodedPeerIndex(id.peerIndex, peerCount, "movable-list ID");
  }
  for (const id of state.lastSetIds) {
    assertEncodedPeerIndex(id.peerIndex, peerCount, "movable-list ID");
  }
}

function sameContainerType(left: ContainerTypeValue, right: ContainerTypeValue): boolean {
  return containerTypeToRawByte(left) === containerTypeToRawByte(right);
}
