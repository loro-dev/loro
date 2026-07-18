import { ByteWriter, bytesEqual } from "./bytes";
import {
  type ChangesHeader,
  decodeChangeKeys,
  decodeChangesHeader,
  decodeChangesMetadata,
  decodeContainerArena,
  decodeDeleteStartIdColumns,
  decodeEncodedOperationColumns,
  encodeChangeKeys,
  encodeChangesHeader,
  encodeChangesMetadata,
  encodeContainerArena,
  encodeDeleteStartIds,
  encodeEncodedOperations,
} from "./change-block-tables";
import { decodeEncodedChangeBlock, encodeEncodedChangeBlock } from "./change-block";
import {
  type ChangeLoroValue,
  type ChangeValue,
  decodeChangeValueContent,
  encodeChangeValueContent,
} from "./change-value";
import { LoroDecodeError, LoroEncodeError, decodeAssert } from "./errors";
import { decodePositionArena, encodePositionArena } from "./position-arena";
import {
  ContainerType,
  type ContainerId,
  type ContainerType as ContainerTypeValue,
  type Id,
} from "./types";

const DELETED_TREE_ROOT: Id = {
  peer: 0xffff_ffff_ffff_ffffn,
  counter: 0x7fff_ffff,
};

export interface LamportId {
  readonly peer: bigint;
  readonly lamport: number;
}

export type DecodedOperationContent =
  | { readonly type: "map-insert"; readonly key: string; readonly value: ChangeLoroValue }
  | { readonly type: "map-delete"; readonly key: string }
  | { readonly type: "text-insert"; readonly position: number; readonly value: string }
  | {
      readonly type: "text-delete";
      readonly position: number;
      readonly length: bigint;
      readonly startId: Id;
    }
  | {
      readonly type: "text-mark";
      readonly start: number;
      readonly end: number;
      readonly key: string;
      readonly value: ChangeLoroValue;
      readonly info: number;
    }
  | { readonly type: "text-mark-end" }
  | {
      readonly type: "list-insert";
      readonly position: number;
      readonly values: readonly ChangeLoroValue[];
    }
  | {
      readonly type: "list-delete";
      readonly position: number;
      readonly length: bigint;
      readonly startId: Id;
    }
  | {
      readonly type: "movable-list-insert";
      readonly position: number;
      readonly values: readonly ChangeLoroValue[];
    }
  | {
      readonly type: "movable-list-delete";
      readonly position: number;
      readonly length: bigint;
      readonly startId: Id;
    }
  | {
      readonly type: "movable-list-move";
      readonly from: number;
      readonly to: number;
      readonly elementId: LamportId;
    }
  | {
      readonly type: "movable-list-set";
      readonly elementId: LamportId;
      readonly value: ChangeLoroValue;
    }
  | {
      readonly type: "tree-create";
      readonly subject: Id;
      readonly parent: Id | undefined;
      readonly position: Uint8Array;
    }
  | {
      readonly type: "tree-move";
      readonly subject: Id;
      readonly parent: Id | undefined;
      readonly position: Uint8Array;
    }
  | { readonly type: "tree-delete"; readonly subject: Id }
  | { readonly type: "future"; readonly property: number; readonly value: ChangeValue };

export interface DecodedOperation {
  readonly container: ContainerId;
  readonly counter: number;
  readonly length: number;
  readonly content: DecodedOperationContent;
}

export interface DecodedChange {
  readonly id: Id;
  readonly timestamp: bigint;
  readonly dependencies: readonly Id[];
  readonly lamport: number;
  readonly message: string | undefined;
  readonly operations: readonly DecodedOperation[];
}

export interface DecodedChangeBlock {
  readonly peers: readonly bigint[];
  readonly keys: readonly string[];
  readonly containers: readonly ContainerId[];
  readonly positions: readonly Uint8Array[];
  readonly changes: readonly DecodedChange[];
}

interface MutableTables {
  readonly peers: bigint[];
  readonly peerIndices: Map<bigint, number>;
  readonly keys: string[];
  readonly keyIndices: Map<string, number>;
  readonly containers: ContainerId[];
  readonly positions: Uint8Array[];
}

interface DecodeOperationContext {
  readonly peers: readonly bigint[];
  readonly keys: readonly string[];
  readonly positions: readonly Uint8Array[];
  readonly deleteStartIds: ReturnType<typeof decodeDeleteStartIdColumns>;
  deleteIndex: number;
}

export interface ValidatedChangeBlockRange {
  readonly peer: bigint;
  readonly counterStart: number;
  readonly counterEnd: number;
}

function decodeChangeBlockParts(bytes: Uint8Array) {
  const encoded = decodeEncodedChangeBlock(bytes);
  const header = decodeChangesHeader(encoded.header, {
    changeCount: encoded.changeCount,
    counterStart: encoded.counterStart,
    counterLength: encoded.counterLength,
    lamportStart: encoded.lamportStart,
    lamportLength: encoded.lamportLength,
  });
  const metadata = decodeChangesMetadata(encoded.changeMetadata, encoded.changeCount);
  const keys = decodeChangeKeys(encoded.keys);
  const containers = decodeContainerArena(encoded.containerIds, header.peers, keys);
  const positions = decodePositionArena(encoded.positions);
  const operations = decodeEncodedOperationColumns(encoded.operations);
  const context: DecodeOperationContext = {
    peers: header.peers,
    keys,
    positions,
    deleteStartIds: decodeDeleteStartIdColumns(encoded.deleteStartIds),
    deleteIndex: 0,
  };
  return { encoded, header, metadata, keys, containers, positions, operations, context };
}

export function decodeChangeBlock(bytes: Uint8Array): DecodedChangeBlock {
  const { encoded, header, metadata, keys, containers, positions, operations, context } =
    decodeChangeBlockParts(bytes);
  const mutableOperations: DecodedOperation[][] = Array.from(
    { length: encoded.changeCount },
    () => [],
  );
  let counter = encoded.counterStart | 0;
  let changeIndex = 0;
  let remainingValues = encoded.values;
  const operationId = { peer: header.peer, counter };
  for (let row = 0; row < operations.containerIndices.length; row += 1) {
    const length = operations.lengths[row]!;
    decodeAssert(length > 0 && length <= 0x7fff_ffff, "invalid operation length");
    const containerIndex = operations.containerIndices[row]!;
    decodeAssert(containerIndex < containers.length, "invalid operation container index");
    const container = containers[containerIndex]!;
    const [value, remaining] = decodeChangeValueContent(
      operations.valueTypes[row]!,
      remainingValues,
    );
    remainingValues = remaining;
    operationId.counter = counter;
    const content = decodeOperationContent(
      container,
      operations.properties[row]!,
      value,
      operationId,
      context,
    );
    mutableOperations[changeIndex]!.push({
      container,
      counter,
      length,
      content,
    });
    counter = checkedCounter(counter + length);
    const nextBoundary = header.counters[changeIndex + 1];
    decodeAssert(nextBoundary !== undefined, "operation exceeds change boundaries");
    decodeAssert(counter <= nextBoundary, "operation crosses a change boundary");
    if (counter === nextBoundary && changeIndex + 1 < encoded.changeCount) {
      changeIndex += 1;
    }
  }
  decodeAssert(remainingValues.length === 0, "trailing change value bytes");
  decodeAssert(
    context.deleteIndex === context.deleteStartIds.peerIndices.length,
    "unused delete start IDs",
  );
  decodeAssert(
    counter === (encoded.counterStart | 0) + encoded.counterLength,
    "operation lengths do not fill the block counter range",
  );

  const changes: DecodedChange[] = [];
  for (let index = 0; index < encoded.changeCount; index += 1) {
    changes.push({
      id: { peer: header.peer, counter: header.counters[index]! },
      timestamp: metadata.timestamps[index]!,
      dependencies: header.dependencies[index]!,
      lamport: header.lamports[index]!,
      message: metadata.commitMessages[index],
      operations: mutableOperations[index]!,
    });
  }
  return { peers: header.peers, keys, containers, positions, changes };
}

/** Fully validates a block while retaining only its covered counter range. */
export function validateChangeBlock(bytes: Uint8Array): ValidatedChangeBlockRange {
  const { encoded, header, containers, operations, context } =
    decodeChangeBlockParts(bytes);
  let counter = encoded.counterStart | 0;
  let changeIndex = 0;
  let remainingValues = encoded.values;
  const operationId = { peer: header.peer, counter };
  for (let row = 0; row < operations.containerIndices.length; row += 1) {
    const length = operations.lengths[row]!;
    decodeAssert(length > 0 && length <= 0x7fff_ffff, "invalid operation length");
    const containerIndex = operations.containerIndices[row]!;
    decodeAssert(containerIndex < containers.length, "invalid operation container index");
    const [value, remaining] = decodeChangeValueContent(
      operations.valueTypes[row]!,
      remainingValues,
    );
    remainingValues = remaining;
    operationId.counter = counter;
    decodeOperationContent(
      containers[containerIndex]!,
      operations.properties[row]!,
      value,
      operationId,
      context,
      false,
    );
    counter = checkedCounter(counter + length);
    const nextBoundary = header.counters[changeIndex + 1];
    decodeAssert(nextBoundary !== undefined, "operation exceeds change boundaries");
    decodeAssert(counter <= nextBoundary, "operation crosses a change boundary");
    if (counter === nextBoundary && changeIndex + 1 < encoded.changeCount) {
      changeIndex += 1;
    }
  }
  decodeAssert(remainingValues.length === 0, "trailing change value bytes");
  decodeAssert(
    context.deleteIndex === context.deleteStartIds.peerIndices.length,
    "unused delete start IDs",
  );
  const counterEnd = checkedCounter((encoded.counterStart | 0) + encoded.counterLength);
  decodeAssert(
    counter === counterEnd,
    "operation lengths do not fill the block counter range",
  );
  return { peer: header.peer, counterStart: encoded.counterStart | 0, counterEnd };
}

export function encodeChangeBlock(block: DecodedChangeBlock): Uint8Array {
  if (block.changes.length === 0) {
    throw new LoroEncodeError("cannot encode an empty change block");
  }
  const firstPeer = block.changes[0]!.id.peer;
  const tables = initializeTables(block);
  if (tables.peers.length === 0) {
    registerPeer(tables, firstPeer);
  } else if (tables.peers[0] !== firstPeer) {
    throw new LoroEncodeError("the first peer table entry must be the block peer");
  }

  const rows: {
    containerIndex: number;
    property: number;
    valueType: number;
    length: number;
  }[] = [];
  const deleteStartIds: {
    peerIndex: bigint;
    counter: number;
    length: bigint;
  }[] = [];
  const valueWriter = new ByteWriter();
  const lengths: number[] = [];
  const counters: number[] = [];
  let expectedCounter = block.changes[0]!.id.counter;
  for (const change of block.changes) {
    if (change.id.peer !== firstPeer || change.id.counter !== expectedCounter) {
      throw new LoroEncodeError("changes must be continuous and belong to one peer");
    }
    counters.push(change.id.counter);
    let atomLength = 0;
    for (const operation of change.operations) {
      if (operation.counter !== expectedCounter + atomLength) {
        throw new LoroEncodeError("operation counters are not continuous");
      }
      assertOperationLength(operation.length);
      const containerIndex = registerContainer(tables, operation.container);
      const encodedContent = encodeOperationContent(
        operation.content,
        tables,
        deleteStartIds,
      );
      const encodedValue = encodeChangeValueContent(encodedContent.value);
      valueWriter.writeBytes(encodedValue.bytes);
      rows.push({
        containerIndex,
        property: encodedContent.property,
        valueType: encodedValue.tag,
        length: operation.length,
      });
      atomLength += operation.length;
      if (atomLength > 0x7fff_ffff) {
        throw new LoroEncodeError("change atom length is out of range");
      }
    }
    if (atomLength === 0) {
      throw new LoroEncodeError("changes must contain at least one operation atom");
    }
    lengths.push(atomLength);
    expectedCounter = checkedEncodeCounter(expectedCounter + atomLength);
  }
  counters.push(expectedCounter);

  for (const container of tables.containers) {
    if (container.kind === "root") {
      registerKey(tables, container.name);
    } else {
      registerPeer(tables, container.peer);
    }
  }
  for (const change of block.changes) {
    for (const dependency of change.dependencies) {
      registerPeer(tables, dependency.peer);
    }
  }

  const header: ChangesHeader = {
    peer: firstPeer,
    peers: tables.peers,
    counters,
    lengths,
    lamports: block.changes.map((change) => change.lamport),
    dependencies: block.changes.map((change) => change.dependencies),
  };
  const counterStart = block.changes[0]!.id.counter;
  if (counterStart < 0) {
    throw new LoroEncodeError("change block counter start must be nonnegative");
  }
  const counterLength = lengths.reduce((sum, length) => sum + length, 0);
  const lamportStart = block.changes[0]!.lamport;
  const finalChange = block.changes[block.changes.length - 1]!;
  const lamportEnd = finalChange.lamport + lengths[lengths.length - 1]!;
  const lamportLength = lamportEnd - lamportStart;
  if (
    !Number.isSafeInteger(lamportStart) ||
    lamportStart < 0 ||
    lamportStart > 0xffff_ffff ||
    lamportLength < 0 ||
    lamportLength > 0xffff_ffff
  ) {
    throw new LoroEncodeError("change block lamport range is invalid");
  }
  return encodeEncodedChangeBlock({
    counterStart,
    counterLength,
    lamportStart,
    lamportLength,
    changeCount: block.changes.length,
    header: encodeChangesHeader(header),
    changeMetadata: encodeChangesMetadata({
      timestamps: block.changes.map((change) => change.timestamp),
      commitMessages: block.changes.map((change) => change.message),
    }),
    containerIds: encodeContainerArena(tables.containers, tables.peers, tables.keys),
    keys: encodeChangeKeys(tables.keys),
    positions: encodePositionArena(tables.positions),
    operations: encodeEncodedOperations(rows),
    deleteStartIds: encodeDeleteStartIds(deleteStartIds),
    values: valueWriter.toUint8Array(),
  });
}

function decodeOperationContent(
  container: ContainerId,
  property: number,
  value: ChangeValue,
  operationId: Id,
  context: DecodeOperationContext,
): DecodedOperationContent;
function decodeOperationContent(
  container: ContainerId,
  property: number,
  value: ChangeValue,
  operationId: Id,
  context: DecodeOperationContext,
  materialize: false,
): undefined;
function decodeOperationContent(
  container: ContainerId,
  property: number,
  value: ChangeValue,
  operationId: Id,
  context: DecodeOperationContext,
  materialize = true,
): DecodedOperationContent | undefined {
  const type = container.containerType;
  if (type === ContainerType.Map) {
    const key = getIndex(context.keys, property, "map key");
    if (value.type === "delete-once") {
      return materialize ? { type: "map-delete", key } : undefined;
    }
    if (value.type === "loro-value") {
      return materialize ? { type: "map-insert", key, value: value.value } : undefined;
    }
    throw new LoroDecodeError("invalid map operation value");
  }
  if (type === ContainerType.Text) {
    if (value.type === "string") {
      decodeAssert(property >= 0, "invalid text insertion position");
      return materialize
        ? { type: "text-insert", position: property, value: value.value }
        : undefined;
    }
    if (value.type === "delete-sequence") {
      const deletion = takeDeletion(context, materialize);
      return materialize
        ? {
            type: "text-delete",
            position: property,
            length: deletion!.length,
            startId: deletion!.startId,
          }
        : undefined;
    }
    if (value.type === "mark-start") {
      decodeAssert(property >= 0, "invalid text mark position");
      const length = bigintToIndex(value.length, 0xffff_ffff, "text mark length");
      const keyIndex = bigintToIndex(
        value.keyIndex,
        context.keys.length - 1,
        "text mark key index",
      );
      const end = property + length;
      decodeAssert(end <= 0xffff_ffff, "text mark end is out of range");
      return materialize
        ? {
            type: "text-mark",
            start: property,
            end,
            key: context.keys[keyIndex]!,
            value: value.value,
            info: value.info,
          }
        : undefined;
    }
    if (value.type === "null") {
      return materialize ? { type: "text-mark-end" } : undefined;
    }
    throw new LoroDecodeError("invalid text operation value");
  }
  if (type === ContainerType.List) {
    if (value.type === "loro-value" && value.value.type === "list") {
      decodeAssert(property >= 0, "invalid list insertion position");
      return materialize
        ? { type: "list-insert", position: property, values: value.value.value }
        : undefined;
    }
    if (value.type === "delete-sequence") {
      const deletion = takeDeletion(context, materialize);
      return materialize
        ? {
            type: "list-delete",
            position: property,
            length: deletion!.length,
            startId: deletion!.startId,
          }
        : undefined;
    }
    throw new LoroDecodeError("invalid list operation value");
  }
  if (type === ContainerType.MovableList) {
    if (value.type === "loro-value" && value.value.type === "list") {
      decodeAssert(property >= 0, "invalid movable-list insertion position");
      return materialize
        ? {
            type: "movable-list-insert",
            position: property,
            values: value.value.value,
          }
        : undefined;
    }
    if (value.type === "delete-sequence") {
      const deletion = takeDeletion(context, materialize);
      return materialize
        ? {
            type: "movable-list-delete",
            position: property,
            length: deletion!.length,
            startId: deletion!.startId,
          }
        : undefined;
    }
    if (value.type === "list-move") {
      decodeAssert(property >= 0, "invalid movable-list destination");
      const from = bigintToIndex(value.from, 0xffff_ffff, "movable-list source");
      const peer =
        context.peers[
          bigintToIndex(
            value.fromPeerIndex,
            context.peers.length - 1,
            "movable-list peer index",
          )
        ]!;
      const lamport = bigintToIndex(value.lamport, 0xffff_ffff, "movable-list lamport");
      return materialize
        ? {
            type: "movable-list-move",
            from,
            to: property,
            elementId: { peer, lamport },
          }
        : undefined;
    }
    if (value.type === "list-set") {
      const peer =
        context.peers[
          bigintToIndex(
            value.peerIndex,
            context.peers.length - 1,
            "movable-list peer index",
          )
        ]!;
      return materialize
        ? {
            type: "movable-list-set",
            elementId: { peer, lamport: value.lamport },
            value: value.value,
          }
        : undefined;
    }
    throw new LoroDecodeError("invalid movable-list operation value");
  }
  if (type === ContainerType.Tree) {
    if (value.type !== "raw-tree-move") {
      throw new LoroDecodeError("invalid tree operation value");
    }
    const subjectPeer =
      context.peers[
        bigintToIndex(
          value.subjectPeerIndex,
          context.peers.length - 1,
          "tree subject peer index",
        )
      ]!;
    const parentPeer = value.parentIsNull
      ? undefined
      : context.peers[
          bigintToIndex(
            value.parentPeerIndex,
            context.peers.length - 1,
            "tree parent peer index",
          )
        ]!;
    const deleting =
      parentPeer === DELETED_TREE_ROOT.peer &&
      value.parentCounter === DELETED_TREE_ROOT.counter;
    if (deleting) {
      return materialize
        ? {
            type: "tree-delete",
            subject: { peer: subjectPeer, counter: value.subjectCounter },
          }
        : undefined;
    }
    const position =
      context.positions[
        bigintToIndex(
          value.positionIndex,
          context.positions.length - 1,
          "tree position index",
        )
      ]!;
    if (!materialize) return undefined;
    const subject: Id = { peer: subjectPeer, counter: value.subjectCounter };
    const parent: Id | undefined =
      parentPeer === undefined
        ? undefined
        : { peer: parentPeer, counter: value.parentCounter };
    return idsEqual(subject, operationId)
      ? { type: "tree-create", subject, parent, position }
      : { type: "tree-move", subject, parent, position };
  }
  return materialize ? { type: "future", property, value } : undefined;
}

function encodeOperationContent(
  content: DecodedOperationContent,
  tables: MutableTables,
  deletions: { peerIndex: bigint; counter: number; length: bigint }[],
): { readonly property: number; readonly value: ChangeValue } {
  switch (content.type) {
    case "map-insert":
      return {
        property: registerKey(tables, content.key),
        value: { type: "loro-value", value: content.value },
      };
    case "map-delete":
      return {
        property: registerKey(tables, content.key),
        value: { type: "delete-once" },
      };
    case "text-insert":
      return {
        property: assertNonnegativePosition(content.position),
        value: { type: "string", value: content.value },
      };
    case "text-delete":
      registerDeletion(deletions, tables, content.startId, content.length);
      return { property: content.position, value: { type: "delete-sequence" } };
    case "text-mark": {
      const length = content.end - content.start;
      if (length < 0) {
        throw new LoroEncodeError("text mark end precedes its start");
      }
      return {
        property: assertNonnegativePosition(content.start),
        value: {
          type: "mark-start",
          info: content.info,
          length: BigInt(length),
          keyIndex: BigInt(registerKey(tables, content.key)),
          value: content.value,
        },
      };
    }
    case "text-mark-end":
      return { property: 0, value: { type: "null" } };
    case "list-insert":
      return {
        property: assertNonnegativePosition(content.position),
        value: { type: "loro-value", value: { type: "list", value: content.values } },
      };
    case "list-delete":
      registerDeletion(deletions, tables, content.startId, content.length);
      return { property: content.position, value: { type: "delete-sequence" } };
    case "movable-list-insert":
      return {
        property: assertNonnegativePosition(content.position),
        value: { type: "loro-value", value: { type: "list", value: content.values } },
      };
    case "movable-list-delete":
      registerDeletion(deletions, tables, content.startId, content.length);
      return { property: content.position, value: { type: "delete-sequence" } };
    case "movable-list-move":
      return {
        property: assertNonnegativePosition(content.to),
        value: {
          type: "list-move",
          from: BigInt(assertNonnegativePosition(content.from)),
          fromPeerIndex: BigInt(registerPeer(tables, content.elementId.peer)),
          lamport: BigInt(content.elementId.lamport),
        },
      };
    case "movable-list-set":
      return {
        property: 0,
        value: {
          type: "list-set",
          peerIndex: BigInt(registerPeer(tables, content.elementId.peer)),
          lamport: content.elementId.lamport,
          value: content.value,
        },
      };
    case "tree-create":
    case "tree-move":
      return encodeTreeMove(content.subject, content.parent, content.position, tables);
    case "tree-delete":
      return encodeTreeMove(
        content.subject,
        DELETED_TREE_ROOT,
        new Uint8Array(),
        tables,
        true,
      );
    case "future":
      return { property: content.property, value: content.value };
  }
}

function encodeTreeMove(
  subject: Id,
  parent: Id | undefined,
  position: Uint8Array,
  tables: MutableTables,
  deleting = false,
): { readonly property: number; readonly value: ChangeValue } {
  return {
    property: 0,
    value: {
      type: "raw-tree-move",
      subjectPeerIndex: BigInt(registerPeer(tables, subject.peer)),
      subjectCounter: subject.counter,
      positionIndex: deleting ? 0n : BigInt(registerPosition(tables, position)),
      parentIsNull: parent === undefined,
      parentPeerIndex:
        parent === undefined ? 0n : BigInt(registerPeer(tables, parent.peer)),
      parentCounter: parent?.counter ?? 0,
    },
  };
}

function takeDeletion(
  context: DecodeOperationContext,
  materialize: boolean,
): { readonly startId: Id; readonly length: bigint } | undefined {
  const peerIndexValue = context.deleteStartIds.peerIndices[context.deleteIndex];
  const counter = context.deleteStartIds.counters[context.deleteIndex];
  const length = context.deleteStartIds.lengths[context.deleteIndex];
  decodeAssert(
    peerIndexValue !== undefined && counter !== undefined && length !== undefined,
    "delete start ID underflow",
  );
  context.deleteIndex += 1;
  const peerIndex = bigintToIndex(
    peerIndexValue,
    context.peers.length - 1,
    "delete start peer index",
  );
  return materialize
    ? {
        startId: { peer: context.peers[peerIndex]!, counter },
        length,
      }
    : undefined;
}

function registerDeletion(
  deletions: { peerIndex: bigint; counter: number; length: bigint }[],
  tables: MutableTables,
  startId: Id,
  length: bigint,
): void {
  deletions.push({
    peerIndex: BigInt(registerPeer(tables, startId.peer)),
    counter: startId.counter,
    length,
  });
}

function initializeTables(block: DecodedChangeBlock): MutableTables {
  const peers = [...block.peers];
  const keys = [...block.keys];
  const peerIndices = new Map<bigint, number>();
  const keyIndices = new Map<string, number>();
  for (let index = 0; index < peers.length; index += 1) {
    if (peerIndices.has(peers[index]!)) {
      throw new LoroEncodeError("duplicate peer table entry");
    }
    peerIndices.set(peers[index]!, index);
  }
  for (let index = 0; index < keys.length; index += 1) {
    if (!keyIndices.has(keys[index]!)) {
      keyIndices.set(keys[index]!, index);
    }
  }
  return {
    peers,
    peerIndices,
    keys,
    keyIndices,
    containers: block.containers.map(cloneContainerId),
    positions: block.positions.map((position) => position.slice()),
  };
}

function registerPeer(tables: MutableTables, peer: bigint): number {
  const current = tables.peerIndices.get(peer);
  if (current !== undefined) {
    return current;
  }
  if (peer < 0n || peer > 0xffff_ffff_ffff_ffffn) {
    throw new LoroEncodeError(`peer ID is out of range: ${peer}`);
  }
  const index = tables.peers.length;
  tables.peers.push(peer);
  tables.peerIndices.set(peer, index);
  return index;
}

function registerKey(tables: MutableTables, key: string): number {
  const current = tables.keyIndices.get(key);
  if (current !== undefined) {
    return current;
  }
  const index = tables.keys.length;
  tables.keys.push(key);
  tables.keyIndices.set(key, index);
  return index;
}

function registerContainer(tables: MutableTables, container: ContainerId): number {
  const index = tables.containers.findIndex((current) =>
    containerIdsEqual(current, container),
  );
  if (index >= 0) {
    return index;
  }
  tables.containers.push(cloneContainerId(container));
  return tables.containers.length - 1;
}

function registerPosition(tables: MutableTables, position: Uint8Array): number {
  const index = tables.positions.findIndex((current) => bytesEqual(current, position));
  if (index >= 0) {
    return index;
  }
  tables.positions.push(position.slice());
  return tables.positions.length - 1;
}

function getIndex<T>(values: readonly T[], index: number, label: string): T {
  decodeAssert(
    Number.isSafeInteger(index) && index >= 0 && index < values.length,
    `invalid ${label} index`,
  );
  return values[index]!;
}

function bigintToIndex(value: bigint, max: number, label: string): number {
  decodeAssert(
    max >= 0 && value >= 0n && value <= BigInt(max),
    `${label} is out of range`,
  );
  return Number(value);
}

function assertNonnegativePosition(value: number): number {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0x7fff_ffff) {
    throw new LoroEncodeError(`position is out of range: ${value}`);
  }
  return value;
}

function assertOperationLength(value: number): void {
  if (!Number.isSafeInteger(value) || value <= 0 || value > 0x7fff_ffff) {
    throw new LoroEncodeError(`operation length is out of range: ${value}`);
  }
}

function checkedCounter(value: number): number {
  decodeAssert(
    Number.isSafeInteger(value) && value >= -0x8000_0000 && value <= 0x7fff_ffff,
    "operation counter overflow",
  );
  return value;
}

function checkedEncodeCounter(value: number): number {
  if (!Number.isSafeInteger(value) || value < -0x8000_0000 || value > 0x7fff_ffff) {
    throw new LoroEncodeError(`operation counter is out of range: ${value}`);
  }
  return value;
}

function idsEqual(left: Id, right: Id): boolean {
  return left.peer === right.peer && left.counter === right.counter;
}

function containerIdsEqual(left: ContainerId, right: ContainerId): boolean {
  if (
    left.kind !== right.kind ||
    !containerTypesEqual(left.containerType, right.containerType)
  ) {
    return false;
  }
  return left.kind === "root" && right.kind === "root"
    ? left.name === right.name
    : left.kind === "normal" &&
        right.kind === "normal" &&
        left.peer === right.peer &&
        left.counter === right.counter;
}

function containerTypesEqual(
  left: ContainerTypeValue,
  right: ContainerTypeValue,
): boolean {
  return typeof left === "string" || typeof right === "string"
    ? left === right
    : left.value === right.value;
}

function cloneContainerId(container: ContainerId): ContainerId {
  return container.kind === "root"
    ? {
        kind: "root",
        name: container.name,
        containerType: container.containerType,
      }
    : {
        kind: "normal",
        peer: container.peer,
        counter: container.counter,
        containerType: container.containerType,
      };
}
