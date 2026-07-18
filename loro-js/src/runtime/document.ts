import packageMetadata from "../../package.json" with { type: "json" };

import { bytesEqual, bytesToHex, hexToBytes } from "../codec/bytes";
import {
  decodeChangeBlock,
  encodeChangeBlock,
  type DecodedChange,
  type DecodedOperation,
  type DecodedOperationContent,
} from "../codec/change-block-codec";
import type { ChangeLoroValue, ChangeValue } from "../codec/change-value";
import { containerTypeFromRawByte, containerTypeToRawByte } from "../codec/container-id";
import {
  decodeDocument,
  decodeFastSnapshotBody,
  decodeFastUpdatesBody,
  encodeDocument,
  encodeFastSnapshotBody,
  encodeFastUpdatesBody,
  EncodeMode,
  type FastSnapshotBody,
  type ParsedDocument,
} from "../codec/document";
import { decodeChangeBlockKey, encodeChangeBlockKey } from "../codec/id";
import { decodeSstable, encodeSstable, type SstableEntry } from "../codec/sstable";
import {
  decodeStateSnapshotStore,
  encodeStateSnapshotStore,
  type ContainerStateSnapshot,
  type MapStateMetadata,
  type StateSnapshotContainerEntry,
  type StateSnapshotStore,
} from "../codec/state-snapshot";
import {
  ContainerType as CodecContainerType,
  type ContainerId as CodecContainerId,
  type EncodedLoroValue,
  type Id as CodecId,
} from "../codec/types";
import {
  decodePostcardFrontiers,
  decodePostcardVersionVector,
  encodePostcardFrontiers,
  encodePostcardVersionVector,
} from "../codec/version";
import {
  cloneRuntimeValue,
  containerDeepValueWithId,
  Cursor,
  isContainer,
  LoroContainer,
  LoroCounter,
  LoroList,
  LoroMap,
  LoroMovableList,
  LoroText,
  LoroTree,
  LoroTreeNode,
  runtimeValueToJson,
  type Container,
  type CausalVersion,
  type LastWriter,
  type RuntimeValue,
  type SequenceElement,
  type SequenceMoveMeta,
  type SequenceValueMeta,
  type TextElement,
  type TextStyleMeta,
  type TreeNodeRecord,
} from "./containers";
import { SequenceEventDiff } from "./event-diff";
import {
  codecTypeToPublic,
  containerIdsEqual,
  formatContainerId,
  formatOpId,
  formatTreeId,
  idsEqual,
  isContainerId,
  parseContainerId,
  parseOpId,
  parsePeerId,
  parseTreeId,
  peerIdToString,
  publicTypeToCodec,
} from "./ids";
import { compileJsonPathEventMatcher, evaluateJsonPath } from "./jsonpath";
import {
  isMergeableContainerId,
  mergeableMarker,
  newMergeableContainerId,
  parseMergeableMarker,
} from "./mergeable";
import { OrderedIndex } from "./ordered-index";
import type { SequenceIdRun } from "./sequence-index";
import type {
  Change,
  CommitOptions,
  ContainerID,
  ContainerType,
  CounterSpan,
  Delta,
  Diff,
  ExportMode,
  Frontiers,
  IdSpan,
  ImportBlobMetadata,
  ImportStatus,
  JsonChange,
  JsonContainerID,
  JsonDiff,
  JsonOp,
  JsonOpContent,
  JsonSchema,
  JsonValue,
  LoroEvent,
  LoroEventBatch,
  OpId,
  Path,
  PeerID,
  PeerIdInput,
  Side,
  Subscription,
  TextStyleConfig,
  TextStyleExpand,
  TreeID,
  TreeDiffItem,
  Value,
  VersionVectorDiff,
} from "./types";
import { VersionVector, type VersionVectorInput } from "./version-vector";

const VERSION_KEY = Uint8Array.of(0x76, 0x76);
const FRONTIERS_KEY = Uint8Array.of(0x66, 0x72);
const START_VERSION_KEY = Uint8Array.of(0x73, 0x76);
const START_FRONTIERS_KEY = Uint8Array.of(0x73, 0x66);
let fallbackPeer = 1n;

interface HistoryRecord {
  readonly change: DecodedChange;
  keys: readonly string[];
  keyIndices?: Map<string, number>;
}

interface IntegrationResult {
  readonly added: HistoryRecord[];
  readonly pending: HistoryRecord[];
}

interface DecodedImportData {
  readonly mode: EncodeMode;
  readonly records: HistoryRecord[];
  readonly snapshot?: FastSnapshotBody;
  readonly startVersion?: VersionVector;
  readonly startFrontiers?: CodecId[];
  readonly endVersion?: VersionVector;
}

interface DeferredSnapshotHistory {
  readonly entries: readonly SstableEntry[];
  readonly validatedBlocks: ReadonlyMap<SstableEntry, readonly HistoryRecord[]>;
  readonly endVersion: VersionVector;
  readonly frontiers: readonly CodecId[];
  readonly operationCount: number;
}

interface IndexedHistoryOperation {
  readonly record: HistoryRecord;
  readonly operation: DecodedOperation;
  readonly writer: LastWriter;
}

interface IndexedSubjectHistory {
  readonly byWriter: OrderedIndex<IndexedHistoryOperation>;
  readonly byPeer: Map<bigint, IndexedHistoryOperation[]>;
  readonly placementsByPeer?: Map<bigint, IndexedHistoryOperation[]>;
}

interface EventRecording {
  readonly beforeValues: Map<string, unknown>;
  readonly eventStates: Map<string, PendingEventState>;
}

type MovableMoveTransitionMode = "anchors" | "replay";

interface PendingChange extends EventRecording {
  readonly id: CodecId;
  readonly dependencies: CodecId[];
  readonly lamport: number;
  readonly from: CodecId[];
  readonly operations: DecodedOperation[];
  operationLength: number;
  readonly causalVersion: Map<bigint, number>;
  readonly keys: string[];
  readonly keyIndices: Map<string, number>;
  readonly changedContainers: Set<string>;
  readonly beforeValues: Map<string, unknown>;
  readonly eventStates: Map<string, PendingEventState>;
}

type PendingEventState =
  | { readonly kind: "sequence"; readonly diff: SequenceEventDiff }
  | {
      readonly kind: "map";
      readonly originals: Map<
        string,
        {
          readonly present: boolean;
          readonly visible: boolean;
          readonly value: unknown;
        }
      >;
    }
  | { readonly kind: "counter"; readonly before: number }
  | {
      readonly kind: "tree";
      readonly originals: Map<TreeID, TreeEventNode | undefined>;
    };

interface ContainerBlueprint {
  readonly kind: ContainerType;
  readonly value: unknown;
}

interface MutableCommitOptions {
  origin?: string;
  timestamp?: number;
  message?: string;
}

export class LoroDoc<T extends Record<string, Container> = Record<string, Container>> {
  #peer = generatePeerId();
  #history = new Map<string, HistoryRecord>();
  #historyOrder = new OrderedIndex<HistoryRecord>(compareHistoryRecords);
  #historyByPeer = new Map<bigint, HistoryRecord[]>();
  #historyEndByPeer = new Map<bigint, number>();
  #historyOperationCount = 0;
  #sortedHistoryCache: HistoryRecord[] | undefined;
  #historyFrontiers = new Map<string, CodecId>();
  #dependencyVersionCache = new WeakMap<DecodedChange, ReadonlyMap<bigint, number>>();
  #mapOperationHistory = new Map<string, Map<string, IndexedSubjectHistory>>();
  #treeOperationHistory = new Map<string, Map<string, IndexedSubjectHistory>>();
  #movableOrderHistory = new Map<string, OrderedIndex<IndexedHistoryOperation>>();
  #movableMovePeers = new Map<string, Set<bigint>>();
  #containersWithOperations = new Set<string>();
  #containerKeys = new WeakMap<CodecContainerId, string>();
  #pendingHistory = new Map<string, HistoryRecord>();
  #deferredSnapshotHistory: DeferredSnapshotHistory | undefined;
  #containers = new Map<string, LoroContainer>();
  #roots = new Map<string, LoroContainer>();
  #pending: PendingChange | undefined;
  #nextCounter = 0;
  #recordTimestamp = false;
  #changeMergeInterval = 1000n;
  #nextCommitOptions: CommitOptions = {};
  #subscribers = new Set<(event: LoroEventBatch) => void>();
  #localUpdateSubscribers = new Set<(bytes: Uint8Array) => void>();
  #containerSubscribers = new Map<string, Set<(event: LoroEventBatch) => void>>();
  #firstCommitSubscribers = new Set<(event: { peer: PeerID }) => void>();
  #preCommitSubscribers = new Set<
    (event: { changeMeta: Change; origin: string; modifier: ChangeModifier }) => void
  >();
  #seenCommittedPeers = new Set<bigint>();
  #committing = false;
  #preCommitRecord: HistoryRecord | undefined;
  #detached = false;
  #detachedEditing = false;
  #checkoutVersion: VersionVector | undefined;
  #hideEmptyRoots = false;
  #shallowStartVersion = new VersionVector();
  #shallowRootVersion = new VersionVector();
  #shallowRootFrontiers: CodecId[] = [];
  #shallowRootStore: StateSnapshotStore | undefined;
  #textStyles = new Map<string, TextStyleExpand>([
    ["bold", "after"],
    ["italic", "after"],
    ["underline", "after"],
    ["link", "none"],
    ["highlight", "none"],
    ["comment", "none"],
    ["code", "none"],
  ]);
  #defaultTextStyle: TextStyleExpand | undefined;

  static fromSnapshot(snapshot: Uint8Array): LoroDoc {
    const doc = new LoroDoc();
    doc.import(snapshot);
    return doc;
  }

  free(): void {}

  get peerId(): bigint {
    return this.#peer;
  }

  get peerIdStr(): PeerID {
    return peerIdToString(this.#peer);
  }

  setPeerId(peer: PeerIdInput): void {
    if (this.#pending !== undefined)
      throw new Error("cannot change peer id with pending operations");
    this.#peer = parsePeerId(peer);
    this.#nextCounter = this.oplogVersion().get(this.#peer) ?? 0;
  }

  setRecordTimestamp(enabled: boolean): void {
    this.#recordTimestamp = enabled;
  }

  setChangeMergeInterval(interval: number): void {
    this.#changeMergeInterval = numberToI64(interval);
  }
  setDetachedEditing(enabled: boolean): void {
    this.#detachedEditing = enabled;
    if (enabled && this.#detached) {
      this.#commit({}, true);
      this.#renewPeerId();
    }
  }
  isDetachedEditingEnabled(): boolean {
    return this.#detachedEditing;
  }
  configTextStyle(styles: Readonly<Record<string, TextStyleConfig>>): void {
    this.#textStyles.clear();
    for (const [key, style] of Object.entries(styles)) {
      if (key.includes(":")) throw new TypeError("text style keys cannot contain ':'");
      assertTextStyleExpand(style.expand);
      this.#textStyles.set(key, style.expand);
    }
  }
  configDefaultTextStyle(style: TextStyleConfig | undefined): void {
    if (style === undefined) {
      this.#defaultTextStyle = undefined;
      return;
    }
    assertTextStyleExpand(style.expand);
    this.#defaultTextStyle = style.expand;
  }

  getMap<Key extends keyof T | ContainerID>(
    name: Key,
  ): T[Key] extends LoroMap ? T[Key] : LoroMap {
    return this.#getContainer(name as string, "Map") as never;
  }

  getList<Key extends keyof T | ContainerID>(
    name: Key,
  ): T[Key] extends LoroList ? T[Key] : LoroList {
    return this.#getContainer(name as string, "List") as never;
  }

  getMovableList<Key extends keyof T | ContainerID>(
    name: Key,
  ): T[Key] extends LoroMovableList ? T[Key] : LoroMovableList {
    return this.#getContainer(name as string, "MovableList") as never;
  }

  getText(name: string): LoroText {
    return this.#getContainer(name, "Text") as LoroText;
  }

  getTree<Key extends keyof T | ContainerID>(
    name: Key,
  ): T[Key] extends LoroTree ? T[Key] : LoroTree {
    return this.#getContainer(name as string, "Tree") as never;
  }

  getCounter(name: string): LoroCounter {
    return this.#getContainer(name, "Counter") as LoroCounter;
  }

  getContainerById(id: ContainerID): Container | undefined {
    const parsed = parseContainerId(id);
    if (parsed.kind === "root" && !isMergeableContainerId(parsed)) {
      return this.#getOrCreateContainer(parsed) as Container;
    }
    const container = this.#containers.get(formatContainerId(parsed));
    if (
      container !== undefined &&
      isMergeableContainerId(parsed) &&
      this._isContainerDeleted(container) &&
      !this.#containerHasOperations(parsed)
    ) {
      return undefined;
    }
    return container as Container | undefined;
  }

  hasContainer(id: ContainerID): boolean {
    return this.getContainerById(id) !== undefined;
  }

  #containerHasOperations(id: CodecContainerId): boolean {
    this.#materializeDeferredHistory();
    return this.#containersWithOperations.has(formatContainerId(id));
  }

  deleteRootContainer(id: ContainerID): void {
    const parsed = parseContainerId(id);
    if (parsed.kind !== "root")
      throw new TypeError("only root containers can be deleted directly");
    this.#roots.delete(parsed.name);
    this.#containers.delete(formatContainerId(parsed));
  }

  setHideEmptyRootContainers(hide: boolean): void {
    this.#hideEmptyRoots = hide;
  }

  commit(options: CommitOptions = {}): void {
    this.#commit(options, false);
  }

  #commit(options: CommitOptions, preserveOptionsOnEmpty: boolean): void {
    if (this.#committing) return;
    const pending = this.#pending;
    if (pending === undefined || pending.operations.length === 0) {
      if (!preserveOptionsOnEmpty) this.#nextCommitOptions = {};
      return;
    }
    const mergedOptions: MutableCommitOptions = {
      ...this.#nextCommitOptions,
      ...options,
    };
    this.#nextCommitOptions = {};

    const isFirstCommit = !this.#seenCommittedPeers.has(pending.id.peer);
    this.#committing = true;
    try {
      if (isFirstCommit) {
        this.#seenCommittedPeers.add(pending.id.peer);
        for (const listener of this.#firstCommitSubscribers) {
          listener({ peer: peerIdToString(pending.id.peer) });
        }
      }

      const latestTimestamp = this.#greatestTimestampAt(pending.dependencies);
      const initialTimestamp = maxBigInt(
        latestTimestamp,
        numberToI64(
          mergedOptions.timestamp ?? (this.#recordTimestamp ? Date.now() / 1000 : 0),
        ),
      );
      const provisional: DecodedChange = {
        id: pending.id,
        dependencies: pending.dependencies,
        lamport: pending.lamport,
        timestamp: initialTimestamp,
        message: mergedOptions.message,
        operations: pending.operations,
      };
      const modifier = new ChangeModifier(mergedOptions);
      this.#preCommitRecord = { change: provisional, keys: pending.keys };
      try {
        for (const listener of this.#preCommitSubscribers) {
          listener({
            changeMeta: publicChange(provisional),
            origin: mergedOptions.origin ?? "",
            modifier,
          });
        }
      } finally {
        this.#preCommitRecord = undefined;
      }

      const atomLength = pending.operationLength;
      const timestamp = maxBigInt(
        latestTimestamp,
        numberToI64(
          mergedOptions.timestamp ?? (this.#recordTimestamp ? Date.now() / 1000 : 0),
        ),
      );
      const change: DecodedChange = {
        id: pending.id,
        dependencies: pending.dependencies,
        lamport: pending.lamport,
        timestamp,
        message: mergedOptions.message,
        operations: pending.operations,
      };
      this.#pending = undefined;
      const committedRecord: HistoryRecord = { change, keys: pending.keys };
      const previous = this.#mergeablePreviousRecord(change, this.#changeMergeInterval);
      let updateRecord = committedRecord;
      if (previous === undefined) {
        this.#setHistoryRecord(changeKey(change.id), committedRecord, committedRecord);
      } else {
        const previousLength = changeLength(previous.change);
        updateRecord = appendHistoryRecord(previous, committedRecord, previousLength);
        this.#appendMergedHistoryRecord(previous, updateRecord, previousLength);
      }
      this.#nextCounter = change.id.counter + atomLength;
      if (this.#detached) {
        const checkoutVersion = this.#checkoutVersion ?? new VersionVector();
        checkoutVersion.set(change.id.peer, change.id.counter + atomLength);
        this.#checkoutVersion = checkoutVersion;
      }

      if (this.#localUpdateSubscribers.size > 0) {
        const bytes = this.#encodeUpdates([updateRecord]);
        for (const listener of this.#localUpdateSubscribers) listener(bytes.slice());
      }
      if (this.#hasEventSubscribers()) {
        this.#emit(
          "local",
          mergedOptions.origin,
          pending.from,
          this.#frontiersCodec(),
          pending.changedContainers,
          pending.beforeValues,
          this.#recordedEventDiffs(pending),
        );
      }
    } catch (error) {
      if (isFirstCommit) this.#seenCommittedPeers.delete(pending.id.peer);
      throw error;
    } finally {
      this.#committing = false;
    }
  }

  getPendingTxnLength(): number {
    return this.#pending?.operationLength ?? 0;
  }

  setNextCommitMessage(message: string): void {
    this.#nextCommitOptions = { ...this.#nextCommitOptions, message };
  }

  setNextCommitOrigin(origin: string): void {
    this.#nextCommitOptions = { ...this.#nextCommitOptions, origin };
  }

  setNextCommitTimestamp(timestamp: number): void {
    this.#nextCommitOptions = { ...this.#nextCommitOptions, timestamp };
  }

  setNextCommitOptions(options: CommitOptions): void {
    this.#nextCommitOptions = { ...options };
  }

  clearNextCommitOptions(): void {
    this.#nextCommitOptions = {};
  }

  export(mode: ExportMode): Uint8Array {
    this.#commit({}, true);
    if (mode.mode === "update") {
      const from = mode.from ?? new VersionVector();
      const effectiveFrom = from.clone();
      effectiveFrom.merge(this.#shallowStartVersion);
      const records = this.#recordsInVersionRange(effectiveFrom, this.#historyVersion());
      return this.#encodeUpdates(records);
    }
    if (mode.mode === "updates-in-range") {
      const records = this.#recordsInSpans(mode.spans);
      return this.#encodeUpdates(records);
    }
    if (mode.mode === "shallow-snapshot") {
      return this.#encodeShallowSnapshot(mode.frontiers);
    }
    return this.#encodeSnapshot();
  }

  exportJsonUpdates(
    start?: VersionVectorInput,
    end?: VersionVectorInput,
    withPeerCompression = true,
  ): JsonSchema {
    this.#commit({}, true);
    const startVersion = new VersionVector(start);
    const endVersion =
      end === undefined || end === null ? this.#historyVersion() : new VersionVector(end);
    const records = this.#recordsInVersionRange(startVersion, endVersion);
    return historyRecordsToJsonSchema(records, startVersion, withPeerCompression);
  }

  exportJsonInIdSpan(span: IdSpan): JsonChange[] {
    this.#commit({}, true);
    const records = this.#recordsInSpans([
      { id: { peer: span.peer, counter: span.counter }, len: span.length },
    ]);
    return historyRecordsToJsonSchema(records, new VersionVector(), false, false).changes;
  }

  importJsonUpdates(input: JsonSchema | string): ImportStatus {
    const parsed = typeof input === "string" ? (JSON.parse(input) as JsonSchema) : input;
    if (parsed.schema_version !== 1) {
      throw new TypeError(
        `unsupported JSON schema version ${String(parsed.schema_version)}`,
      );
    }
    const records = jsonSchemaToHistoryRecords(parsed);
    return this.import(this.#encodeUpdates(records));
  }

  import(bytes: Uint8Array): ImportStatus {
    this.#commit({}, true);
    const before = this.#frontiersCodec();
    const beforeVersion = this.#historyVersion();
    const parsed = decodeDocument(bytes);
    if (this.#deferredSnapshotHistory !== undefined) {
      this.#materializeDeferredHistory();
    }
    const imported: HistoryRecord[] = [];
    let integration: IntegrationResult = { added: [], pending: [] };
    let beforeValues = new Map<string, unknown>();
    let preparedDiffs = new Map<string, Diff>();
    let deferredChanged: Set<string> | undefined;
    let deferredStatus: ImportStatus | undefined;
    if (parsed.mode === EncodeMode.FastUpdates) {
      for (const blockBytes of decodeFastUpdatesBody(parsed.body)) {
        imported.push(...this.#readChangeBlock(blockBytes));
      }
      this.#assertImportsNotOutdated(imported);
      integration = this.#integrateHistory(imported);
      const { added } = integration;
      if (added.length > 0 && !this.#detached) {
        const recording = this.#hasEventSubscribers()
          ? { beforeValues: new Map(), eventStates: new Map() }
          : undefined;
        this.#applyRecords(added, recording);
        this.#canonicalizeImportedMovableMoves(added, recording);
        if (recording !== undefined) {
          beforeValues = recording.beforeValues;
          preparedDiffs = this.#recordedEventDiffs(recording);
        }
      }
    } else {
      const snapshot = decodeFastSnapshotBody(parsed.body);
      const oplogEntries = decodeSstable(snapshot.oplog.slice());
      let encodedStartVersion: VersionVector | undefined;
      let encodedStartFrontiers: CodecId[] | undefined;
      let encodedEndVersion: VersionVector | undefined;
      let encodedEndFrontiers: CodecId[] | undefined;
      for (const entry of oplogEntries) {
        if (bytesEqual(entry.key, START_VERSION_KEY)) {
          encodedStartVersion = versionVectorFromCodec(
            decodePostcardVersionVector(entry.value),
          );
        } else if (bytesEqual(entry.key, START_FRONTIERS_KEY)) {
          encodedStartFrontiers = decodePostcardFrontiers(entry.value);
        } else if (bytesEqual(entry.key, VERSION_KEY)) {
          encodedEndVersion = versionVectorFromCodec(
            decodePostcardVersionVector(entry.value),
          );
        } else if (bytesEqual(entry.key, FRONTIERS_KEY)) {
          encodedEndFrontiers = decodePostcardFrontiers(entry.value);
        }
      }

      const initializeFromSnapshot =
        this.#history.size === 0 &&
        this.#pendingHistory.size === 0 &&
        this.#shallowRootStore === undefined;
      const isShallowSnapshot = snapshot.shallowRootState.length > 0;
      let rootStore: StateSnapshotStore | undefined;
      let stagedShallowStartVersion: VersionVector | undefined;
      let stagedShallowRootVersion: VersionVector | undefined;
      let stagedShallowRootFrontiers: CodecId[] | undefined;
      let canonicalStartMetadata =
        !isShallowSnapshot &&
        encodedStartVersion === undefined &&
        encodedStartFrontiers === undefined;
      if (initializeFromSnapshot && isShallowSnapshot) {
        rootStore = decodeStateSnapshotStore(snapshot.shallowRootState.slice());
        if (rootStore.kind !== "sstable") {
          throw new Error("shallow snapshot root state must be an SSTable");
        }
        if (
          rootStore.frontiers !== undefined &&
          encodedStartFrontiers !== undefined &&
          !frontierSetsEqual(
            rootStore.frontiers.map(formatOpId),
            encodedStartFrontiers.map(formatOpId),
          )
        ) {
          throw new Error("shallow snapshot start frontiers do not match root state");
        }
        const startFrontiers = encodedStartFrontiers ?? rootStore.frontiers ?? [];
        const startVersion = encodedStartVersion ?? new VersionVector();
        const rootVersion = startVersion.clone();
        for (const frontier of startFrontiers) {
          rootVersion.set(frontier.peer, frontier.counter + 1);
        }
        stagedShallowStartVersion = startVersion;
        stagedShallowRootVersion = rootVersion;
        stagedShallowRootFrontiers = startFrontiers.map((id) => ({ ...id }));
        canonicalStartMetadata =
          encodedStartVersion !== undefined && encodedStartFrontiers !== undefined;
      }

      const stateStore = decodeStateSnapshotStore(snapshot.state);
      const hydratedStore =
        rootStore === undefined
          ? stateStore
          : mergeStateSnapshotStores(rootStore, stateStore);
      if (
        stagedShallowRootVersion !== undefined &&
        encodedEndVersion !== undefined &&
        !versionIncludes(encodedEndVersion, stagedShallowRootVersion)
      ) {
        throw new Error("shallow snapshot root version exceeds its end version");
      }
      const installStagedShallowRoot = (): void => {
        if (
          rootStore === undefined ||
          stagedShallowStartVersion === undefined ||
          stagedShallowRootVersion === undefined ||
          stagedShallowRootFrontiers === undefined
        ) {
          return;
        }
        this.#shallowStartVersion = stagedShallowStartVersion;
        this.#shallowRootVersion = stagedShallowRootVersion;
        this.#shallowRootFrontiers = stagedShallowRootFrontiers;
        this.#shallowRootStore = rootStore;
      };
      const canDeferHistory =
        initializeFromSnapshot &&
        !this.#detached &&
        canonicalStartMetadata &&
        encodedEndVersion !== undefined &&
        encodedEndFrontiers !== undefined &&
        stateStore.kind === "sstable" &&
        hydratedStore.kind === "sstable";

      if (canDeferHistory) {
        const endVersion = encodedEndVersion!;
        const endFrontiers = encodedEndFrontiers!;
        const validatedBlocks = this.#validateDeferredFrontierBlocks(
          oplogEntries,
          endVersion,
          endFrontiers,
        );
        installStagedShallowRoot();
        deferredChanged = new Set(
          hydratedStore.containers.map(({ id }) => formatContainerId(id)),
        );
        if (this.#hasEventSubscribers()) {
          beforeValues = this.#captureContainerEventValues(deferredChanged);
        }
        this.#hydrateState(hydratedStore);
        this.#deferredSnapshotHistory = {
          entries: oplogEntries,
          validatedBlocks,
          endVersion,
          frontiers: endFrontiers.map((id) => ({ ...id })),
          operationCount: versionDistance(
            stagedShallowStartVersion ?? this.#shallowStartVersion,
            endVersion,
          ),
        };
        for (const { peer } of endVersion._codecEntriesUnsorted()) {
          this.#seenCommittedPeers.add(peer);
        }
        deferredStatus = importStatusBetweenVersions(
          beforeVersion,
          stagedShallowStartVersion ?? this.#shallowStartVersion,
          endVersion,
        );
      } else {
        for (const entry of oplogEntries) {
          if (entry.key.length !== 12) continue;
          const expected = decodeChangeBlockKey(entry.key);
          const records = this.#readChangeBlock(entry.value);
          if (records[0] !== undefined && !idsEqual(records[0].change.id, expected)) {
            throw new Error("snapshot change key does not match its block");
          }
          imported.push(...records);
        }
        installStagedShallowRoot();
        if (!initializeFromSnapshot) this.#assertImportsNotOutdated(imported);
        integration = this.#integrateHistory(imported);
        if (!this.#detached && initializeFromSnapshot) {
          if (this.#hasEventSubscribers()) {
            beforeValues = this.#captureEventValues(integration.added);
          }
          if (rootStore !== undefined && stateStore.kind === "empty") {
            this.#rebuildFromHistory();
          } else {
            this.#hydrateState(hydratedStore);
          }
        } else if (!this.#detached && integration.added.length > 0) {
          const recording = this.#hasEventSubscribers()
            ? { beforeValues: new Map(), eventStates: new Map() }
            : undefined;
          this.#applyRecords(integration.added, recording);
          this.#canonicalizeImportedMovableMoves(integration.added, recording);
          if (recording !== undefined) {
            beforeValues = recording.beforeValues;
            preparedDiffs = this.#recordedEventDiffs(recording);
          }
        }
      }
    }
    this.#nextCounter = this.oplogVersion().get(this.#peer) ?? 0;
    const changed = this.#detached
      ? new Set<string>()
      : (deferredChanged ??
        new Set(
          integration.added.flatMap(({ change }) =>
            change.operations.map((op) => formatContainerId(op.container)),
          ),
        ));
    this.#emit(
      "import",
      undefined,
      before,
      this.#frontiersCodec(),
      changed,
      beforeValues,
      preparedDiffs,
    );
    return deferredStatus ?? importStatus(integration.added, integration.pending);
  }

  importBatch(blobs: readonly Uint8Array[]): ImportStatus {
    if (blobs.length === 0) return { success: new Map(), pending: null };
    if (blobs.length === 1) return this.import(blobs[0]!);
    this.#commit({}, true);
    this.#materializeDeferredHistory();
    const before = this.#frontiersCodec();
    const ordered = blobs
      .map((blob) => this.#decodeImportData(decodeDocument(blob)))
      .sort(
        (left, right) =>
          Number(left.mode === EncodeMode.FastUpdates) -
            Number(right.mode === EncodeMode.FastUpdates) ||
          right.records.length - left.records.length,
      );
    const initializeFromSnapshot =
      this.#history.size === 0 &&
      this.#pendingHistory.size === 0 &&
      this.#shallowRootStore === undefined;
    const snapshotSeed = initializeFromSnapshot
      ? ordered.find(({ snapshot }) => snapshot !== undefined)
      : undefined;
    let rootStore: StateSnapshotStore | undefined;
    if (
      snapshotSeed?.snapshot !== undefined &&
      snapshotSeed.snapshot.shallowRootState.length > 0
    ) {
      rootStore = decodeStateSnapshotStore(snapshotSeed.snapshot.shallowRootState);
      if (rootStore.kind !== "sstable") {
        throw new Error("shallow snapshot root state must be an SSTable");
      }
      const startFrontiers = snapshotSeed.startFrontiers ?? rootStore.frontiers ?? [];
      const startVersion = snapshotSeed.startVersion ?? new VersionVector();
      const rootVersion = startVersion.clone();
      for (const frontier of startFrontiers) {
        rootVersion.set(frontier.peer, frontier.counter + 1);
      }
      this.#shallowStartVersion = startVersion;
      this.#shallowRootVersion = rootVersion;
      this.#shallowRootFrontiers = startFrontiers.map((id) => ({ ...id }));
      this.#shallowRootStore = rootStore;
    }

    for (const decoded of ordered) {
      if (decoded !== snapshotSeed) this.#assertImportsNotOutdated(decoded.records);
    }
    const integration = this.#integrateHistory(ordered.flatMap(({ records }) => records));
    let beforeValues = new Map<string, unknown>();
    let preparedDiffs = new Map<string, Diff>();
    if (integration.added.length > 0 && !this.#detached) {
      if (snapshotSeed?.snapshot !== undefined) {
        if (this.#hasEventSubscribers()) {
          beforeValues = this.#captureEventValues(integration.added);
        }
        const stateStore = decodeStateSnapshotStore(snapshotSeed.snapshot.state);
        if (rootStore !== undefined && stateStore.kind === "empty") {
          this.#rebuildFromHistory();
        } else {
          this.#hydrateState(
            rootStore === undefined
              ? stateStore
              : mergeStateSnapshotStores(rootStore, stateStore),
          );
          const snapshotVersion =
            snapshotSeed.endVersion?.clone() ??
            historyVersionForRecords(snapshotSeed.records, snapshotSeed.startVersion);
          const forwardRecords = this.#recordsInVersionRange(
            snapshotVersion,
            this.#historyVersion(),
          );
          this.#applyRecords(forwardRecords);
          this.#canonicalizeImportedMovableMoves(forwardRecords);
        }
      } else {
        const recording = this.#hasEventSubscribers()
          ? { beforeValues: new Map(), eventStates: new Map() }
          : undefined;
        this.#applyRecords(integration.added, recording);
        this.#canonicalizeImportedMovableMoves(integration.added, recording);
        if (recording !== undefined) {
          beforeValues = recording.beforeValues;
          preparedDiffs = this.#recordedEventDiffs(recording);
        }
      }
    }
    this.#nextCounter = this.oplogVersion().get(this.#peer) ?? 0;
    const changed = this.#detached
      ? new Set<string>()
      : changedContainerIds(integration.added);
    this.#emit(
      "import",
      undefined,
      before,
      this.#frontiersCodec(),
      changed,
      beforeValues,
      preparedDiffs,
    );
    return importStatus(integration.added, integration.pending);
  }

  importUpdateBatch(blobs: readonly Uint8Array[]): ImportStatus {
    return this.importBatch(blobs);
  }

  toJSON(): Record<string, unknown> {
    const output: Record<string, unknown> = {};
    for (const [name, container] of this.#roots) {
      const value = container.toJSON();
      if (this.#hideEmptyRoots && isEmptyJson(value)) continue;
      output[name] = value;
    }
    return output;
  }

  toJsonWithReplacer(
    replacer: (
      key: string | number,
      value: Value | Container,
    ) => Value | Container | undefined,
  ): unknown {
    const processed = new Set<ContainerID>();
    const run = (value: unknown): unknown => {
      if (Array.isArray(value)) {
        return value.flatMap((item, index) => {
          const next = visit(index, item);
          return next === undefined ? [] : [next];
        });
      }
      if (typeof value === "object" && value !== null && !(value instanceof Uint8Array)) {
        return Object.fromEntries(
          Object.entries(value).flatMap(([key, item]) => {
            const next = visit(key, item);
            return next === undefined ? [] : [[key, next]];
          }),
        );
      }
      return value;
    };
    const visit = (key: string | number, value: unknown): unknown => {
      if (
        typeof value === "string" &&
        isContainerId(value) &&
        !processed.has(value as ContainerID)
      ) {
        const id = value as ContainerID;
        processed.add(id);
        const container = this.getContainerById(id);
        if (container === undefined)
          throw new RangeError(`container ${id} does not exist`);
        const replaced = replacer(key, container);
        if (replaced === container) return run(container.getShallowValue());
        if (isContainer(replaced)) {
          throw new TypeError("replacer cannot substitute a different container");
        }
        return replaced === undefined ? undefined : run(replaced);
      }
      if (typeof value === "object" && value !== null && !(value instanceof Uint8Array)) {
        return run(value);
      }
      const replaced = replacer(key, value as Value);
      if (isContainer(replaced)) {
        throw new TypeError("replacer cannot introduce a container");
      }
      return replaced;
    };
    return run(this.getShallowValue());
  }

  diff(from: Frontiers, to: Frontiers, forJson: false): [ContainerID, Diff][];
  diff(from: Frontiers, to: Frontiers, forJson?: true): [ContainerID, JsonDiff][];
  diff(from: Frontiers, to: Frontiers, forJson = true): [ContainerID, Diff | JsonDiff][] {
    this.#commit({}, true);
    const fromVersion = this.#versionForExistingFrontiers(from);
    const toVersion = this.#versionForExistingFrontiers(to);
    this.#assertVersionNotBeforeShallowRoot(fromVersion);
    this.#assertVersionNotBeforeShallowRoot(toVersion);
    if (fromVersion.compare(toVersion) === 0) return [];
    const restoreVersion = this.version();
    const currentToFromForward = this.#recordsInVersionRange(restoreVersion, fromVersion);
    const currentToFromRetreat = this.#recordsInVersionRange(fromVersion, restoreVersion);
    const forwardRecords = this.#recordsInVersionRange(fromVersion, toVersion);
    const retreatRecords = this.#recordsInVersionRange(toVersion, fromVersion);
    const currentToFromMoveMode = movableMoveTransitionMode(
      currentToFromRetreat,
      currentToFromForward,
      this.#movableMovePeers,
    );
    const fromToToMoveMode = movableMoveTransitionMode(
      retreatRecords,
      forwardRecords,
      this.#movableMovePeers,
    );
    const useIncrementalTransition =
      this.#canTransitionRecords(
        [...currentToFromRetreat, ...currentToFromForward],
        currentToFromMoveMode,
      ) &&
      this.#canTransitionRecords(
        [...retreatRecords, ...forwardRecords],
        fromToToMoveMode,
      );
    let materializedVersion = restoreVersion;
    let transitionFailed = false;
    try {
      if (materializedVersion.compare(fromVersion) !== 0) {
        if (useIncrementalTransition) {
          this.#applyVersionTransition(
            currentToFromRetreat,
            currentToFromForward,
            fromVersion,
            undefined,
            currentToFromMoveMode,
          );
        } else {
          this.#rebuildFromHistory(fromVersion);
        }
        materializedVersion = fromVersion;
      }
      const changed = changedContainerIds([...forwardRecords, ...retreatRecords]);
      let before = new Map<string, unknown>();
      let calculated = new Map<string, Diff>();
      let mapKeysAtFrom = new Map<string, Set<string>>();
      let mapKeysAtTo = new Map<string, Set<string>>();
      if (useIncrementalTransition || retreatRecords.length === 0) {
        const recording: EventRecording = {
          beforeValues: new Map(),
          eventStates: new Map(),
        };
        if (useIncrementalTransition) {
          this.#applyVersionTransition(
            retreatRecords,
            forwardRecords,
            toVersion,
            recording,
            fromToToMoveMode,
          );
        } else {
          this.#applyRecords(forwardRecords, recording);
        }
        before = recording.beforeValues;
        calculated = this.#recordedEventDiffs(recording, true);
      } else {
        before = this.#captureContainerEventValues(changed);
        mapKeysAtFrom = this.#captureMapKeys(changed);
        this.#rebuildFromHistory(toVersion);
        mapKeysAtTo = this.#captureMapKeys(changed);
      }
      materializedVersion = toVersion;
      return [...changed]
        .flatMap((id) => {
          const container = this.#containers.get(id);
          return container === undefined ? [] : [container];
        })
        .sort((left, right) => containerDepth(left) - containerDepth(right))
        .flatMap((container) => {
          const diff =
            calculated.get(container.id) ??
            containerDiff(container, before.get(container.id), {
              from: mapKeysAtFrom.get(container.id),
              to: mapKeysAtTo.get(container.id),
            });
          if (isEmptyContainerDiff(diff)) return [];
          return [
            [container.id, forJson ? diffForJson(diff) : diff] as [
              ContainerID,
              Diff | JsonDiff,
            ],
          ];
        });
    } catch (error) {
      transitionFailed = useIncrementalTransition;
      throw error;
    } finally {
      if (transitionFailed) {
        this.#rebuildFromHistory(restoreVersion);
      } else if (materializedVersion.compare(restoreVersion) !== 0) {
        if (useIncrementalTransition) {
          if (materializedVersion.compare(toVersion) === 0) {
            this.#applyVersionTransition(
              forwardRecords,
              retreatRecords,
              fromVersion,
              undefined,
              fromToToMoveMode,
            );
            materializedVersion = fromVersion;
          }
          if (materializedVersion.compare(restoreVersion) !== 0) {
            this.#applyVersionTransition(
              currentToFromForward,
              currentToFromRetreat,
              restoreVersion,
              undefined,
              currentToFromMoveMode,
            );
          }
        } else {
          this.#rebuildFromHistory(restoreVersion);
        }
      }
    }
  }

  applyDiff(diffBatch: readonly (readonly [ContainerID, Diff | JsonDiff])[]): void {
    if (!Array.isArray(diffBatch)) throw new TypeError("diff batch must be an array");
    if (this.#detached && !this.#detachedEditing) {
      throw new Error("cannot edit a detached document; call attach() first");
    }

    const containerRemap = new Map<ContainerID, Container>();
    const treeRemap = new Map<TreeID, TreeID>();
    for (const entry of diffBatch) {
      if (!Array.isArray(entry) || entry.length !== 2 || typeof entry[0] !== "string") {
        throw new TypeError("each diff entry must be a [ContainerID, Diff] tuple");
      }
      const sourceId = entry[0] as ContainerID;
      const diff = entry[1];
      const container = this.#resolveDiffContainer(sourceId, containerRemap);
      if (container === undefined) continue;
      this.#applyContainerDiff(container, diff, containerRemap, treeRemap);
    }
  }

  revertTo(frontiers: Frontiers): void {
    if (this.#detached && !this.#detachedEditing) {
      throw new Error("cannot edit a detached document; call attach() first");
    }
    this.#commit({}, true);
    const diff = this.diff(this.frontiers(), frontiers, false);
    this.applyDiff(diff);
  }

  #resolveDiffContainer(
    sourceId: ContainerID,
    remap: ReadonlyMap<ContainerID, Container>,
  ): Container | undefined {
    const mapped = remap.get(sourceId);
    if (mapped !== undefined) return mapped;

    const parsed = parseContainerId(sourceId);
    if (parsed.kind === "root" && !isMergeableContainerId(parsed)) {
      return this.#getOrCreateContainer(parsed) as Container;
    }
    const existing = this.#containers.get(formatContainerId(parsed)) as
      | Container
      | undefined;
    return existing !== undefined && !this._isContainerDeleted(existing)
      ? existing
      : undefined;
  }

  #applyContainerDiff(
    container: Container,
    diff: Diff | JsonDiff,
    containerRemap: Map<ContainerID, Container>,
    treeRemap: Map<TreeID, TreeID>,
  ): void {
    if (diff.type === "map") {
      if (!(container instanceof LoroMap)) throw diffKindMismatch(container, diff.type);
      for (const [key, value] of Object.entries(diff.updated)) {
        if (value === undefined) {
          container.delete(key);
          continue;
        }
        const sourceChildId = diffContainerId(value);
        if (sourceChildId === undefined) {
          container.set(key, value);
        } else {
          this.#applyMapChildDiff(container, key, sourceChildId, containerRemap);
        }
      }
      return;
    }

    if (diff.type === "text") {
      if (!(container instanceof LoroText)) throw diffKindMismatch(container, diff.type);
      container.applyDelta(diff.diff);
      return;
    }

    if (diff.type === "counter") {
      if (!(container instanceof LoroCounter))
        throw diffKindMismatch(container, diff.type);
      container.increment(diff.increment);
      return;
    }

    if (diff.type === "tree") {
      if (!(container instanceof LoroTree)) throw diffKindMismatch(container, diff.type);
      this.#applyTreeDiff(container, diff.diff, containerRemap, treeRemap);
      return;
    }

    if (!(container instanceof LoroList)) throw diffKindMismatch(container, diff.type);
    let position = 0;
    for (const operation of diff.diff) {
      if ("retain" in operation) {
        position += operation.retain;
      } else if ("delete" in operation) {
        container.delete(position, operation.delete);
      } else {
        for (const value of operation.insert) {
          const sourceChildId = diffContainerId(value);
          if (sourceChildId === undefined) {
            container.insert(position, value);
          } else {
            const parsed = parseContainerId(sourceChildId);
            if (parsed.kind === "root") {
              throw new TypeError("a root container cannot be inserted as a child");
            }
            const child = createContainer(
              codecTypeToPublic(parsed.containerType),
            ) as Container;
            const attached = container.insertContainer(position, child);
            containerRemap.set(sourceChildId, attached);
          }
          position += 1;
        }
      }
    }
  }

  #applyMapChildDiff(
    parent: LoroMap,
    key: string,
    sourceId: ContainerID,
    remap: Map<ContainerID, Container>,
  ): void {
    const mapped = remap.get(sourceId);
    const current = parent.get(key);
    if (mapped !== undefined && current === mapped) return;

    const parsed = parseContainerId(sourceId);
    const type = codecTypeToPublic(parsed.containerType);
    if (isMergeableContainerId(parsed)) {
      const child = ensureMergeableChild(parent, key, type);
      remap.set(sourceId, child);
      return;
    }
    if (parsed.kind === "root") {
      throw new TypeError("a root container cannot be assigned as a child");
    }
    if (isContainer(current) && current.id === sourceId) {
      remap.set(sourceId, current);
      return;
    }
    const child = createContainer(type) as Container;
    remap.set(sourceId, parent.setContainer(key, child));
  }

  #applyTreeDiff(
    tree: LoroTree,
    diff: readonly TreeDiffItem[],
    containerRemap: Map<ContainerID, Container>,
    treeRemap: Map<TreeID, TreeID>,
  ): void {
    const resolveNode = (source: TreeID): TreeID | undefined =>
      treeRemap.get(source) ?? (tree.has(source) ? source : undefined);

    for (const item of diff) {
      if (item.action !== "delete") continue;
      const target = resolveNode(item.target);
      if (target !== undefined) tree.delete(target);
    }

    const pending = diff.filter(
      (item): item is Extract<TreeDiffItem, { action: "create" | "move" }> =>
        item.action !== "delete",
    );
    while (pending.length > 0) {
      let progressed = false;
      for (let index = 0; index < pending.length; ) {
        const item = pending[index]!;
        const parent = item.parent === undefined ? undefined : resolveNode(item.parent);
        if (item.parent !== undefined && parent === undefined) {
          index += 1;
          continue;
        }

        if (item.action === "create") {
          const node = tree.createNode(parent, item.index);
          treeRemap.set(item.target, node.id);
          const sourceNode = parseTreeId(item.target);
          const sourceMetaId = formatContainerId({
            kind: "normal",
            ...sourceNode,
            containerType: CodecContainerType.Map,
          });
          containerRemap.set(sourceMetaId, node.data);
        } else {
          const target = resolveNode(item.target);
          if (target !== undefined) tree.move(target, parent, item.index);
        }
        pending.splice(index, 1);
        progressed = true;
      }
      if (!progressed) {
        throw new RangeError("tree diff refers to a parent that does not exist");
      }
    }
  }

  getDeepValueWithId(): Record<string, unknown> {
    return Object.fromEntries(
      [...this.#roots].map(([name, container]) => [
        name,
        containerDeepValueWithId(container as Container),
      ]),
    );
  }

  getShallowValue(): Record<string, ContainerID> {
    return Object.fromEntries(
      [...this.#roots].map(([name, container]) => [name, container.id]),
    );
  }

  getDeepValueWithID(): Record<string, unknown> {
    return this.getDeepValueWithId();
  }

  version(): VersionVector {
    const version = this.#checkoutVersion?.clone() ?? this.#historyVersion();
    if (this.#pending !== undefined) {
      const end = this.#pending.id.counter + this.#pending.operationLength;
      if (end > (version.get(this.#peer) ?? 0)) version.set(this.#peer, end);
    }
    return version;
  }

  oplogVersion(): VersionVector {
    return this.#historyVersion();
  }

  #historyVersion(): VersionVector {
    if (this.#deferredSnapshotHistory !== undefined) {
      return this.#deferredSnapshotHistory.endVersion.clone();
    }
    const version = this.#shallowStartVersion.clone();
    for (const [peer, end] of this.#historyEndByPeer) {
      if (end > (version.get(peer) ?? 0)) version.set(peer, end);
    }
    return version;
  }

  frontiers(): Frontiers {
    return this.#frontiersCodec().map(formatOpId);
  }

  oplogFrontiers(): Frontiers {
    if (this.#deferredSnapshotHistory !== undefined) {
      return [...this.#deferredSnapshotHistory.frontiers]
        .sort(compareIds)
        .map(formatOpId);
    }
    return [...this.#historyFrontiers.values()].sort(compareIds).map(formatOpId);
  }

  frontiersToVV(frontiers: Frontiers): VersionVector {
    const version = new VersionVector();
    for (const [peer, counter] of this.#causalVersionAt(frontiers.map(parseOpId))) {
      version.set(peer, counter);
    }
    return version;
  }

  #versionForExistingFrontiers(frontiers: Frontiers): VersionVector {
    for (const frontier of frontiers) {
      if (this.#recordContaining(parseOpId(frontier)) === undefined) {
        throw new RangeError(
          `frontiers include unknown id ${frontier.counter}@${frontier.peer}`,
        );
      }
    }
    return this.frontiersToVV(frontiers);
  }

  vvToFrontiers(version: VersionVector): Frontiers {
    return this.#frontiersForVersion(version).map(formatOpId);
  }

  cmpWithFrontiers(frontiers: Frontiers): -1 | 0 | 1 {
    const current = this.oplogFrontiers();
    if (frontierSetsEqual(current, frontiers)) return 0;
    const version = this.#historyVersion();
    return frontiers.every((frontier) => {
      const id = parseOpId(frontier);
      return id.counter < (version.get(id.peer) ?? 0);
    })
      ? 1
      : -1;
  }

  cmpFrontiers(left: Frontiers, right: Frontiers): -1 | 0 | 1 | undefined {
    return this.#versionForExistingFrontiers(left).compare(
      this.#versionForExistingFrontiers(right),
    );
  }

  changeCount(): number {
    this.#materializeDeferredHistory();
    return this.#history.size;
  }

  debugHistory(): void {
    this.#sortedHistory();
  }

  opCount(): number {
    return this.#deferredSnapshotHistory?.operationCount ?? this.#historyOperationCount;
  }

  getAllChanges(): Map<PeerID, Change[]> {
    const changes = new Map<PeerID, Change[]>();
    for (const { change } of this.#sortedHistory()) {
      const peer = peerIdToString(change.id.peer);
      const peerChanges = changes.get(peer);
      if (peerChanges === undefined) changes.set(peer, [publicChange(change)]);
      else peerChanges.push(publicChange(change));
    }
    return changes;
  }

  getChangeAt(id: OpId): Change {
    const target = parseOpId(id);
    const record = this.#recordContaining(target);
    if (record === undefined) {
      throw new RangeError(`change ${target.counter}@${target.peer} is unknown`);
    }
    return publicChange(record.change);
  }

  getChangeAtLamport(peer: PeerIdInput, lamport: number): Change | undefined {
    this.#materializeDeferredHistory();
    const parsedPeer = parsePeerId(peer);
    const records = this.#historyByPeer.get(parsedPeer) ?? [];
    let low = 0;
    let high = records.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (records[middle]!.change.lamport <= lamport) low = middle + 1;
      else high = middle;
    }
    const record = records[low - 1];
    return record === undefined ? undefined : publicChange(record.change);
  }

  getOpsInChange(id: OpId): JsonOp[] {
    this.#commit({}, true);
    const record = this.#recordContaining(parseOpId(id));
    if (record === undefined) {
      throw new RangeError(`change ${id.counter}@${id.peer} is unknown`);
    }
    return historyRecordToJsonChange(record, undefined, false).ops;
  }

  getUncommittedOpsAsJson(): JsonSchema | undefined {
    const pending = this.#pending;
    if (pending === undefined || pending.operations.length === 0) return undefined;
    const timestamp =
      this.#nextCommitOptions.timestamp ??
      (this.#recordTimestamp ? Date.now() / 1000 : 0);
    const record: HistoryRecord = {
      keys: pending.keys,
      change: {
        id: pending.id,
        dependencies: pending.dependencies,
        lamport: pending.lamport,
        timestamp: numberToI64(timestamp),
        message: this.#nextCommitOptions.message,
        operations: pending.operations,
      },
    };
    return historyRecordsToJsonSchema([record], this.#historyVersion(), false);
  }

  travelChangeAncestors(
    ids: Frontiers,
    callback: (change: Change) => boolean | void,
  ): void {
    this.#commit({}, true);
    const visited = new Set<string>();
    let stopped = false;
    const visit = (id: CodecId): void => {
      if (stopped) return;
      const record = this.#recordContaining(id);
      if (record === undefined) {
        if (id.counter < (this.#shallowStartVersion.get(id.peer) ?? 0)) return;
        throw new RangeError(`change ${id.counter}@${id.peer} is unknown`);
      }
      const key = changeKey(record.change.id);
      if (visited.has(key)) return;
      for (const dependency of record.change.dependencies) visit(dependency);
      if (stopped || visited.has(key)) return;
      visited.add(key);
      if (callback(publicChange(record.change)) === false) stopped = true;
    };
    for (const id of ids.map(parseOpId)) visit(id);
  }

  findIdSpansBetween(from: Frontiers, to: Frontiers): VersionVectorDiff {
    const fromVersion = this.#versionForExistingFrontiers(from);
    const toVersion = this.#versionForExistingFrontiers(to);
    this.#assertVersionNotBeforeShallowRoot(fromVersion);
    this.#assertVersionNotBeforeShallowRoot(toVersion);
    const retreat: VersionVectorDiff["retreat"] = [];
    const forward: VersionVectorDiff["forward"] = [];
    const peers = new Set([
      ...fromVersion._codecEntriesUnsorted().map(({ peer }) => peer),
      ...toVersion._codecEntriesUnsorted().map(({ peer }) => peer),
    ]);
    for (const peer of [...peers].sort((left, right) =>
      left < right ? -1 : left > right ? 1 : 0,
    )) {
      const fromCounter = fromVersion.get(peer) ?? 0;
      const toCounter = toVersion.get(peer) ?? 0;
      if (fromCounter < toCounter) {
        forward.push({
          peer: peerIdToString(peer),
          counter: fromCounter,
          length: toCounter - fromCounter,
        });
      } else if (fromCounter > toCounter) {
        retreat.push({
          peer: peerIdToString(peer),
          counter: toCounter,
          length: fromCounter - toCounter,
        });
      }
    }
    return { retreat, forward };
  }

  getChangedContainersIn(id: OpId, len: number): ContainerID[] {
    this.#commit({}, true);
    this.#materializeDeferredHistory();
    if (!Number.isSafeInteger(len) || len < 0) {
      throw new RangeError(`change range length is out of range: ${len}`);
    }
    const start = parseOpId(id);
    const end = start.counter + len;
    const containers = new Set<ContainerID>();
    const records = this.#historyByPeer.get(start.peer) ?? [];
    let recordIndex = Math.max(0, lowerBoundHistory(records, start.counter + 1) - 1);
    for (; recordIndex < records.length; recordIndex += 1) {
      const { change } = records[recordIndex]!;
      if (change.id.counter >= end) break;
      let operationIndex = lowerBoundOperation(change.operations, start.counter + 1) - 1;
      operationIndex = Math.max(0, operationIndex);
      for (; operationIndex < change.operations.length; operationIndex += 1) {
        const operation = change.operations[operationIndex]!;
        if (operation.counter >= end) break;
        if (
          operation.counter < end &&
          operation.counter + operation.length > start.counter
        ) {
          containers.add(formatContainerId(operation.container));
        }
      }
    }
    return [...containers];
  }

  #recordContaining(id: CodecId): HistoryRecord | undefined {
    this.#materializeDeferredHistory();
    const records = this.#historyByPeer.get(id.peer);
    if (records === undefined) return undefined;
    const index = lowerBoundHistory(records, id.counter + 1) - 1;
    const record = records[index];
    return record !== undefined &&
      id.counter < record.change.id.counter + changeLength(record.change)
      ? record
      : undefined;
  }

  #greatestTimestampAt(frontiers: readonly CodecId[]): bigint {
    const version = this.#causalVersionAt(frontiers);
    let greatest = 0n;
    for (const [peer, counter] of version) {
      if (counter === 0) continue;
      const record = this.#recordContaining({ peer, counter: counter - 1 });
      if (record !== undefined) greatest = maxBigInt(greatest, record.change.timestamp);
    }
    return greatest;
  }

  #mergeablePreviousRecord(
    change: DecodedChange,
    interval: bigint,
  ): HistoryRecord | undefined {
    if (change.id.counter === 0) return undefined;
    const previous = this.#recordContaining({
      peer: change.id.peer,
      counter: change.id.counter - 1,
    });
    return previous !== undefined && canMergeChanges(previous.change, change, interval)
      ? previous
      : undefined;
  }

  fork(): LoroDoc<T> {
    return this.forkAt(this.frontiers());
  }

  forkAt(frontiers: Frontiers): LoroDoc<T> {
    this.#commit({}, true);
    const version = this.#versionForExistingFrontiers(frontiers);
    this.#assertVersionNotBeforeShallowRoot(version);
    const fork = new LoroDoc<T>();
    fork.#recordTimestamp = this.#recordTimestamp;
    fork.#changeMergeInterval = this.#changeMergeInterval;
    fork.#detachedEditing = this.#detachedEditing;
    fork.#hideEmptyRoots = this.#hideEmptyRoots;
    fork.#textStyles = new Map(this.#textStyles);
    fork.#defaultTextStyle = this.#defaultTextStyle;
    fork.#shallowStartVersion = this.#shallowStartVersion.clone();
    fork.#shallowRootVersion = this.#shallowRootVersion.clone();
    fork.#shallowRootFrontiers = this.#shallowRootFrontiers.map((id) => ({ ...id }));
    fork.#shallowRootStore = this.#shallowRootStore;
    fork.#integrateHistory(this.#recordsAtVersion(version), undefined);
    fork.#rebuildFromHistory();
    return fork;
  }

  attach(): void {
    this.checkoutToLatest();
  }

  checkout(frontiers: Frontiers): void {
    this.#commit({}, true);
    for (const frontier of frontiers) {
      if (this.#recordContaining(parseOpId(frontier)) === undefined) {
        throw new RangeError(`frontier ${frontier.counter}@${frontier.peer} is unknown`);
      }
    }
    const before = this.#frontiersCodec();
    const currentVersion = this.version();
    const targetVersion = this.frontiersToVV(frontiers);
    this.#assertVersionNotBeforeShallowRoot(targetVersion);
    const forwardRecords = this.#recordsInVersionRange(currentVersion, targetVersion);
    const retreatRecords = this.#recordsInVersionRange(targetVersion, currentVersion);
    const changed = changedContainerIds([...forwardRecords, ...retreatRecords]);
    let beforeValues = new Map<string, unknown>();
    let preparedDiffs = new Map<string, Diff>();
    this.#checkoutVersion = targetVersion;
    this.#detached = true;
    const changedRecords = [...forwardRecords, ...retreatRecords];
    const movableMoveMode = movableMoveTransitionMode(
      retreatRecords,
      forwardRecords,
      this.#movableMovePeers,
    );
    if (this.#canTransitionRecords(changedRecords, movableMoveMode)) {
      const recording = this.#hasEventSubscribers()
        ? { beforeValues: new Map(), eventStates: new Map() }
        : undefined;
      this.#applyVersionTransition(
        retreatRecords,
        forwardRecords,
        targetVersion,
        recording,
        movableMoveMode,
      );
      if (recording !== undefined) {
        beforeValues = recording.beforeValues;
        preparedDiffs = this.#recordedEventDiffs(recording);
      }
    } else if (
      retreatRecords.length === 0 &&
      !hasMaterializedSequenceInsertions(forwardRecords, this.#containers)
    ) {
      const recording = this.#hasEventSubscribers()
        ? { beforeValues: new Map(), eventStates: new Map() }
        : undefined;
      this.#applyRecords(forwardRecords, recording);
      if (recording !== undefined) {
        beforeValues = recording.beforeValues;
        preparedDiffs = this.#recordedEventDiffs(recording);
      }
    } else {
      if (this.#hasEventSubscribers()) {
        beforeValues = this.#captureContainerEventValues(changed);
      }
      this.#rebuildFromHistory(targetVersion);
    }
    this.#emit(
      "checkout",
      undefined,
      before,
      this.#frontiersCodec(),
      changed,
      beforeValues,
      preparedDiffs,
    );
    if (this.#detachedEditing) this.#renewPeerId();
  }

  detach(): void {
    this.#commit({}, true);
    this.#checkoutVersion = this.version();
    this.#detached = true;
  }

  isDetached(): boolean {
    return this.#detached;
  }

  checkoutToLatest(): void {
    this.#commit({}, true);
    const before = this.#frontiersCodec();
    const wasDetached = this.#detached;
    const currentVersion = this.version();
    const latestVersion = this.#historyVersion();
    const forwardRecords = this.#recordsInVersionRange(currentVersion, latestVersion);
    const retreatRecords = this.#recordsInVersionRange(latestVersion, currentVersion);
    const changed = changedContainerIds([...forwardRecords, ...retreatRecords]);
    let beforeValues = new Map<string, unknown>();
    let preparedDiffs = new Map<string, Diff>();
    this.#checkoutVersion = undefined;
    this.#detached = false;
    if (wasDetached) {
      const changedRecords = [...forwardRecords, ...retreatRecords];
      const movableMoveMode = movableMoveTransitionMode(
        retreatRecords,
        forwardRecords,
        this.#movableMovePeers,
      );
      if (this.#canTransitionRecords(changedRecords, movableMoveMode)) {
        const recording = this.#hasEventSubscribers()
          ? { beforeValues: new Map(), eventStates: new Map() }
          : undefined;
        this.#applyVersionTransition(
          retreatRecords,
          forwardRecords,
          latestVersion,
          recording,
          movableMoveMode,
        );
        if (recording !== undefined) {
          beforeValues = recording.beforeValues;
          preparedDiffs = this.#recordedEventDiffs(recording);
        }
      } else if (
        retreatRecords.length === 0 &&
        !hasMaterializedSequenceInsertions(forwardRecords, this.#containers)
      ) {
        const recording = this.#hasEventSubscribers()
          ? { beforeValues: new Map(), eventStates: new Map() }
          : undefined;
        this.#applyRecords(forwardRecords, recording);
        if (recording !== undefined) {
          beforeValues = recording.beforeValues;
          preparedDiffs = this.#recordedEventDiffs(recording);
        }
      } else {
        if (this.#hasEventSubscribers()) {
          beforeValues = this.#captureContainerEventValues(changed);
        }
        this.#rebuildFromHistory();
      }
      this.#emit(
        "checkout",
        undefined,
        before,
        this.#frontiersCodec(),
        changed,
        beforeValues,
        preparedDiffs,
      );
      if (this.#detachedEditing) this.#renewPeerId();
    }
  }

  #renewPeerId(): void {
    let peer = generatePeerId();
    while (peer === this.#peer || peer === 0xffff_ffff_ffff_ffffn) {
      peer = generatePeerId();
    }
    this.#peer = peer;
    this.#nextCounter = this.oplogVersion().get(peer) ?? 0;
  }
  isShallow(): boolean {
    return this.#shallowRootStore !== undefined;
  }
  shallowSinceVV(): VersionVector {
    return this.#shallowStartVersion.clone();
  }
  shallowSinceFrontiers(): Frontiers {
    return this.#shallowRootFrontiers.map(formatOpId);
  }

  subscribe(listener: (event: LoroEventBatch) => void): Subscription {
    this.#subscribers.add(listener);
    return () => this.#subscribers.delete(listener);
  }

  subscribeLocalUpdates(listener: (bytes: Uint8Array) => void): Subscription {
    this.#localUpdateSubscribers.add(listener);
    return () => this.#localUpdateSubscribers.delete(listener);
  }

  subscribeFirstCommitFromPeer(
    listener: (event: { peer: PeerID }) => void,
  ): Subscription {
    this.#firstCommitSubscribers.add(listener);
    return () => this.#firstCommitSubscribers.delete(listener);
  }

  subscribePreCommit(
    listener: (event: {
      changeMeta: Change;
      origin: string;
      modifier: ChangeModifier;
    }) => void,
  ): Subscription {
    this.#preCommitSubscribers.add(listener);
    return () => this.#preCommitSubscribers.delete(listener);
  }

  subscribeJsonpath(path: string, callback: () => void): Subscription {
    const matches = compileJsonPathEventMatcher(path);
    return this.subscribe((event) => {
      if (event.events.some(matches)) callback();
    });
  }

  JSONPath(path: string): unknown[] {
    if (path === "$") return [Object.fromEntries(this.#roots)];
    return evaluateJsonPath(this.#roots, path);
  }

  getPathToContainer(id: ContainerID): Path | undefined {
    const container = this.getContainerById(id);
    if (container === undefined || this._isContainerDeleted(container)) return undefined;
    return containerPath(container);
  }

  getByPath(path: string): unknown {
    const parts = path.split("/").filter(Boolean);
    if (parts.length === 0) return undefined;
    let value: unknown = this.#roots.get(parts[0]!);
    for (const part of parts.slice(1)) {
      if (value instanceof LoroMap) {
        value = value.get(part);
      } else if (value instanceof LoroList) {
        value = value.get(parsePathIndex(part));
      } else if (value instanceof LoroTree) {
        value = treeNodeAtPath(value, undefined, part);
      } else if (value instanceof LoroTreeNode) {
        const index = parseOptionalPathIndex(part);
        value = index === undefined ? value.data.get(part) : value._childAt(index);
      } else if (Array.isArray(value)) {
        value = value[parsePathIndex(part)];
      } else if (typeof value === "object" && value !== null) {
        value = (value as Record<string, unknown>)[part];
      } else {
        return undefined;
      }
      if (value === undefined) return undefined;
    }
    return value instanceof LoroTreeNode ? value.data : value;
  }

  getCursorPos(
    cursor: Cursor,
  ): { update?: Cursor; offset: number; side: Side } | undefined {
    const container = this.getContainerById(cursor.containerId());
    if (container === undefined || this._isContainerDeleted(container)) return undefined;
    const id = cursor._idValue();
    if (!(container instanceof LoroList || container instanceof LoroText))
      return undefined;
    const publicLength = container.length;
    if (id === undefined) {
      return {
        offset: Math.min(cursor._originPositionValue(), publicLength),
        side: cursor.side(),
      };
    }

    const target = container._sequence.findById(id);
    if (target === undefined) return undefined;
    const publicOffset = (element: SequenceElement): number =>
      container instanceof LoroText
        ? (container._sequence.visibleMetricOffsetOf(element as TextElement, "utf16") ??
          0)
        : (container._sequence.visibleIndexOf(element) ?? 0);
    if (!target.deleted) {
      const offset = publicOffset(target);
      return {
        offset:
          offset +
          (cursor.side() === 1
            ? container instanceof LoroText
              ? (target.value as string).length
              : 1
            : 0),
        side: cursor.side(),
      };
    }

    const next = container._sequence.nextVisible(target as never) as
      | SequenceElement
      | undefined;
    if (next !== undefined) {
      const offset = publicOffset(next);
      return {
        offset,
        side: -1,
        update: new Cursor(container.id, next.id, -1, offset),
      };
    }
    const previous = container._sequence.previousVisible(target as never);
    if (previous !== undefined) {
      return {
        offset: publicLength,
        side: 1,
        update: new Cursor(container.id, previous.id, 1, publicLength),
      };
    }
    return {
      offset: 0,
      side: 0,
      update: new Cursor(container.id, undefined, 0, 0),
    };
  }

  _subscribeContainer(
    container: LoroContainer,
    listener: (event: LoroEventBatch) => void,
  ): Subscription {
    let listeners = this.#containerSubscribers.get(container.id);
    if (listeners === undefined) {
      listeners = new Set();
      this.#containerSubscribers.set(container.id, listeners);
    }
    listeners.add(listener);
    return () => {
      listeners!.delete(listener);
      if (
        listeners!.size === 0 &&
        this.#containerSubscribers.get(container.id) === listeners
      ) {
        this.#containerSubscribers.delete(container.id);
      }
    };
  }

  _isContainerDeleted(container: LoroContainer): boolean {
    if (
      container._codecId?.kind === "root" &&
      !isMergeableContainerId(container._codecId)
    )
      return !this.#roots.has(container._codecId.name);
    const parent = container.parent();
    if (
      container._codecId !== undefined &&
      isMergeableContainerId(container._codecId) &&
      parent === undefined
    )
      return true;
    const binding =
      container._parentLink?.binding ??
      (parent === undefined ? undefined : recoverParentBinding(container, parent));
    if (binding?.kind === "map" && parent instanceof LoroMap) {
      const record = parent._entries.get(binding.key);
      return record === undefined || record.deleted || record.value !== container;
    }
    if (binding?.kind === "sequence" && parent instanceof LoroList) {
      return binding.element.deleted || binding.element.value !== container;
    }
    if (binding?.kind === "tree" && parent instanceof LoroTree) {
      return binding.record.deleted || binding.record.data !== container;
    }
    return parent !== undefined;
  }

  _undoIdSpan(peer: PeerID, range: CounterSpan): void {
    this.#commit({}, true);
    this.#materializeDeferredHistory();
    const parsedPeer = parsePeerId(peer);
    const records = this.#historyByPeer.get(parsedPeer) ?? [];
    let recordIndex = Math.min(
      records.length - 1,
      lowerBoundHistory(records, range.end) - 1,
    );
    for (; recordIndex >= 0; recordIndex -= 1) {
      const record = records[recordIndex]!;
      const changeEnd = record.change.id.counter + changeLength(record.change);
      if (changeEnd <= range.start) break;
      let operationIndex = Math.min(
        record.change.operations.length - 1,
        lowerBoundOperation(record.change.operations, range.end) - 1,
      );
      for (; operationIndex >= 0; operationIndex -= 1) {
        const operation = record.change.operations[operationIndex]!;
        if (operation.counter + operation.length <= range.start) break;
        this.#undoOperation(record, operation, parsedPeer, range);
      }
    }
  }

  _transformUndoCursors(cursors: readonly Cursor[]): Cursor[] {
    return cursors.map((cursor) => {
      const container = this.getContainerById(cursor.containerId());
      if (!(container instanceof LoroText || container instanceof LoroList)) {
        return cursor;
      }
      const position = Math.min(cursor._originPositionValue(), container.length);
      return container.getCursor(position, cursor.side()) ?? cursor;
    });
  }

  #undoOperation(
    record: HistoryRecord,
    operation: DecodedOperation,
    peer: bigint,
    range: CounterSpan,
  ): void {
    const container = this.#containers.get(formatContainerId(operation.container));
    if (container === undefined) return;
    const content = operation.content;
    const overlapStart = Math.max(operation.counter, range.start);
    const overlapEnd = Math.min(operation.counter + operation.length, range.end);
    if (overlapStart >= overlapEnd) return;

    switch (content.type) {
      case "text-insert":
        if (container instanceof LoroText) {
          this.#deleteInsertedElements(container, peer, overlapStart, overlapEnd);
        }
        return;
      case "list-insert":
      case "movable-list-insert":
        if (container instanceof LoroList) {
          this.#deleteInsertedElements(container, peer, overlapStart, overlapEnd);
        }
        return;
      case "text-delete":
        if (container instanceof LoroText) {
          this.#restoreDeletedElements(container, peer, range, overlapStart, overlapEnd);
        }
        return;
      case "list-delete":
      case "movable-list-delete":
        if (container instanceof LoroList) {
          this.#restoreDeletedElements(container, peer, range, overlapStart, overlapEnd);
        }
        return;
      case "map-insert":
      case "map-delete":
        if (container instanceof LoroMap) {
          this.#undoMapOperation(container, record, operation);
        }
        return;
      case "tree-create":
      case "tree-move":
      case "tree-delete":
        if (container instanceof LoroTree) {
          this.#undoTreeOperation(container, record, operation);
        }
        return;
      case "future":
        if (
          container instanceof LoroCounter &&
          (content.value.type === "double" || content.value.type === "i64")
        ) {
          container.increment(
            -(content.value.type === "double"
              ? content.value.value
              : Number(content.value.value)),
          );
        } else if (
          container instanceof LoroCounter &&
          content.value.type === "delta-int"
        ) {
          container.increment(-content.value.value);
        }
        return;
      case "text-mark":
      case "text-mark-end":
      case "movable-list-move":
      case "movable-list-set":
        return;
    }
  }

  #deleteInsertedElements(
    container: LoroText | LoroList,
    peer: bigint,
    start: number,
    end: number,
  ): void {
    const positions: number[] = [];
    for (let counter = start; counter < end; counter += 1) {
      const element = container._sequence.findById({ peer, counter });
      if (element === undefined || element.deleted) continue;
      const index = container._sequence.visibleIndexOf(element as never);
      if (index !== undefined) positions.push(index);
    }
    positions.sort((left, right) => left - right);
    const ranges: { start: number; length: number }[] = [];
    for (const position of positions) {
      const previous = ranges.at(-1);
      if (previous !== undefined && previous.start + previous.length === position) {
        previous.length += 1;
      } else {
        ranges.push({ start: position, length: 1 });
      }
    }
    for (const selected of ranges.reverse()) {
      if (container instanceof LoroText) {
        this._textDelete(container, selected.start, selected.length);
      } else {
        this._sequenceDelete(container, selected.start, selected.length);
      }
    }
  }

  #restoreDeletedElements(
    container: LoroText | LoroList,
    peer: bigint,
    range: CounterSpan,
    overlapStart: number,
    overlapEnd: number,
  ): void {
    const belongsToUndo = (id: CodecId): boolean =>
      id.peer === peer && id.counter >= range.start && id.counter < range.end;
    const belongsToOperation = (id: CodecId): boolean =>
      id.peer === peer && id.counter >= overlapStart && id.counter < overlapEnd;
    const targets = container._sequence
      .elementsDeletedBy(peer, overlapStart, overlapEnd)
      .filter(
        (element) =>
          !belongsToUndo(element.id) &&
          container._sequence.someDeletion(element, belongsToOperation) &&
          container._sequence.everyDeletion(element, belongsToUndo),
      );
    const groups: (typeof targets)[] = [];
    for (const target of targets) {
      const previous = groups.at(-1)?.at(-1);
      if (
        previous !== undefined &&
        container._sequence.physicalIndexOf(previous as never)! + 1 ===
          container._sequence.physicalIndexOf(target as never)!
      ) {
        groups.at(-1)!.push(target);
      } else {
        groups.push([target]);
      }
    }

    for (const group of groups) {
      const position = container._sequence.visibleIndexOf(group[0] as never)!;
      if (container instanceof LoroText) {
        const text = group
          .map((element) => {
            if (typeof element.value !== "string") {
              throw new TypeError("text element value must be a string");
            }
            return element.value;
          })
          .join("");
        this._textInsert(container, position, text);
      } else {
        let offset = 0;
        for (const element of group) {
          if (isContainer(element.value)) {
            const child = createContainer(element.value.kind()) as Container;
            restoreBlueprint(child, captureBlueprint(element.value));
            this._listInsertContainer(container, position + offset, child);
          } else {
            this._listInsert(
              container,
              position + offset,
              cloneRuntimeValue(element.value),
            );
          }
          offset += 1;
        }
      }
    }
  }

  #undoMapOperation(
    map: LoroMap,
    record: HistoryRecord,
    operation: DecodedOperation,
  ): void {
    const content = operation.content;
    if (content.type !== "map-insert" && content.type !== "map-delete") return;
    const writer = operationWriter(record.change, operation);
    const current = map._entries.get(content.key);
    if (current === undefined || compareWriter(current.writer, writer) !== 0) return;

    const previous = this.#previousMapOperation(operation.container, content.key, writer);
    if (previous === undefined || previous.operation.content.type === "map-delete") {
      map.delete(content.key);
      return;
    }
    const previousContent = previous.operation.content;
    if (previousContent.type !== "map-insert") return;
    const previousId = {
      peer: previous.record.change.id.peer,
      counter: previous.operation.counter,
    };
    const rawValue = this.#decodeRuntimeValue(
      previousContent.value,
      previous.record.keys,
      previousId,
      map,
    );
    const value = this.#materializeMapValue(map, content.key, rawValue);
    if (isContainer(value)) {
      const child = createContainer(value.kind()) as Container;
      restoreBlueprint(child, captureBlueprint(value));
      map.setContainer(content.key, child);
    } else {
      map.set(content.key, cloneRuntimeValue(value));
    }
  }

  #previousMapOperation(
    containerId: CodecContainerId,
    key: string,
    before: LastWriter,
  ): { record: HistoryRecord; operation: DecodedOperation } | undefined {
    const operations = this.#mapOperationHistory
      .get(formatContainerId(containerId))
      ?.get(key);
    if (operations === undefined) return undefined;
    const previous = operations.byWriter.at(
      operations.byWriter._lowerBoundBy((operation) =>
        compareWriter(operation.writer, before),
      ) - 1,
    );
    return previous === undefined
      ? undefined
      : { record: previous.record, operation: previous.operation };
  }

  #undoTreeOperation(
    tree: LoroTree,
    record: HistoryRecord,
    operation: DecodedOperation,
  ): void {
    const content = operation.content;
    if (
      content.type !== "tree-create" &&
      content.type !== "tree-move" &&
      content.type !== "tree-delete"
    ) {
      return;
    }
    const writer = operationWriter(record.change, operation);
    const node = tree._nodes.get(formatTreeId(content.subject));
    if (node === undefined || compareWriter(node.writer, writer) !== 0) return;
    const previous = this.#previousTreeOperation(
      operation.container,
      content.subject,
      writer,
    );
    if (previous === undefined || previous.operation.content.type === "tree-delete") {
      this.#appendAndApply(tree, { type: "tree-delete", subject: content.subject }, 1);
      return;
    }
    const previousContent = previous.operation.content;
    if (previousContent.type !== "tree-create" && previousContent.type !== "tree-move") {
      return;
    }
    this.#appendAndApply(
      tree,
      {
        type: "tree-move",
        subject: content.subject,
        parent: previousContent.parent,
        position: previousContent.position.slice(),
      },
      1,
    );
  }

  #previousTreeOperation(
    containerId: CodecContainerId,
    subject: CodecId,
    before: LastWriter,
  ): { record: HistoryRecord; operation: DecodedOperation } | undefined {
    const operations = this.#treeOperationHistory
      .get(formatContainerId(containerId))
      ?.get(idKey(subject));
    if (operations === undefined) return undefined;
    const previous = operations.byWriter.at(
      operations.byWriter._lowerBoundBy((operation) =>
        compareWriter(operation.writer, before),
      ) - 1,
    );
    return previous === undefined
      ? undefined
      : { record: previous.record, operation: previous.operation };
  }

  _mapSet(container: LoroMap, key: string, value: unknown): void {
    const current = container._entries.get(key);
    if (
      current !== undefined &&
      !current.deleted &&
      eventValuesEqual(current.value, normalizeComparableValue(value))
    ) {
      return;
    }
    const encoded = this.#encodeRuntimeValue(value, this.#ensurePending());
    this.#appendAndApply(container, { type: "map-insert", key, value: encoded }, 1);
  }

  _mapDelete(container: LoroMap, key: string): void {
    const current = container._entries.get(key);
    if (current === undefined || current.deleted) return;
    this.#appendAndApply(container, { type: "map-delete", key }, 1);
  }

  _mapSetContainer<C extends Container>(container: LoroMap, key: string, child: C): C {
    return this.#attachChild(container, child, (rawType) => ({
      type: "map-insert",
      key,
      value: { type: "container-type", value: rawType },
    }));
  }

  _mapEnsureMergeable(container: LoroMap, key: string, type: ContainerType): Container {
    const parentId = container._codecId;
    if (parentId === undefined || container._doc !== this) {
      throw new Error("cannot ensure a mergeable child on a detached map");
    }

    const codecType = publicTypeToCodec(type);
    const childId = newMergeableContainerId(parentId, key, codecType);
    const marker = mergeableMarker(parentId, key, codecType);
    const existing = container._entries.get(key);
    if (existing !== undefined && !existing.deleted && existing.rawValue !== null) {
      const existingType = parseMergeableMarker(parentId, key, existing.rawValue);
      if (existingType === undefined) {
        throw new TypeError(
          `cannot create a mergeable ${type} at key ${JSON.stringify(key)}: ` +
            "the key already holds a non-mergeable value",
        );
      }
      if (
        existing.rawValue instanceof Uint8Array &&
        bytesEqual(existing.rawValue, marker)
      ) {
        return this.#getOrCreateContainer(childId, container) as Container;
      }
    }

    const child = this.#getOrCreateContainer(childId, container) as Container;
    this.#appendAndApply(
      container,
      { type: "map-insert", key, value: { type: "binary", value: marker } },
      1,
    );
    return child;
  }

  _listInsert(container: LoroList, position: number, value: unknown): void {
    const encoded = this.#encodeRuntimeValue(value, this.#ensurePending());
    const type =
      container instanceof LoroMovableList ? "movable-list-insert" : "list-insert";
    this.#appendAndApply(container, { type, position, values: [encoded] }, 1);
  }

  _listInsertContainer<C extends Container>(
    container: LoroList,
    position: number,
    child: C,
  ): C {
    return this.#attachChild(container, child, (rawType) => ({
      type: container instanceof LoroMovableList ? "movable-list-insert" : "list-insert",
      position,
      values: [{ type: "container-type", value: rawType }],
    }));
  }

  _sequenceDelete(container: LoroList, position: number, length: number): void {
    this.#deleteSequenceRuns(
      container,
      position,
      length,
      container instanceof LoroMovableList ? "movable-list-delete" : "list-delete",
    );
  }

  _textInsert(container: LoroText, position: number, text: string): void {
    this.#appendAndApply(
      container,
      { type: "text-insert", position, value: text },
      unicodeScalarLength(text),
    );
  }

  _textDelete(container: LoroText, position: number, length: number): void {
    this.#deleteTextRuns(container, position, length);
  }

  _textMark(
    container: LoroText,
    start: number,
    end: number,
    key: string,
    value: unknown,
  ): void {
    const encoded = this.#encodeRuntimeValue(value, this.#ensurePending());
    const info = textStyleInfoByte(this.#textStyleExpand(key), value === null);
    this.#appendAndApply(
      container,
      { type: "text-mark", start, end, key, value: encoded, info },
      1,
    );
    this.#appendAndApply(container, { type: "text-mark-end" }, 1);
  }

  _movableMove(container: LoroMovableList, from: number, to: number): void {
    const element = container._visibleElementAt(from)!;
    this.#appendAndApply(
      container,
      {
        type: "movable-list-move",
        from,
        to,
        elementId: { peer: element.id.peer, lamport: element.lamport },
      },
      1,
    );
  }

  _movableSet(container: LoroMovableList, position: number, value: unknown): void {
    const element = container._visibleElementAt(position)!;
    if (eventValuesEqual(element.value, normalizeComparableValue(value))) return;
    const encoded = this.#encodeRuntimeValue(value, this.#ensurePending());
    this.#appendAndApply(
      container,
      {
        type: "movable-list-set",
        elementId: { peer: element.id.peer, lamport: element.lamport },
        value: encoded,
      },
      1,
    );
  }

  _movableSetContainer<C extends Container>(
    container: LoroMovableList,
    position: number,
    child: C,
  ): C {
    const element = container._visibleElementAt(position)!;
    return this.#attachChild(container, child, (rawType) => ({
      type: "movable-list-set",
      elementId: { peer: element.id.peer, lamport: element.lamport },
      value: { type: "container-type", value: rawType },
    }));
  }

  _counterIncrement(container: LoroCounter, value: number): void {
    if (value === 0) return;
    this.#appendAndApply(
      container,
      { type: "future", property: 0, value: { type: "double", value } },
      1,
    );
  }

  #textStyleExpand(key: string): TextStyleExpand {
    const baseKey = key.split(":", 1)[0]!;
    const configured = this.#textStyles.get(baseKey) ?? this.#defaultTextStyle;
    if (configured === undefined) {
      throw new RangeError(`text style ${JSON.stringify(baseKey)} is not configured`);
    }
    return configured;
  }

  _treeCreate<TData extends Record<string, unknown>>(
    tree: LoroTree<TData>,
    parent?: TreeID,
    index?: number,
  ): LoroTreeNode<TData> {
    const pending = this.#ensurePending();
    const subject = { peer: this.#peer, counter: this.#nextOperationCounter(pending) };
    const parentId = parent === undefined ? undefined : parseTreeId(parent);
    if (parent !== undefined && !tree.has(parent)) {
      throw new RangeError(`tree parent ${parent} does not exist`);
    }
    const position = tree._positionFor(parentId, index);
    this.#appendAndApply(
      tree,
      { type: "tree-create", subject, parent: parentId, position },
      1,
    );
    return new LoroTreeNode(tree, subject);
  }

  _treeMove(tree: LoroTree, target: TreeID, parent?: TreeID, index?: number): void {
    if (!tree.has(target)) throw new RangeError(`tree node ${target} does not exist`);
    if (parent !== undefined && !tree.has(parent)) {
      throw new RangeError(`tree parent ${parent} does not exist`);
    }
    const subject = parseTreeId(target);
    const parentId = parent === undefined ? undefined : parseTreeId(parent);
    let ancestor = parentId;
    while (ancestor !== undefined) {
      if (idsEqual(ancestor, subject)) {
        throw new RangeError("cannot move a tree node below itself or its descendant");
      }
      ancestor = tree._nodes.get(formatTreeId(ancestor))?.parent;
    }
    const position = tree._positionFor(parentId, index, subject);
    this.#appendAndApply(
      tree,
      { type: "tree-move", subject, parent: parentId, position },
      1,
    );
  }

  _treeDelete(tree: LoroTree, target: TreeID): void {
    this.#appendAndApply(tree, { type: "tree-delete", subject: parseTreeId(target) }, 1);
  }

  #getContainer(nameOrId: string, expectedType: ContainerType): LoroContainer {
    const byId = isContainerId(nameOrId);
    const id: CodecContainerId = byId
      ? parseContainerId(nameOrId)
      : { kind: "root", name: nameOrId, containerType: publicTypeToCodec(expectedType) };
    if (codecTypeToPublic(id.containerType) !== expectedType) {
      throw new TypeError(`container ${nameOrId} is not a ${expectedType}`);
    }
    if (
      byId &&
      (id.kind === "normal" || isMergeableContainerId(id)) &&
      !this.#containers.has(formatContainerId(id))
    ) {
      throw new RangeError(`container ${nameOrId} does not exist in this document`);
    }
    const container = this.#getOrCreateContainer(id);
    if (id.kind === "root" && !isMergeableContainerId(id)) {
      this.#roots.set(id.name, container);
    }
    return container;
  }

  #getOrCreateContainer(id: CodecContainerId, parent?: LoroContainer): LoroContainer {
    const key = this.#containerKey(id);
    const existing = this.#containers.get(key);
    if (existing !== undefined) {
      if (parent !== undefined) existing._parentLink = { container: parent };
      return existing;
    }
    const container = createContainer(codecTypeToPublic(id.containerType));
    container._attach(this, id, parent);
    this.#containers.set(key, container);
    if (id.kind === "root" && !isMergeableContainerId(id)) {
      this.#roots.set(id.name, container);
    }
    return container;
  }

  #containerKey(id: CodecContainerId): string {
    let key = this.#containerKeys.get(id);
    if (key === undefined) {
      key = formatContainerId(id);
      this.#containerKeys.set(id, key);
    }
    return key;
  }

  #ensurePending(): PendingChange {
    if (this.#pending !== undefined) return this.#pending;
    this.#materializeDeferredHistory();
    if (this.#detached && !this.#detachedEditing) {
      throw new Error("cannot edit a detached document; call attach() first");
    }
    const from = this.#frontiersCodec();
    const lamport = from.reduce(
      (max, dependency) => Math.max(max, this.#lamportAt(dependency) + 1),
      0,
    );
    this.#nextCounter = Math.max(
      this.#nextCounter,
      this.oplogVersion().get(this.#peer) ?? 0,
    );
    this.#pending = {
      id: { peer: this.#peer, counter: this.#nextCounter },
      dependencies: from.map((id) => ({ ...id })),
      lamport,
      from,
      operations: [],
      operationLength: 0,
      causalVersion: this.#causalVersionAt(from),
      keys: [],
      keyIndices: new Map(),
      changedContainers: new Set(),
      beforeValues: new Map(),
      eventStates: new Map(),
    };
    return this.#pending;
  }

  #nextOperationCounter(pending: PendingChange): number {
    return pending.id.counter + pending.operationLength;
  }

  #appendAndApply(
    container: LoroContainer,
    content: DecodedOperationContent,
    length: number,
    preferredChild?: LoroContainer,
  ): DecodedOperation {
    if (container._codecId === undefined || container._doc !== this)
      throw new Error("container is detached");
    const pending = this.#ensurePending();
    const counter = this.#nextOperationCounter(pending);
    const operation: DecodedOperation = {
      container: container._codecId,
      counter,
      length,
      content,
    };
    if (preferredChild !== undefined) {
      const childId = this.#childIdForOperation(operation, preferredChild.kind());
      preferredChild._attach(this, childId, container);
      this.#containers.set(preferredChild.id, preferredChild);
    }
    if (!appendToTrailingListInsert(pending.operations, operation)) {
      pending.operations.push(operation);
    }
    pending.operationLength += operation.length;
    pending.changedContainers.add(container.id);
    const causalVersion = pending.causalVersion;
    causalVersion.set(
      pending.id.peer,
      Math.max(causalVersion.get(pending.id.peer) ?? 0, operation.counter),
    );
    const finishEvent = this.#prepareEvent(
      pending,
      container,
      operation,
      pending.keys,
      pending.id,
      causalVersion,
    );
    this.#applyOperation(
      operation,
      pending.keys,
      pending.id,
      pending.lamport,
      causalVersion,
      container,
    );
    finishEvent?.();
    return operation;
  }

  #prepareEvent(
    recording: EventRecording,
    container: LoroContainer,
    operation: DecodedOperation,
    keys: readonly string[],
    changeId: CodecId,
    causalVersion: CausalVersion,
    force = false,
  ): (() => void) | undefined {
    if (!force && !this.#hasEventSubscribers()) return undefined;
    const content = operation.content;
    if (container instanceof LoroText) {
      const state = this.#sequenceEventState(recording, container, "text");
      if (content.type === "text-insert") {
        return () => {
          const first = container._sequence.findById({
            peer: changeId.peer,
            counter: operation.counter,
          });
          if (first === undefined || first.deleted) return;
          const position = container._sequence.visibleMetricOffsetOf(first, "utf16");
          if (position === undefined) return;
          const attributes = Object.fromEntries(
            [...container._attributesAt(first)].map(([key, value]) => [
              key,
              runtimeValueToJson(value) as Value,
            ]),
          );
          state.diff.insertText(position, content.value, attributes);
        };
      }
      if (content.type === "text-delete") {
        for (const range of this.#sequenceEventDeletionRanges(
          container,
          content.startId,
          Math.abs(Number(content.length)),
        ).reverse()) {
          state.diff.delete(range.position, range.length);
        }
        return undefined;
      }
      if (content.type === "text-mark") {
        const value = this.#decodeRuntimeValue(
          content.value,
          keys,
          { peer: changeId.peer, counter: operation.counter },
          container,
        );
        const runs = container._styleRuns(content.start, content.end, causalVersion);
        for (const range of container._sequence.visibleMetricRangesForIdRuns(
          runs,
          "utf16",
        )) {
          state.diff.formatText(
            range.start,
            range.end - range.start,
            content.key,
            runtimeValueToJson(value) as Value,
          );
        }
        return undefined;
      }
      if (content.type === "text-mark-end") return undefined;
    } else if (container instanceof LoroList) {
      const state = this.#sequenceEventState(recording, container, "list");
      if (content.type === "list-insert" || content.type === "movable-list-insert") {
        return () => {
          const first = container._sequence.findById({
            peer: changeId.peer,
            counter: operation.counter,
          });
          if (first === undefined || first.deleted) return;
          const position = container._sequence.visibleIndexOf(first);
          if (position === undefined) return;
          state.diff.insertList(
            position,
            container
              ._visibleElementsRange(position, position + content.values.length)
              .map((element) => cloneRuntimeValue(element.value)),
          );
        };
      }
      if (content.type === "list-delete" || content.type === "movable-list-delete") {
        for (const range of this.#sequenceEventDeletionRanges(
          container,
          content.startId,
          Math.abs(Number(content.length)),
        ).reverse()) {
          state.diff.delete(range.position, range.length);
        }
        return undefined;
      }
      if (content.type === "movable-list-move") {
        const element = container._sequence.findByLamport(
          content.elementId.peer,
          content.elementId.lamport,
        );
        const from =
          element === undefined || element.deleted
            ? undefined
            : container._sequence.visibleIndexOf(element);
        if (element === undefined || from === undefined) return undefined;
        const value = cloneRuntimeValue(element.value);
        return () => {
          const to = element.deleted
            ? undefined
            : container._sequence.visibleIndexOf(element);
          if (to === undefined || to === from) return;
          state.diff.delete(from, 1);
          state.diff.insertList(to, [value]);
        };
      }
      if (content.type === "movable-list-set") {
        const element = container._sequence.findByLamport(
          content.elementId.peer,
          content.elementId.lamport,
        );
        const position =
          element === undefined || element.deleted
            ? undefined
            : container._sequence.visibleIndexOf(element);
        if (element === undefined || position === undefined) return undefined;
        const selected = element;
        const previous = cloneRuntimeValue(element.value);
        return () => {
          const value = cloneRuntimeValue(selected.value);
          if (eventValuesEqual(previous, value)) return;
          state.diff.delete(position, 1);
          state.diff.insertList(position, [value]);
        };
      }
    } else if (container instanceof LoroMap) {
      if (content.type === "map-insert" || content.type === "map-delete") {
        const state = this.#mapEventState(recording, container);
        if (!state.originals.has(content.key)) {
          const record = container._entries.get(content.key);
          state.originals.set(content.key, {
            present: record !== undefined,
            visible: record !== undefined && !record.deleted,
            value:
              record === undefined || record.deleted
                ? undefined
                : cloneRuntimeValue(record.value),
          });
        }
        return undefined;
      }
    } else if (container instanceof LoroTree) {
      if (
        content.type === "tree-create" ||
        content.type === "tree-move" ||
        content.type === "tree-delete"
      ) {
        const state = this.#treeEventState(recording, container);
        const id = formatTreeId(content.subject);
        if (!state.originals.has(id)) {
          const record = container._nodes.get(id);
          state.originals.set(
            id,
            record === undefined || record.deleted
              ? undefined
              : this.#treeEventNode(container, record),
          );
        }
        return undefined;
      }
    } else if (container instanceof LoroCounter) {
      if (!recording.eventStates.has(container.id)) {
        recording.eventStates.set(container.id, {
          kind: "counter",
          before: container.value,
        });
      }
      return undefined;
    }

    if (!recording.beforeValues.has(container.id)) {
      recording.beforeValues.set(container.id, containerEventValue(container));
    }
    return undefined;
  }

  #sequenceEventState(
    recording: EventRecording,
    container: LoroText | LoroList,
    kind: "text" | "list",
  ): Extract<PendingEventState, { kind: "sequence" }> {
    const existing = recording.eventStates.get(container.id);
    if (existing !== undefined) {
      if (existing.kind !== "sequence") throw new Error("event state kind changed");
      return existing;
    }
    const state = {
      kind: "sequence" as const,
      diff: new SequenceEventDiff(kind, container.length),
    };
    recording.eventStates.set(container.id, state);
    return state;
  }

  #mapEventState(
    recording: EventRecording,
    container: LoroMap,
  ): Extract<PendingEventState, { kind: "map" }> {
    const existing = recording.eventStates.get(container.id);
    if (existing !== undefined) {
      if (existing.kind !== "map") throw new Error("event state kind changed");
      return existing;
    }
    const state = {
      kind: "map" as const,
      originals: new Map<
        string,
        { present: boolean; visible: boolean; value: unknown }
      >(),
    };
    recording.eventStates.set(container.id, state);
    return state;
  }

  #treeEventState(
    recording: EventRecording,
    container: LoroTree,
  ): Extract<PendingEventState, { kind: "tree" }> {
    const existing = recording.eventStates.get(container.id);
    if (existing !== undefined) {
      if (existing.kind !== "tree") throw new Error("event state kind changed");
      return existing;
    }
    const state = {
      kind: "tree" as const,
      originals: new Map<TreeID, TreeEventNode | undefined>(),
    };
    recording.eventStates.set(container.id, state);
    return state;
  }

  #treeEventNode(tree: LoroTree, record: TreeNodeRecord): TreeEventNode {
    return {
      id: formatTreeId(record.id),
      parent: record.parent === undefined ? undefined : formatTreeId(record.parent),
      index: tree._indexOf(record),
      fractionalIndex: bytesToHex(record.position).toUpperCase(),
    };
  }

  #sequenceEventDeletionRanges(
    container: LoroText | LoroList,
    startId: CodecId,
    length: number,
  ): { position: number; length: number }[] {
    return container._sequence
      .visibleMetricRangesForIdRuns(
        [{ start: { peer: startId.peer, counter: startId.counter }, length }],
        "utf16",
      )
      .map(({ start, end }) => ({ position: start, length: end - start }));
  }

  #recordedEventDiffs(
    recording: EventRecording,
    includeMapTombstones = false,
  ): Map<string, Diff> {
    const diffs = new Map<string, Diff>();
    for (const [id, state] of recording.eventStates) {
      const container = this.#containers.get(id);
      if (container === undefined) continue;
      if (state.kind === "sequence") {
        diffs.set(id, state.diff.toDiff());
      } else if (state.kind === "map" && container instanceof LoroMap) {
        const updated: [string, unknown][] = [];
        for (const [key, original] of state.originals) {
          const record = container._entries.get(key);
          const present = record !== undefined;
          const visible = record !== undefined && !record.deleted;
          const value = visible ? cloneRuntimeValue(record.value) : undefined;
          if (
            (includeMapTombstones && present !== original.present) ||
            visible !== original.visible ||
            !eventValuesEqual(value, original.value)
          ) {
            updated.push([key, value]);
          }
        }
        diffs.set(id, { type: "map", updated: Object.fromEntries(updated) });
      } else if (state.kind === "counter" && container instanceof LoroCounter) {
        diffs.set(id, { type: "counter", increment: container.value - state.before });
      } else if (state.kind === "tree" && container instanceof LoroTree) {
        const before: TreeEventNode[] = [];
        const after: TreeEventNode[] = [];
        for (const [nodeId, original] of state.originals) {
          if (original !== undefined) before.push(original);
          const record = container._nodes.get(nodeId);
          if (record !== undefined && !record.deleted) {
            after.push(this.#treeEventNode(container, record));
          }
        }
        diffs.set(id, { type: "tree", diff: treeDelta(before, after) });
      }
    }
    return diffs;
  }

  #attachChild<C extends Container>(
    parent: LoroContainer,
    child: C,
    content: (rawType: number) => DecodedOperationContent,
  ): C {
    if (child._doc !== undefined && child._doc !== this) {
      throw new Error("cannot attach a container from another document");
    }
    const blueprint = captureBlueprint(child);
    const attached = createContainer(child.kind()) as C;
    const rawType = containerTypeToRawByte(publicTypeToCodec(child.kind()));
    this.#appendAndApply(parent, content(rawType), 1, attached);
    restoreBlueprint(attached, blueprint);
    if (!child.isAttached()) child._attached = attached;
    return attached;
  }

  #childIdForOperation(
    operation: DecodedOperation,
    type: ContainerType,
  ): CodecContainerId {
    return {
      kind: "normal",
      peer: this.#peer,
      counter: operation.counter,
      containerType: publicTypeToCodec(type),
    };
  }

  #deleteSequenceRuns(
    container: LoroList,
    position: number,
    length: number,
    type: "list-delete" | "movable-list-delete",
  ): void {
    for (const run of container._sequence.visibleIdRuns(position, position + length)) {
      this.#appendAndApply(
        container,
        { type, position, length: BigInt(run.length), startId: run.start },
        run.length,
      );
    }
  }

  #deleteTextRuns(container: LoroText, position: number, length: number): void {
    for (const run of container._sequence.visibleIdRuns(position, position + length)) {
      this.#appendAndApply(
        container,
        {
          type: "text-delete",
          position,
          length: BigInt(run.length),
          startId: run.start,
        },
        run.length,
      );
      this.#mergeTrailingTextDeletes();
    }
  }

  #mergeTrailingTextDeletes(): void {
    const operations = this.#pending?.operations;
    if (operations === undefined || operations.length < 2) return;
    const left = operations[operations.length - 2]!;
    const right = operations[operations.length - 1]!;
    if (
      left.content.type !== "text-delete" ||
      right.content.type !== "text-delete" ||
      !containerIdsEqual(left.container, right.container)
    ) {
      return;
    }

    const leftLength = Number(left.content.length);
    const rightLength = Number(right.content.length);
    if (rightLength <= 0 || left.content.startId.peer !== right.content.startId.peer) {
      return;
    }

    let mergedLength: number | undefined;
    let startId = left.content.startId;
    if (
      leftLength > 0 &&
      left.content.position === right.content.position &&
      right.content.startId.counter === left.content.startId.counter + leftLength
    ) {
      mergedLength = leftLength + rightLength;
    } else if (
      leftLength === 1 &&
      right.content.position + rightLength === left.content.position &&
      right.content.startId.counter + rightLength === left.content.startId.counter
    ) {
      mergedLength = -(leftLength + rightLength);
      startId = right.content.startId;
    } else if (
      leftLength < 0 &&
      right.content.position + rightLength - 1 === left.content.position + leftLength &&
      right.content.startId.counter + rightLength === left.content.startId.counter
    ) {
      mergedLength = leftLength - rightLength;
      startId = right.content.startId;
    }
    if (mergedLength === undefined) return;

    operations.splice(operations.length - 2, 2, {
      ...left,
      length: left.length + right.length,
      content: {
        ...left.content,
        length: BigInt(mergedLength),
        startId,
      },
    });
  }

  #encodeRuntimeValue(value: unknown, pending: PendingChange): ChangeLoroValue {
    if (value == null) return { type: "null" };
    if (typeof value === "boolean") return { type: "bool", value };
    if (typeof value === "number") {
      if (!Number.isFinite(value)) throw new TypeError("Loro numbers must be finite");
      return Number.isSafeInteger(value)
        ? { type: "i64", value: BigInt(value) }
        : { type: "double", value };
    }
    if (typeof value === "string") return { type: "string", value };
    if (value instanceof Uint8Array) return { type: "binary", value: value.slice() };
    if (Array.isArray(value)) {
      return {
        type: "list",
        value: value.map((item) => this.#encodeRuntimeValue(item, pending)),
      };
    }
    if (isContainer(value))
      throw new TypeError("attach child containers with a container-specific method");
    if (typeof value === "object") {
      if (Object.getOwnPropertySymbols(value).length > 0) {
        throw new TypeError("Object keys must be strings");
      }
      return {
        type: "map",
        value: Object.entries(value).map(([key, item]) => [
          BigInt(registerKey(pending, key)),
          this.#encodeRuntimeValue(item, pending),
        ]),
      };
    }
    throw new TypeError(`unsupported Loro value type: ${typeof value}`);
  }

  #decodeRuntimeValue(
    value: ChangeLoroValue,
    keys: readonly string[],
    operationId: CodecId,
    parent: LoroContainer,
  ): RuntimeValue {
    switch (value.type) {
      case "null":
        return null;
      case "bool":
        return value.value;
      case "i64":
        return Number(value.value);
      case "double":
        return value.value;
      case "string":
        return value.value;
      case "binary":
        return value.value.slice();
      case "list":
        return value.value.map((item, index) =>
          this.#decodeRuntimeValue(
            item,
            keys,
            { peer: operationId.peer, counter: operationId.counter + index },
            parent,
          ),
        );
      case "map":
        return Object.fromEntries(
          value.value.map(([keyIndex, item]) => {
            const key = keys[Number(keyIndex)];
            if (key === undefined)
              throw new Error("change value map key index is out of range");
            return [key, this.#decodeRuntimeValue(item, keys, operationId, parent)];
          }),
        );
      case "container-type": {
        const containerType = containerTypeFromRawByte(value.value);
        const id: CodecContainerId = { kind: "normal", ...operationId, containerType };
        return this.#getOrCreateContainer(id, parent);
      }
    }
  }

  #applyOperation(
    operation: DecodedOperation,
    keys: readonly string[],
    changeId: CodecId,
    changeLamport: number,
    causalVersion: CausalVersion,
    knownContainer?: LoroContainer,
  ): void {
    const container = knownContainer ?? this.#getOrCreateContainer(operation.container);
    const operationId = { peer: changeId.peer, counter: operation.counter };
    const lamport = changeLamport + (operation.counter - changeId.counter);
    const writer: LastWriter = { peer: changeId.peer, lamport };
    const content = operation.content;
    switch (content.type) {
      case "map-insert":
        {
          const rawValue = this.#decodeRuntimeValue(
            content.value,
            keys,
            operationId,
            container,
          );
          (container as LoroMap)._applyValue(
            content.key,
            this.#materializeMapValue(container as LoroMap, content.key, rawValue),
            writer,
            rawValue,
          );
        }
        return;
      case "map-delete":
        (container as LoroMap)._applyDelete(content.key, writer);
        return;
      case "list-insert":
      case "movable-list-insert": {
        const values = content.values.map((value, index) =>
          this.#decodeRuntimeValue(
            value,
            keys,
            { peer: operationId.peer, counter: operationId.counter + index },
            container,
          ),
        );
        (container as LoroList)._insertFugue(
          content.position,
          values,
          values.map((_, index) => ({
            peer: operationId.peer,
            counter: operationId.counter + index,
          })),
          values.map((_, index) => lamport + index),
          causalVersion,
        );
        return;
      }
      case "list-delete":
      case "movable-list-delete":
        (container as LoroList)._deleteIdSpan(
          content.startId,
          Number(content.length),
          operationId,
        );
        return;
      case "text-insert": {
        (container as LoroText)._insertFugue(
          content.position,
          content.value,
          operationId,
          lamport,
          causalVersion,
        );
        return;
      }
      case "text-delete":
        (container as LoroText)._deleteIdSpan(
          content.startId,
          Number(content.length),
          operationId,
        );
        return;
      case "text-mark":
        {
          const value = this.#decodeRuntimeValue(
            content.value,
            keys,
            operationId,
            container,
          );
          const meta: TextStyleMeta = {
            startId: operationId,
            lamport,
            info: content.info,
            value,
          };
          (container as LoroText)._applyMark(
            content.start,
            content.end,
            content.key,
            value,
            meta,
            causalVersion,
          );
        }
        return;
      case "text-mark-end":
        return;
      case "movable-list-move": {
        const list = container as LoroMovableList;
        const element = list._sequence.findByLamport(
          content.elementId.peer,
          content.elementId.lamport,
        );
        const from =
          element === undefined || element.deleted
            ? undefined
            : list._sequence.visibleIndexOf(element);
        if (from !== undefined)
          list._applyMove(from, Math.min(content.to, list.length - 1), {
            id: operationId,
            lamport,
          });
        return;
      }
      case "movable-list-set": {
        const list = container as LoroMovableList;
        const element = list._sequence.findByLamport(
          content.elementId.peer,
          content.elementId.lamport,
        );
        if (element !== undefined) {
          const value = this.#decodeRuntimeValue(
            content.value,
            keys,
            operationId,
            container,
          );
          list._applySet(element, value, {
            id: operationId,
            lamport,
            value,
          });
        }
        return;
      }
      case "tree-create":
      case "tree-move": {
        const tree = container as LoroTree;
        const nodeKey = formatTreeId(content.subject);
        let record = tree._nodes.get(nodeKey);
        if (record === undefined) {
          const dataId: CodecContainerId = {
            kind: "normal",
            ...content.subject,
            containerType: CodecContainerType.Map,
          };
          const data = this.#getOrCreateContainer(dataId, tree) as LoroMap;
          record = {
            id: content.subject,
            parent: content.parent,
            position: content.position.slice(),
            deleted: false,
            writer,
            lastMoveId: operationId,
            data,
          };
          tree._setRecord(record);
        } else if (compareWriter(record.writer, writer) <= 0) {
          tree._updateRecord(
            record,
            content.parent,
            content.position.slice(),
            writer,
            operationId,
          );
        }
        return;
      }
      case "tree-delete": {
        const record = (container as LoroTree)._nodes.get(formatTreeId(content.subject));
        if (record !== undefined && compareWriter(record.writer, writer) <= 0) {
          (container as LoroTree)._deleteRecord(record, writer);
        }
        return;
      }
      case "future":
        if (container instanceof LoroCounter) {
          if (content.value.type === "double" || content.value.type === "i64") {
            container._value +=
              content.value.type === "double"
                ? content.value.value
                : Number(content.value.value);
          } else if (content.value.type === "delta-int") {
            container._value += content.value.value;
          }
        }
    }
  }

  #readChangeBlock(bytes: Uint8Array): HistoryRecord[] {
    const block = decodeChangeBlock(bytes);
    return block.changes.map((change) => ({ change, keys: block.keys }));
  }

  #decodeImportData(parsed: ParsedDocument): DecodedImportData {
    if (parsed.mode === EncodeMode.FastUpdates) {
      return {
        mode: parsed.mode,
        records: decodeFastUpdatesBody(parsed.body).flatMap((block) =>
          this.#readChangeBlock(block),
        ),
      };
    }

    const snapshot = decodeFastSnapshotBody(parsed.body);
    const records: HistoryRecord[] = [];
    let startVersion: VersionVector | undefined;
    let startFrontiers: CodecId[] | undefined;
    let endVersion: VersionVector | undefined;
    for (const entry of decodeSstable(snapshot.oplog)) {
      if (bytesEqual(entry.key, VERSION_KEY)) {
        endVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
      } else if (bytesEqual(entry.key, START_VERSION_KEY)) {
        startVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
      } else if (bytesEqual(entry.key, START_FRONTIERS_KEY)) {
        startFrontiers = decodePostcardFrontiers(entry.value);
      } else if (entry.key.length === 12) {
        const expected = decodeChangeBlockKey(entry.key);
        const block = this.#readChangeBlock(entry.value);
        if (block[0] !== undefined && !idsEqual(block[0].change.id, expected)) {
          throw new Error("snapshot change key does not match its block");
        }
        records.push(...block);
      }
    }
    return {
      mode: parsed.mode,
      records,
      snapshot,
      ...(startVersion === undefined ? {} : { startVersion }),
      ...(startFrontiers === undefined ? {} : { startFrontiers }),
      ...(endVersion === undefined ? {} : { endVersion }),
    };
  }

  #integrateHistory(
    records: readonly HistoryRecord[],
    mergeInterval: bigint | undefined = 0n,
  ): IntegrationResult {
    const added: HistoryRecord[] = [];
    const addedRecordIndices = new Map<HistoryRecord, number>();
    for (const record of records) {
      const key = changeKey(record.change.id);
      const knownEnd = Math.max(
        this.#shallowStartVersion.get(record.change.id.peer) ?? 0,
        this.#historyEndByPeer.get(record.change.id.peer) ?? 0,
      );
      const recordEnd = record.change.id.counter + changeLength(record.change);
      if (recordEnd <= knownEnd) continue;
      const pending = this.#pendingHistory.get(key);
      if (
        pending !== undefined &&
        pending.change.id.counter + changeLength(pending.change) >= recordEnd
      )
        continue;
      this.#pendingHistory.set(key, record);
    }

    let promoted = true;
    while (promoted) {
      promoted = false;
      const version = this.#historyVersion();
      for (const pendingRecord of [...this.#pendingHistory.values()].sort(
        compareHistoryRecords,
      )) {
        let record = pendingRecord;
        let change = record.change;
        let key = changeKey(change.id);
        const knownEnd = version.get(change.id.peer) ?? 0;
        const changeEnd = change.id.counter + changeLength(change);
        if (change.id.counter < knownEnd) {
          this.#pendingHistory.delete(key);
          if (changeEnd <= knownEnd) {
            promoted = true;
            continue;
          }
          record = {
            ...record,
            change: sliceChange(
              change,
              knownEnd - change.id.counter,
              changeLength(change),
            ),
          };
          change = record.change;
          key = changeKey(change.id);
          this.#pendingHistory.set(key, record);
          promoted = true;
        }
        if (change.id.counter !== (version.get(change.id.peer) ?? 0)) continue;
        if (
          change.dependencies.some((dependency) => !this.#historyContainsId(dependency))
        ) {
          continue;
        }

        this.#pendingHistory.delete(key);
        const previous =
          mergeInterval === undefined
            ? undefined
            : this.#mergeablePreviousRecord(change, mergeInterval);
        if (previous === undefined) {
          this.#setHistoryRecord(key, record, record);
          addedRecordIndices.set(record, added.length);
        } else {
          const addedPreviousIndex = addedRecordIndices.get(previous);
          if (addedPreviousIndex !== undefined) {
            added[addedPreviousIndex] = cloneHistoryRecord(previous);
            addedRecordIndices.delete(previous);
          }
          const previousLength = changeLength(previous.change);
          const appended = appendHistoryRecord(previous, record, previousLength);
          this.#appendMergedHistoryRecord(previous, appended, previousLength);
        }
        version.set(change.id.peer, change.id.counter + changeLength(change));
        added.push(record);
        this.#seenCommittedPeers.add(change.id.peer);
        promoted = true;
      }
    }
    return {
      added: added.sort(compareHistoryRecords),
      pending: [...this.#pendingHistory.values()].sort(compareHistoryRecords),
    };
  }

  #historyContainsId(id: CodecId): boolean {
    if (id.counter < (this.#shallowStartVersion.get(id.peer) ?? 0)) return true;
    return id.counter < (this.#historyEndByPeer.get(id.peer) ?? 0);
  }

  #assertImportsNotOutdated(records: readonly HistoryRecord[]): void {
    if (this.#shallowRootStore === undefined) return;
    for (const { change } of records) {
      const knownEnd = this.#shallowStartVersion.get(change.id.peer) ?? 0;
      if (change.id.counter + changeLength(change) <= knownEnd) continue;
      if (change.dependencies.length === 0) {
        throw new Error("cannot import updates that depend on an outdated version");
      }

      const touchesShallowRoot = change.dependencies.some((dependency) =>
        this.#shallowRootFrontiers.some((frontier) => idsEqual(frontier, dependency)),
      );
      if (touchesShallowRoot) continue;

      const dependsOnPrunedHistory = change.dependencies.some(
        (dependency) =>
          dependency.counter < (this.#shallowStartVersion.get(dependency.peer) ?? 0),
      );
      if (dependsOnPrunedHistory) {
        throw new Error("cannot import updates that depend on an outdated version");
      }
    }
  }

  #applyRecords(records: readonly HistoryRecord[], recording?: EventRecording): void {
    for (const record of records) {
      const causalVersion = this.#causalVersionAt(record.change.dependencies);
      for (const operation of record.change.operations) {
        const container = this.#getOrCreateContainer(operation.container);
        const finishEvent =
          recording === undefined
            ? undefined
            : this.#prepareEvent(
                recording,
                container,
                operation,
                record.keys,
                record.change.id,
                causalVersion,
                true,
              );
        this.#applyOperation(
          operation,
          record.keys,
          record.change.id,
          record.change.lamport,
          causalVersion,
          container,
        );
        finishEvent?.();
        causalVersion.set(
          record.change.id.peer,
          Math.max(
            causalVersion.get(record.change.id.peer) ?? 0,
            operation.counter + operation.length,
          ),
        );
      }
    }
  }

  #canTransitionRecords(
    records: readonly HistoryRecord[],
    movableMoveMode: MovableMoveTransitionMode = "anchors",
  ): boolean {
    const replayedMoveContainers = new Set<string>();
    if (movableMoveMode === "replay") {
      for (const { change } of records) {
        for (const operation of change.operations) {
          if (operation.content.type === "movable-list-move") {
            const containerId = this.#containerKey(operation.container);
            replayedMoveContainers.add(containerId);
          }
        }
      }
      for (const containerId of replayedMoveContainers) {
        if (this.#movableOrderHistory.get(containerId) === undefined) return false;
      }
    }
    const moveSuffixes = new Map<
      SequenceElement,
      { readonly history: readonly SequenceMoveMeta[]; readonly indices: Set<number> }
    >();
    for (const { change } of records) {
      const causalVersion = this.#causalVersionAt(change.dependencies);
      for (const operation of change.operations) {
        const container = this.#containers.get(this.#containerKey(operation.container));
        const content = operation.content;
        if (content.type === "movable-list-move") {
          const element =
            container instanceof LoroMovableList
              ? container._sequence.findByLamport(
                  content.elementId.peer,
                  content.elementId.lamport,
                )
              : undefined;
          const history = element?.moveHistory;
          const moveIndex = findSequenceMoveMetaIndex(
            history,
            operationWriter(change, operation),
          );
          if (
            !(container instanceof LoroMovableList) ||
            !container._moveHistoryComplete ||
            element === undefined ||
            (movableMoveMode === "anchors" &&
              (history === undefined ||
                moveIndex < 0 ||
                history[moveIndex]!.id.peer !== change.id.peer ||
                history[moveIndex]!.id.counter !== operation.counter))
          ) {
            return false;
          }
          if (movableMoveMode === "anchors") {
            let suffix = moveSuffixes.get(element);
            if (suffix === undefined) {
              suffix = { history: history!, indices: new Set() };
              moveSuffixes.set(element, suffix);
            }
            suffix.indices.add(moveIndex);
          }
        } else if (content.type === "movable-list-set") {
          const element =
            container instanceof LoroMovableList
              ? container._sequence.findByLamport(
                  content.elementId.peer,
                  content.elementId.lamport,
                )
              : undefined;
          if (
            !(container instanceof LoroMovableList) ||
            !container._valueHistoryComplete ||
            element === undefined ||
            !hasSequenceValueMeta(
              element.valueHistory,
              operationWriter(change, operation),
              change.id.peer,
              operation.counter,
            )
          ) {
            return false;
          }
        } else if (content.type === "text-mark") {
          if (!(container instanceof LoroText) || !container._attributeHistoryComplete) {
            return false;
          }
          const viewLength = container._sequence.isFullyIncluded(causalVersion)
            ? container._sequence.visibleLength
            : container._sequence.causalView(causalVersion).length;
          if (content.end > viewLength) {
            return false;
          }
          const runs = container._styleRuns(content.start, content.end, causalVersion);
          if (
            !container._styleIndex.runsContainMeta(runs, content.key, {
              peer: change.id.peer,
              counter: operation.counter,
            })
          ) {
            return false;
          }
        } else if (content.type === "text-mark-end") {
          if (!(container instanceof LoroText)) return false;
        } else if (content.type === "text-insert") {
          if (!(container instanceof LoroText)) return false;
          if (
            !container._sequence.containsIdRuns([
              {
                start: { peer: change.id.peer, counter: operation.counter },
                length: operation.length,
              },
            ])
          ) {
            return false;
          }
        } else if (
          content.type === "list-insert" ||
          content.type === "movable-list-insert"
        ) {
          if (!(container instanceof LoroList)) return false;
          if (
            !container._sequence.containsIdRuns([
              {
                start: { peer: change.id.peer, counter: operation.counter },
                length: operation.length,
              },
            ])
          ) {
            return false;
          }
        } else if (
          content.type === "text-delete" ||
          content.type === "list-delete" ||
          content.type === "movable-list-delete"
        ) {
          if (!(container instanceof LoroList || container instanceof LoroText)) {
            return false;
          }
        } else if (content.type === "map-insert" || content.type === "map-delete") {
          if (!(container instanceof LoroMap)) return false;
        } else if (
          content.type === "tree-create" ||
          content.type === "tree-move" ||
          content.type === "tree-delete"
        ) {
          if (!(container instanceof LoroTree)) return false;
        } else if (content.type === "future") {
          if (
            !(container instanceof LoroCounter) ||
            counterDelta(content) === undefined
          ) {
            return false;
          }
        }
        causalVersion.set(
          change.id.peer,
          Math.max(
            causalVersion.get(change.id.peer) ?? 0,
            operation.counter + operation.length,
          ),
        );
      }
    }
    for (const { history, indices } of moveSuffixes.values()) {
      const first = Math.min(...indices);
      if (history.length - first !== indices.size) return false;
      for (let index = first; index < history.length; index += 1) {
        if (!indices.has(index)) return false;
      }
    }
    return true;
  }

  #applyVersionTransition(
    retreat: readonly HistoryRecord[],
    forward: readonly HistoryRecord[],
    target: VersionVector,
    recording?: EventRecording,
    movableMoveMode: MovableMoveTransitionMode = "anchors",
  ): void {
    const sequences = new Map<LoroList | LoroText, Set<SequenceElement>>();
    const bulkSequenceRemovals = new Map<LoroList | LoroText, SequenceIdRun[]>();
    const bulkSequenceRestorations = new Map<LoroList | LoroText, SequenceIdRun[]>();
    const textAttributeRuns = new Map<LoroText, Map<string, SequenceIdRun[]>>();
    const textStyleContainers = new Set<LoroText>();
    const movableValues = new Map<LoroMovableList, Set<SequenceElement>>();
    const mapKeys = new Map<LoroMap, Set<string>>();
    const treeSubjects = new Map<LoroTree, Map<string, CodecId>>();
    const counters = new Map<LoroCounter, number>();
    const sequenceElements = (container: LoroList | LoroText): Set<SequenceElement> => {
      let elements = sequences.get(container);
      if (elements === undefined) {
        elements = new Set();
        sequences.set(container, elements);
      }
      return elements;
    };
    const addTextAttributeRuns = (
      container: LoroText,
      key: string,
      runs: readonly SequenceIdRun[],
    ): void => {
      let keys = textAttributeRuns.get(container);
      if (keys === undefined) {
        keys = new Map();
        textAttributeRuns.set(container, keys);
      }
      const existing = keys.get(key);
      if (existing === undefined) keys.set(key, [...runs]);
      else existing.push(...runs);
    };
    const addBulkSequenceRemovals = (
      container: LoroList | LoroText,
      runs: readonly SequenceIdRun[],
    ): void => {
      const existing = bulkSequenceRemovals.get(container);
      if (existing === undefined) bulkSequenceRemovals.set(container, [...runs]);
      else existing.push(...runs);
    };
    const addBulkSequenceRestorations = (
      container: LoroList | LoroText,
      runs: readonly SequenceIdRun[],
    ): void => {
      const existing = bulkSequenceRestorations.get(container);
      if (existing === undefined) bulkSequenceRestorations.set(container, [...runs]);
      else existing.push(...runs);
    };
    const collect = (records: readonly HistoryRecord[], direction: -1 | 1): void => {
      for (const { change } of records) {
        const causalVersion = this.#causalVersionAt(change.dependencies);
        for (const operation of change.operations) {
          const container = this.#containers.get(
            this.#containerKey(operation.container),
          )!;
          const content = operation.content;
          if (
            content.type === "text-insert" ||
            content.type === "list-insert" ||
            content.type === "movable-list-insert"
          ) {
            const sequenceContainer = container as LoroText | LoroList;
            const sequence = sequenceContainer._sequence;
            const insertedRuns = [
              {
                start: { peer: change.id.peer, counter: operation.counter },
                length: operation.length,
              },
            ];
            if (
              direction === -1 ||
              (recording === undefined && sequence.canShowIdRunsAt(insertedRuns, target))
            ) {
              if (direction === -1) {
                addBulkSequenceRemovals(sequenceContainer, insertedRuns);
              } else {
                addBulkSequenceRestorations(sequenceContainer, insertedRuns);
              }
              causalVersion.set(
                change.id.peer,
                Math.max(
                  causalVersion.get(change.id.peer) ?? 0,
                  operation.counter + operation.length,
                ),
              );
              continue;
            }
            const elements = sequenceElements(sequenceContainer);
            for (let offset = 0; offset < operation.length; offset += 1) {
              elements.add(
                sequence.findById({
                  peer: change.id.peer,
                  counter: operation.counter + offset,
                })!,
              );
            }
          } else if (
            content.type === "text-delete" ||
            content.type === "list-delete" ||
            content.type === "movable-list-delete"
          ) {
            const sequenceContainer = container as LoroText | LoroList;
            const deletedRuns = sequenceContainer._sequence.idRunsDeletedBy(
              change.id.peer,
              operation.counter,
              operation.counter + operation.length,
            );
            if (
              direction === 1 ||
              (recording === undefined &&
                sequenceContainer._sequence.canShowIdRunsAt(deletedRuns, target))
            ) {
              if (direction === 1) {
                addBulkSequenceRemovals(sequenceContainer, deletedRuns);
              } else {
                addBulkSequenceRestorations(sequenceContainer, deletedRuns);
              }
              causalVersion.set(
                change.id.peer,
                Math.max(
                  causalVersion.get(change.id.peer) ?? 0,
                  operation.counter + operation.length,
                ),
              );
              continue;
            }
            const elements = sequenceElements(sequenceContainer);
            for (const element of sequenceContainer._sequence.elementsDeletedBy(
              change.id.peer,
              operation.counter,
              operation.counter + operation.length,
            )) {
              elements.add(element);
            }
          } else if (content.type === "text-mark") {
            const text = container as LoroText;
            textStyleContainers.add(text);
            if (recording !== undefined) {
              addTextAttributeRuns(
                text,
                content.key,
                text._styleRuns(content.start, content.end, causalVersion),
              );
            }
          } else if (content.type === "movable-list-set") {
            const list = container as LoroMovableList;
            const element = list._sequence.findByLamport(
              content.elementId.peer,
              content.elementId.lamport,
            )!;
            let elements = movableValues.get(list);
            if (elements === undefined) {
              elements = new Set();
              movableValues.set(list, elements);
            }
            elements.add(element);
          } else if (content.type === "map-insert" || content.type === "map-delete") {
            const map = container as LoroMap;
            let keys = mapKeys.get(map);
            if (keys === undefined) {
              keys = new Set();
              mapKeys.set(map, keys);
            }
            keys.add(content.key);
          } else if (
            content.type === "tree-create" ||
            content.type === "tree-move" ||
            content.type === "tree-delete"
          ) {
            const tree = container as LoroTree;
            let subjects = treeSubjects.get(tree);
            if (subjects === undefined) {
              subjects = new Map();
              treeSubjects.set(tree, subjects);
            }
            subjects.set(idKey(content.subject), content.subject);
          } else if (content.type === "future") {
            const counter = container as LoroCounter;
            counters.set(
              counter,
              (counters.get(counter) ?? 0) + direction * counterDelta(content)!,
            );
          }
          causalVersion.set(
            change.id.peer,
            Math.max(
              causalVersion.get(change.id.peer) ?? 0,
              operation.counter + operation.length,
            ),
          );
        }
      }
    };
    collect(retreat, -1);
    collect(forward, 1);

    if (recording !== undefined) {
      for (const [container, runs] of [...bulkSequenceRemovals]) {
        if (!sequences.has(container)) continue;
        const elements = sequenceElements(container);
        for (const run of runs) {
          for (let offset = 0; offset < run.length; offset += 1) {
            const element = container._sequence.findById({
              peer: run.start.peer,
              counter: run.start.counter + offset,
            });
            if (element !== undefined) elements.add(element as never);
          }
        }
        bulkSequenceRemovals.delete(container);
      }
    }

    const includes = (id: CodecId): boolean => id.counter < (target.get(id.peer) ?? 0);
    const targetDeleted = (
      container: LoroText | LoroList,
      element: SequenceElement,
    ): boolean =>
      !includes(element.id) || container._sequence.someDeletion(element, includes);
    const styleVersion =
      target.compare(this.#historyVersion()) === 0
        ? undefined
        : new Map(
            target
              ._codecEntriesUnsorted()
              .map(({ peer, counter }) => [peer, counter] as const),
          );
    for (const [text, keys] of textAttributeRuns) {
      const state = this.#sequenceEventState(recording!, text, "text");
      const removedRuns = [
        ...(bulkSequenceRemovals.get(text) ?? []),
        ...[...(sequences.get(text) ?? [])]
          .filter((element) => !element.deleted && targetDeleted(text, element))
          .map((element) => ({ start: element.id, length: 1 })),
      ];
      for (const [key, runs] of keys) {
        for (const transition of text._styleIndex.transitions(
          runs,
          key,
          text._styleVersion,
          styleVersion,
        )) {
          const beforePresent =
            transition.before !== undefined && transition.before.value !== null;
          const beforeValue = beforePresent ? transition.before!.value : undefined;
          const afterPresent =
            transition.after !== undefined && transition.after.value !== null;
          const afterValue = afterPresent ? transition.after!.value : undefined;
          if (
            beforePresent === afterPresent &&
            eventValuesEqual(beforeValue, afterValue)
          ) {
            continue;
          }
          const retainedRuns = subtractSequenceIdRuns([transition.run], removedRuns);
          for (const range of text._sequence.visibleMetricRangesForIdRuns(
            retainedRuns,
            "utf16",
          )) {
            state.diff.formatText(
              range.start,
              range.end - range.start,
              key,
              afterPresent ? (runtimeValueToJson(afterValue!) as Value) : null,
            );
          }
        }
      }
    }
    for (const text of textStyleContainers) text._setStyleVersion(styleVersion);

    for (const [container, runs] of bulkSequenceRestorations) {
      container._sequence.setIdRunsVisible(runs);
    }

    for (const [container, elements] of sequences) {
      const state =
        recording === undefined
          ? undefined
          : this.#sequenceEventState(
              recording,
              container,
              container instanceof LoroText ? "text" : "list",
            );
      const removals = [...elements]
        .filter((element) => !element.deleted && targetDeleted(container, element))
        .map((element) => ({
          element,
          position:
            container instanceof LoroText
              ? container._sequence.visibleMetricOffsetOf(
                  element as TextElement,
                  "utf16",
                )!
              : container._sequence.visibleIndexOf(element)!,
        }))
        .sort((left, right) => right.position - left.position);
      for (const { element, position } of removals) {
        state?.diff.delete(
          position,
          container instanceof LoroText ? (element.value as string).length : 1,
        );
        container._sequence.setDeleted(element as never, true);
      }

      const insertions = [...elements]
        .filter((element) => element.deleted && !targetDeleted(container, element))
        .sort(
          (left, right) =>
            container._sequence.physicalIndexOf(left as never)! -
            container._sequence.physicalIndexOf(right as never)!,
        );
      for (const element of insertions) {
        container._sequence.setDeleted(element as never, false);
        if (state === undefined) continue;
        if (container instanceof LoroText) {
          const textElement = element as TextElement;
          const position = container._sequence.visibleMetricOffsetOf(
            textElement,
            "utf16",
          )!;
          const attributes = Object.fromEntries(
            [...container._attributesAt(textElement)].map(([key, value]) => [
              key,
              runtimeValueToJson(value) as Value,
            ]),
          );
          state.diff.insertText(position, textElement.value, attributes);
        } else {
          const position = container._sequence.visibleIndexOf(element)!;
          state.diff.insertList(position, [cloneRuntimeValue(element.value)]);
        }
      }
    }

    for (const [container, runs] of bulkSequenceRemovals) {
      if (recording !== undefined) {
        const state = this.#sequenceEventState(
          recording,
          container,
          container instanceof LoroText ? "text" : "list",
        );
        for (const range of container._sequence
          .visibleMetricRangesForIdRuns(runs, "utf16")
          .reverse()) {
          state.diff.delete(range.start, range.end - range.start);
        }
      }
      container._sequence.setIdRunsDeleted(runs);
    }

    if (movableMoveMode === "replay") {
      this.#replayMovableMoves(retreat, forward, target, recording);
    } else {
      this.#transitionMovableMoves(retreat, true, recording);
      this.#transitionMovableMoves(forward, false, recording);
    }

    for (const [list, elements] of movableValues) {
      const state =
        recording === undefined
          ? undefined
          : this.#sequenceEventState(recording, list, "list");
      for (const element of elements) {
        const winner = latestIncludedSequenceValue(element.valueHistory, target);
        if (winner === undefined || eventValuesEqual(element.value, winner.value)) {
          continue;
        }
        if (!element.deleted) {
          const position = list._sequence.visibleIndexOf(element);
          if (position !== undefined) {
            state?.diff.delete(position, 1);
            state?.diff.insertList(position, [cloneRuntimeValue(winner.value)]);
          }
        }
        element.value = winner.value;
        list._bindChildren([element]);
      }
    }

    for (const [map, keys] of mapKeys) {
      const history = this.#mapOperationHistory.get(map.id);
      for (const key of keys) {
        if (recording !== undefined) {
          const state = this.#mapEventState(recording, map);
          if (!state.originals.has(key)) {
            const record = map._entries.get(key);
            state.originals.set(key, {
              present: record !== undefined,
              visible: record !== undefined && !record.deleted,
              value:
                record === undefined || record.deleted
                  ? undefined
                  : cloneRuntimeValue(record.value),
            });
          }
        }
        const indexed = latestIncludedOperation(history?.get(key), target);
        if (indexed === undefined) {
          map._replaceRecord(key, undefined);
          continue;
        }
        const content = indexed.operation.content;
        if (content.type === "map-delete") {
          map._replaceRecord(key, {
            value: undefined,
            rawValue: undefined,
            deleted: true,
            writer: indexed.writer,
          });
        } else if (content.type === "map-insert") {
          const operationId = {
            peer: indexed.record.change.id.peer,
            counter: indexed.operation.counter,
          };
          const rawValue = this.#decodeRuntimeValue(
            content.value,
            indexed.record.keys,
            operationId,
            map,
          );
          map._replaceRecord(key, {
            value: this.#materializeMapValue(map, key, rawValue),
            rawValue,
            deleted: false,
            writer: indexed.writer,
          });
        }
      }
    }

    for (const [tree, subjects] of treeSubjects) {
      const history = this.#treeOperationHistory.get(tree.id);
      for (const [historyKey, subject] of subjects) {
        const nodeKey = formatTreeId(subject);
        if (recording !== undefined) {
          const state = this.#treeEventState(recording, tree);
          if (!state.originals.has(nodeKey)) {
            const record = tree._nodes.get(nodeKey);
            state.originals.set(
              nodeKey,
              record === undefined || record.deleted
                ? undefined
                : this.#treeEventNode(tree, record),
            );
          }
        }
        const operations = history?.get(historyKey);
        const winner = latestIncludedOperation(operations, target);
        const existing = tree._nodes.get(nodeKey);
        if (winner === undefined) {
          if (existing !== undefined) tree._removeRecord(existing);
          continue;
        }
        const winnerContent = winner.operation.content;
        const placement =
          winnerContent.type === "tree-create" || winnerContent.type === "tree-move"
            ? winner
            : latestIncludedTreePlacement(operations, target, winner.writer);
        if (placement === undefined) continue;
        const placementContent = placement.operation.content;
        if (
          placementContent.type !== "tree-create" &&
          placementContent.type !== "tree-move"
        ) {
          continue;
        }
        let record = tree._nodes.get(nodeKey);
        const lastMoveId = {
          peer: placement.record.change.id.peer,
          counter: placement.operation.counter,
        };
        if (record === undefined) {
          const data = this.#getOrCreateContainer(
            {
              kind: "normal",
              ...placementContent.subject,
              containerType: CodecContainerType.Map,
            },
            tree,
          ) as LoroMap;
          record = {
            id: placementContent.subject,
            parent: placementContent.parent,
            position: placementContent.position.slice(),
            deleted: false,
            writer: placement.writer,
            lastMoveId,
            data,
          };
          tree._setRecord(record);
        } else {
          tree._updateRecord(
            record,
            placementContent.parent,
            placementContent.position.slice(),
            placement.writer,
            lastMoveId,
          );
        }
        if (winnerContent.type === "tree-delete") {
          tree._deleteRecord(record, winner.writer);
        }
      }
    }

    for (const [counter, adjustment] of counters) {
      if (recording !== undefined && !recording.eventStates.has(counter.id)) {
        recording.eventStates.set(counter.id, {
          kind: "counter",
          before: counter.value,
        });
      }
      counter._value += adjustment;
    }
  }

  #transitionMovableMoves(
    records: readonly HistoryRecord[],
    retreat: boolean,
    recording?: EventRecording,
  ): void {
    const operations = records
      .flatMap(({ change }) =>
        change.operations.flatMap((operation) =>
          operation.content.type === "movable-list-move" ? [{ change, operation }] : [],
        ),
      )
      .sort((left, right) =>
        compareHistoryOperations(
          left.change,
          left.operation,
          right.change,
          right.operation,
        ),
      );
    if (retreat) operations.reverse();
    for (const { change, operation } of operations) {
      const content = operation.content;
      if (content.type !== "movable-list-move") continue;
      const container = this.#containers.get(this.#containerKey(operation.container));
      if (!(container instanceof LoroMovableList)) continue;
      const element = container._sequence.findByLamport(
        content.elementId.peer,
        content.elementId.lamport,
      );
      if (element === undefined || element.deleted) continue;
      const meta = findSequenceMoveMeta(
        element.moveHistory,
        operationWriter(change, operation),
      );
      if (meta === undefined) continue;
      const from = container._sequence.visibleIndexOf(element);
      if (from === undefined) continue;
      const value = cloneRuntimeValue(element.value);
      container._moveToAnchors(
        element,
        retreat ? meta.beforePrevious : meta.afterPrevious,
        retreat ? meta.beforeNext : meta.afterNext,
      );
      const to = container._sequence.visibleIndexOf(element);
      if (recording === undefined || to === undefined || to === from) continue;
      const state = this.#sequenceEventState(recording, container, "list");
      state.diff.delete(from, 1);
      state.diff.insertList(to, [value]);
    }
  }

  #canonicalizeImportedMovableMoves(
    records: readonly HistoryRecord[],
    recording?: EventRecording,
  ): void {
    let hasMove = false;
    for (const { change } of records) {
      for (const operation of change.operations) {
        if (operation.content.type !== "movable-list-move") continue;
        hasMove = true;
        const container = this.#containers.get(this.#containerKey(operation.container));
        if (!(container instanceof LoroMovableList) || !container._moveHistoryComplete) {
          return;
        }
      }
    }
    if (hasMove) this.#replayMovableMoves([], records, this.#historyVersion(), recording);
  }

  #replayMovableMoves(
    retreat: readonly HistoryRecord[],
    forward: readonly HistoryRecord[],
    target: VersionVector,
    recording?: EventRecording,
  ): void {
    const containerIds = new Set<string>();
    for (const { change } of [...retreat, ...forward]) {
      for (const operation of change.operations) {
        if (operation.content.type === "movable-list-move") {
          containerIds.add(this.#containerKey(operation.container));
        }
      }
    }

    for (const containerId of containerIds) {
      const container = this.#containers.get(containerId);
      const history = this.#movableOrderHistory.get(containerId);
      if (!(container instanceof LoroMovableList) || history === undefined) continue;
      const replay = new LoroMovableList();
      const ordered = history
        .values()
        .map((indexed) => ({
          indexed,
          orderRecord:
            this.#recordContaining({
              peer: indexed.record.change.id.peer,
              counter: indexed.operation.counter,
            }) ?? indexed.record,
        }))
        .sort(
          (left, right) =>
            compareHistoryRecords(left.orderRecord, right.orderRecord) ||
            left.indexed.operation.counter - right.indexed.operation.counter,
        );
      for (const { indexed } of ordered) {
        const peer = indexed.record.change.id.peer;
        const includedLength = Math.min(
          indexed.operation.length,
          (target.get(peer) ?? 0) - indexed.operation.counter,
        );
        if (includedLength <= 0) continue;
        const operation =
          includedLength === indexed.operation.length
            ? indexed.operation
            : sliceOperation(indexed.operation, 0, includedLength);
        const content = operation.content;
        const operationId = { peer, counter: operation.counter };
        const lamport =
          indexed.record.change.lamport +
          operation.counter -
          indexed.record.change.id.counter;
        const causalVersion = this.#causalVersionAt(indexed.record.change.dependencies);
        causalVersion.set(
          peer,
          Math.max(causalVersion.get(peer) ?? 0, operation.counter),
        );
        if (content.type === "movable-list-insert") {
          replay._insertFugue(
            content.position,
            content.values.map(() => null),
            content.values.map((_, offset) => ({
              peer,
              counter: operation.counter + offset,
            })),
            content.values.map((_, offset) => lamport + offset),
            causalVersion,
          );
        } else if (content.type === "movable-list-delete") {
          replay._deleteIdSpan(content.startId, Number(content.length), operationId);
        } else if (content.type === "movable-list-move") {
          const element = replay._sequence.findByLamport(
            content.elementId.peer,
            content.elementId.lamport,
          );
          const from =
            element === undefined || element.deleted
              ? undefined
              : replay._sequence.visibleIndexOf(element);
          if (from !== undefined) {
            replay._applyMove(from, Math.min(content.to, replay.length - 1), {
              id: operationId,
              lamport,
            });
          }
        }
      }

      const current = container._visibleElements();
      const currentIndex = new Map(
        current.map((element, index) => [element, index] as const),
      );
      const targetElements = replay._visibleElements().map((replayed) => {
        const element = container._sequence.findById(replayed.id);
        if (element === undefined || element.deleted) {
          throw new Error("movable-list replay produced an unavailable target element");
        }
        return element;
      });
      for (const replayed of replay._elements) {
        const element = container._sequence.findById(replayed.id);
        if (element === undefined) continue;
        for (const meta of replayed.moveHistory ?? []) {
          let history = element.moveHistory;
          if (history === undefined) {
            history = [];
            element.moveHistory = history;
          }
          const existing = history.findIndex(
            ({ id }) => id.peer === meta.id.peer && id.counter === meta.id.counter,
          );
          if (existing >= 0) history[existing] = meta;
          else {
            history.push(meta);
            history.sort((left, right) =>
              compareWriter(
                { peer: left.id.peer, lamport: left.lamport },
                { peer: right.id.peer, lamport: right.lamport },
              ),
            );
          }
        }
      }
      if (
        targetElements.length !== current.length ||
        targetElements.some((element) => !currentIndex.has(element))
      ) {
        throw new Error("movable-list replay changed the visible element set");
      }
      const stableTargetIndices = longestIncreasingSubsequenceIndices(
        targetElements.map((element) => currentIndex.get(element)!),
      );
      const state =
        recording === undefined
          ? undefined
          : this.#sequenceEventState(recording, container, "list");
      for (let index = targetElements.length - 1; index >= 0; index -= 1) {
        if (stableTargetIndices.has(index)) continue;
        const element = targetElements[index]!;
        const from = container._sequence.visibleIndexOf(element)!;
        container._sequence.moveBefore(element, targetElements[index + 1]);
        const to = container._sequence.visibleIndexOf(element)!;
        if (state !== undefined && from !== to) {
          state.diff.delete(from, 1);
          state.diff.insertList(to, [cloneRuntimeValue(element.value)]);
        }
      }
    }
  }

  #rebuildFromHistory(version?: VersionVector): void {
    for (const container of this.#containers.values()) container._reset();
    if (this.#shallowRootStore !== undefined) {
      this.#hydrateState(this.#shallowRootStore);
      const target = version ?? this.#historyVersion();
      this.#assertVersionNotBeforeShallowRoot(target);
      this.#applyRecords(this.#recordsInVersionRange(this.#shallowRootVersion, target));
      return;
    }
    this.#applyRecords(
      version === undefined ? this.#sortedHistory() : this.#recordsAtVersion(version),
    );
  }

  #setHistoryRecord(key: string, record: HistoryRecord, appended: HistoryRecord): void {
    const previous = this.#history.get(key);
    this.#history.set(key, record);
    if (previous !== undefined) this.#historyOrder.delete(previous);
    this.#historyOrder.add(record);
    this.#historyOperationCount +=
      changeLength(record.change) -
      (previous === undefined ? 0 : changeLength(previous.change));
    this.#sortedHistoryCache = undefined;

    let peerRecords = this.#historyByPeer.get(record.change.id.peer);
    if (peerRecords === undefined) {
      peerRecords = [];
      this.#historyByPeer.set(record.change.id.peer, peerRecords);
    }
    if (previous !== undefined) {
      const previousIndex = lowerBoundHistory(peerRecords, previous.change.id.counter);
      if (peerRecords[previousIndex] === previous) peerRecords.splice(previousIndex, 1);
    }
    const index = lowerBoundHistory(peerRecords, record.change.id.counter);
    peerRecords.splice(index, 0, record);
    const last = peerRecords.at(-1);
    if (last === undefined) {
      this.#historyEndByPeer.delete(record.change.id.peer);
    } else {
      this.#historyEndByPeer.set(
        record.change.id.peer,
        last.change.id.counter + changeLength(last.change),
      );
    }

    if (previous !== undefined) {
      const previousLast = {
        peer: previous.change.id.peer,
        counter: previous.change.id.counter + changeLength(previous.change) - 1,
      };
      this.#historyFrontiers.delete(idKey(previousLast));
    }
    for (const dependency of appended.change.dependencies) {
      this.#historyFrontiers.delete(idKey(dependency));
    }
    const lastId = {
      peer: record.change.id.peer,
      counter: record.change.id.counter + changeLength(record.change) - 1,
    };
    this.#historyFrontiers.set(idKey(lastId), lastId);
    this.#dependencyVersion(record.change);
    this.#indexHistoryOperations(appended);
  }

  #appendMergedHistoryRecord(
    record: HistoryRecord,
    appended: HistoryRecord,
    previousLength: number,
  ): void {
    const appendedLength = changeLength(appended.change);
    this.#historyOperationCount += appendedLength;
    this.#historyEndByPeer.set(
      record.change.id.peer,
      record.change.id.counter + previousLength + appendedLength,
    );

    const previousLast = {
      peer: record.change.id.peer,
      counter: record.change.id.counter + previousLength - 1,
    };
    this.#historyFrontiers.delete(idKey(previousLast));
    for (const dependency of appended.change.dependencies) {
      this.#historyFrontiers.delete(idKey(dependency));
    }
    const lastId = {
      peer: record.change.id.peer,
      counter: record.change.id.counter + previousLength + appendedLength - 1,
    };
    this.#historyFrontiers.set(idKey(lastId), lastId);
    this.#indexHistoryOperations(appended);
  }

  #indexHistoryOperations(record: HistoryRecord): void {
    for (const operation of record.change.operations) {
      this.#containersWithOperations.add(this.#containerKey(operation.container));
      const content = operation.content;
      const indexed = {
        record,
        operation,
        writer: operationWriter(record.change, operation),
      };
      if (
        content.type === "movable-list-insert" ||
        content.type === "movable-list-delete" ||
        content.type === "movable-list-move"
      ) {
        const container = this.#containerKey(operation.container);
        let history = this.#movableOrderHistory.get(container);
        if (history === undefined) {
          history = new OrderedIndex((left, right) =>
            compareWriter(left.writer, right.writer),
          );
          this.#movableOrderHistory.set(container, history);
        }
        history.add(indexed);
        if (content.type === "movable-list-move") {
          let peers = this.#movableMovePeers.get(container);
          if (peers === undefined) {
            peers = new Set();
            this.#movableMovePeers.set(container, peers);
          }
          peers.add(record.change.id.peer);
        }
      }
      let bySubject: Map<string, IndexedSubjectHistory> | undefined;
      let subject: string | undefined;
      let treeOperation = false;
      if (content.type === "map-insert" || content.type === "map-delete") {
        const container = this.#containerKey(operation.container);
        bySubject = this.#mapOperationHistory.get(container);
        if (bySubject === undefined) {
          bySubject = new Map();
          this.#mapOperationHistory.set(container, bySubject);
        }
        subject = content.key;
      } else if (
        content.type === "tree-create" ||
        content.type === "tree-move" ||
        content.type === "tree-delete"
      ) {
        const container = this.#containerKey(operation.container);
        bySubject = this.#treeOperationHistory.get(container);
        if (bySubject === undefined) {
          bySubject = new Map();
          this.#treeOperationHistory.set(container, bySubject);
        }
        subject = idKey(content.subject);
        treeOperation = true;
      }
      if (bySubject === undefined || subject === undefined) continue;
      let history = bySubject.get(subject);
      if (history === undefined) {
        history = {
          byWriter: new OrderedIndex((left, right) =>
            compareWriter(left.writer, right.writer),
          ),
          byPeer: new Map(),
          ...(treeOperation ? { placementsByPeer: new Map() } : {}),
        };
        bySubject.set(subject, history);
      }
      history.byWriter.add(indexed);
      let peerOperations = history.byPeer.get(record.change.id.peer);
      if (peerOperations === undefined) {
        peerOperations = [];
        history.byPeer.set(record.change.id.peer, peerOperations);
      }
      const peerIndex = lowerBoundIndexedOperation(peerOperations, operation.counter);
      peerOperations.splice(peerIndex, 0, indexed);
      if (content.type === "tree-create" || content.type === "tree-move") {
        let placements = history.placementsByPeer!.get(record.change.id.peer);
        if (placements === undefined) {
          placements = [];
          history.placementsByPeer!.set(record.change.id.peer, placements);
        }
        const placementIndex = lowerBoundIndexedOperation(placements, operation.counter);
        placements.splice(placementIndex, 0, indexed);
      }
    }
  }

  #validateDeferredFrontierBlocks(
    entries: readonly SstableEntry[],
    endVersion: VersionVector,
    frontiers: readonly CodecId[],
  ): ReadonlyMap<SstableEntry, readonly HistoryRecord[]> {
    const changeEntriesByPeer = new Map<
      bigint,
      { readonly entry: SstableEntry; readonly start: CodecId }[]
    >();
    for (const entry of entries) {
      if (entry.key.length !== 12) continue;
      const start = decodeChangeBlockKey(entry.key);
      const peerEntries = changeEntriesByPeer.get(start.peer);
      if (peerEntries === undefined) {
        changeEntriesByPeer.set(start.peer, [{ entry, start }]);
      } else {
        peerEntries.push({ entry, start });
      }
    }
    for (const peerEntries of changeEntriesByPeer.values()) {
      peerEntries.sort((left, right) => left.start.counter - right.start.counter);
    }
    const validated = new Map<SstableEntry, readonly HistoryRecord[]>();
    const seenPeers = new Set<bigint>();
    for (const frontier of frontiers) {
      const peerEnd = endVersion.get(frontier.peer) ?? 0;
      if (frontier.counter >= peerEnd) {
        throw new Error("snapshot frontier exceeds its end version");
      }
      if (seenPeers.has(frontier.peer)) {
        throw new Error("snapshot contains multiple frontiers for one peer");
      }
      seenPeers.add(frontier.peer);

      const peerEntries = changeEntriesByPeer.get(frontier.peer) ?? [];
      let low = 0;
      let high = peerEntries.length;
      while (low < high) {
        const middle = (low + high) >>> 1;
        if (peerEntries[middle]!.start.counter <= frontier.counter) low = middle + 1;
        else high = middle;
      }
      const candidate = peerEntries[low - 1];
      if (candidate === undefined) {
        throw new Error("snapshot frontier change block is missing");
      }
      let block = validated.get(candidate.entry);
      if (block === undefined) {
        block = this.#readChangeBlock(candidate.entry.value);
        if (block[0] === undefined || !idsEqual(block[0].change.id, candidate.start)) {
          throw new Error("snapshot change key does not match its block");
        }
        validated.set(candidate.entry, block);
      }
      if (
        !block.some(
          ({ change }) =>
            change.id.peer === frontier.peer &&
            change.id.counter <= frontier.counter &&
            frontier.counter < change.id.counter + changeLength(change),
        )
      ) {
        throw new Error("snapshot frontier is not covered by its change block");
      }
    }
    return validated;
  }

  #materializeDeferredHistory(): void {
    const deferred = this.#deferredSnapshotHistory;
    if (deferred === undefined) return;

    const records: HistoryRecord[] = [];
    for (const entry of deferred.entries) {
      if (entry.key.length !== 12) continue;
      const expected = decodeChangeBlockKey(entry.key);
      const decoded =
        deferred.validatedBlocks.get(entry) ?? this.#readChangeBlock(entry.value);
      if (decoded[0] !== undefined && !idsEqual(decoded[0].change.id, expected)) {
        throw new Error("snapshot change key does not match its block");
      }
      records.push(...decoded.map(cloneHistoryRecord));
    }

    // Validate and build every history index on an isolated document. A malformed
    // deferred block must not leave this document with half-installed history.
    const staged = new LoroDoc();
    staged.#shallowStartVersion = this.#shallowStartVersion.clone();
    const integration = staged.#integrateHistory(records);
    if (integration.pending.length > 0) {
      throw new Error("snapshot history contains changes with missing dependencies");
    }
    if (staged.#historyVersion().compare(deferred.endVersion) !== 0) {
      throw new Error("snapshot version does not match its history");
    }
    const actualFrontiers = [...staged.#historyFrontiers.values()]
      .sort(compareIds)
      .map(formatOpId);
    const expectedFrontiers = [...deferred.frontiers].sort(compareIds).map(formatOpId);
    if (!frontierSetsEqual(actualFrontiers, expectedFrontiers)) {
      throw new Error("snapshot frontiers do not match its history");
    }
    if (staged.#historyOperationCount !== deferred.operationCount) {
      throw new Error("snapshot operation count does not match its history");
    }

    // State was already hydrated from the latest-state SSTable. Installing these
    // structures restores only history/DAG indexes; applying records would
    // duplicate counters and sequence content.
    this.#history = staged.#history;
    this.#historyOrder = staged.#historyOrder;
    this.#historyByPeer = staged.#historyByPeer;
    this.#historyEndByPeer = staged.#historyEndByPeer;
    this.#historyOperationCount = staged.#historyOperationCount;
    this.#sortedHistoryCache = staged.#sortedHistoryCache;
    this.#historyFrontiers = staged.#historyFrontiers;
    this.#dependencyVersionCache = staged.#dependencyVersionCache;
    this.#mapOperationHistory = staged.#mapOperationHistory;
    this.#treeOperationHistory = staged.#treeOperationHistory;
    this.#movableOrderHistory = staged.#movableOrderHistory;
    this.#movableMovePeers = staged.#movableMovePeers;
    this.#containersWithOperations = staged.#containersWithOperations;
    this.#containerKeys = staged.#containerKeys;
    this.#pendingHistory = staged.#pendingHistory;
    this.#deferredSnapshotHistory = undefined;
  }

  #sortedHistory(): HistoryRecord[] {
    this.#materializeDeferredHistory();
    return (this.#sortedHistoryCache ??= this.#historyOrder.values());
  }

  #recordsAtVersion(version: VersionVector): HistoryRecord[] {
    return this.#recordsInVersionRange(new VersionVector(), version);
  }

  #recordsInVersionRange(from: VersionVector, to: VersionVector): HistoryRecord[] {
    this.#materializeDeferredHistory();
    const selected: HistoryRecord[] = [];
    for (const { peer, counter: toCounter } of to._codecEntriesUnsorted()) {
      const fromCounter = from.get(peer) ?? 0;
      if (fromCounter >= toCounter) continue;
      const records = this.#historyByPeer.get(peer) ?? [];
      let index = Math.max(0, lowerBoundHistory(records, fromCounter + 1) - 1);
      for (; index < records.length; index += 1) {
        const record = records[index]!;
        const recordStart = record.change.id.counter;
        if (recordStart >= toCounter) break;
        const length = changeLength(record.change);
        const start = Math.max(0, fromCounter - recordStart);
        const end = Math.min(length, toCounter - recordStart);
        if (start >= end) continue;
        selected.push(
          start === 0 && end === length
            ? record
            : { ...record, change: sliceChange(record.change, start, end) },
        );
      }
    }
    return selected.sort(compareHistoryRecords);
  }

  #recordsInSpans(
    spans: readonly { readonly id: OpId; readonly len: number }[],
  ): HistoryRecord[] {
    this.#materializeDeferredHistory();
    const spansByPeer = new Map<bigint, { start: number; end: number }[]>();
    for (const { id, len } of spans) {
      if (!Number.isSafeInteger(len) || len < 0) {
        throw new RangeError(`update span length is out of range: ${len}`);
      }
      const parsed = parseOpId(id);
      if (len === 0) continue;
      let peerSpans = spansByPeer.get(parsed.peer);
      if (peerSpans === undefined) {
        peerSpans = [];
        spansByPeer.set(parsed.peer, peerSpans);
      }
      peerSpans.push({ start: parsed.counter, end: parsed.counter + len });
    }

    const selected: HistoryRecord[] = [];
    for (const [peer, ranges] of spansByPeer) {
      ranges.sort((left, right) => left.start - right.start);
      const merged: { start: number; end: number }[] = [];
      for (const range of ranges) {
        const previous = merged[merged.length - 1];
        if (previous !== undefined && range.start <= previous.end) {
          previous.end = Math.max(previous.end, range.end);
        } else {
          merged.push({ ...range });
        }
      }
      const records = this.#historyByPeer.get(peer) ?? [];
      for (const range of merged) {
        let index = Math.max(0, lowerBoundHistory(records, range.start + 1) - 1);
        for (; index < records.length; index += 1) {
          const record = records[index]!;
          const changeStart = record.change.id.counter;
          if (changeStart >= range.end) break;
          const length = changeLength(record.change);
          const start = Math.max(0, range.start - changeStart);
          const end = Math.min(length, range.end - changeStart);
          if (start >= end) continue;
          selected.push(
            start === 0 && end === length
              ? record
              : { ...record, change: sliceChange(record.change, start, end) },
          );
        }
      }

      const preCommit = this.#preCommitRecord;
      if (preCommit?.change.id.peer === peer) {
        const changeStart = preCommit.change.id.counter;
        const length = changeLength(preCommit.change);
        const changeEnd = changeStart + length;
        for (const range of merged) {
          const start = Math.max(changeStart, range.start);
          const end = Math.min(changeEnd, range.end);
          if (start >= end) continue;
          selected.push(
            start === changeStart && end === changeEnd
              ? preCommit
              : {
                  ...preCommit,
                  change: sliceChange(
                    preCommit.change,
                    start - changeStart,
                    end - changeStart,
                  ),
                },
          );
        }
      }
    }
    return selected.sort(compareHistoryRecords);
  }

  #assertVersionNotBeforeShallowRoot(version: VersionVector): void {
    if (!versionIncludes(version, this.#shallowRootVersion)) {
      throw new RangeError("cannot use a version before the shallow history root");
    }
  }

  #causalVersionAt(frontiers: readonly CodecId[]): Map<bigint, number> {
    const version = new Map(
      this.#shallowStartVersion
        ._codecEntriesUnsorted()
        .map(({ peer, counter }) => [peer, counter] as const),
    );
    for (const id of frontiers) {
      const record = this.#recordContaining(id);
      if (record !== undefined) {
        for (const [peer, counter] of this.#dependencyVersion(record.change)) {
          version.set(peer, Math.max(version.get(peer) ?? 0, counter));
        }
      }
      version.set(id.peer, Math.max(version.get(id.peer) ?? 0, id.counter + 1));
    }
    return version;
  }

  #dependencyVersion(change: DecodedChange): ReadonlyMap<bigint, number> {
    const cached = this.#dependencyVersionCache.get(change);
    if (cached !== undefined) return cached;
    const version = new Map(
      this.#shallowStartVersion
        ._codecEntriesUnsorted()
        .map(({ peer, counter }) => [peer, counter] as const),
    );
    for (const dependency of change.dependencies) {
      const dependencyRecord = this.#recordContaining(dependency);
      if (dependencyRecord !== undefined && dependencyRecord.change !== change) {
        for (const [peer, counter] of this.#dependencyVersion(dependencyRecord.change)) {
          version.set(peer, Math.max(version.get(peer) ?? 0, counter));
        }
      }
      version.set(
        dependency.peer,
        Math.max(version.get(dependency.peer) ?? 0, dependency.counter + 1),
      );
    }
    this.#dependencyVersionCache.set(change, version);
    return version;
  }

  #frontiersForVersion(version: VersionVector): CodecId[] {
    const candidates = new Map<string, CodecId>();
    const candidateByPeer = new Map<bigint, CodecId>();
    const known = this.#historyVersion();
    for (const { peer, counter } of version._codecEntriesUnsorted()) {
      const end = Math.min(counter, known.get(peer) ?? 0);
      if (end === 0) continue;
      const last = { peer, counter: end - 1 };
      if (this.#recordContaining(last) !== undefined) {
        candidates.set(idKey(last), last);
        candidateByPeer.set(peer, last);
      }
    }
    for (const frontier of [...candidates.values()]) {
      const causalVersion = this.#causalVersionAt([frontier]);
      for (const [peer, counter] of causalVersion) {
        const candidate = candidateByPeer.get(peer);
        if (
          candidate !== undefined &&
          !idsEqual(candidate, frontier) &&
          candidate.counter < counter
        ) {
          candidates.delete(idKey(candidate));
        }
      }
    }
    return [...candidates.values()].sort(compareIds);
  }

  #frontiersCodec(): CodecId[] {
    const candidates = new Map<string, CodecId>();
    if (this.#checkoutVersion !== undefined) {
      for (const frontier of this.#frontiersForVersion(this.#checkoutVersion)) {
        candidates.set(idKey(frontier), frontier);
      }
    } else if (this.#deferredSnapshotHistory !== undefined) {
      for (const frontier of this.#deferredSnapshotHistory.frontiers) {
        candidates.set(idKey(frontier), frontier);
      }
    } else {
      for (const [key, frontier] of this.#historyFrontiers) {
        candidates.set(key, frontier);
      }
    }
    if (this.#pending !== undefined && this.#pending.operations.length > 0) {
      for (const dependency of this.#pending.dependencies)
        candidates.delete(idKey(dependency));
      const last = {
        peer: this.#peer,
        counter: this.#pending.id.counter + this.#pending.operationLength - 1,
      };
      candidates.set(idKey(last), last);
    }
    return [...candidates.values()].sort(compareIds);
  }

  #lamportAt(id: CodecId): number {
    const record = this.#recordContaining(id);
    return record === undefined
      ? 0
      : record.change.lamport + (id.counter - record.change.id.counter);
  }

  #encodeUpdates(records: readonly HistoryRecord[]): Uint8Array {
    const blocks = records.map((record) =>
      encodeChangeBlock({
        peers: [record.change.id.peer],
        keys: record.keys,
        containers: [],
        positions: [],
        changes: [record.change],
      }),
    );
    return encodeDocument(EncodeMode.FastUpdates, encodeFastUpdatesBody(blocks));
  }

  #encodeSnapshot(): Uint8Array {
    if (this.isShallow()) {
      return this.#encodeShallowSnapshot(this.shallowSinceFrontiers());
    }
    const historyEntries = this.#sortedHistory().map((record) => ({
      key: encodeChangeBlockKey(record.change.id),
      value: encodeChangeBlock({
        peers: [record.change.id.peer],
        keys: record.keys,
        containers: [],
        positions: [],
        changes: [record.change],
      }),
    }));
    historyEntries.push({
      key: VERSION_KEY,
      value: encodePostcardVersionVector(this.version().codecEntries()),
    });
    historyEntries.push({
      key: FRONTIERS_KEY,
      value: encodePostcardFrontiers(this.#frontiersCodec()),
    });
    const body = encodeFastSnapshotBody({
      oplog: encodeSstable(historyEntries, { compression: "auto" }),
      state: encodeStateSnapshotStore(this.#buildStateStore(), { compression: "auto" }),
      shallowRootState: new Uint8Array(),
    });
    return encodeDocument(EncodeMode.FastSnapshot, body);
  }

  #encodeShallowSnapshot(requestedFrontiers: Frontiers): Uint8Array {
    const startFrontiers = this.#calculateShallowStart(requestedFrontiers);
    const rootVersion = this.#causalVersionForKnownFrontiers(startFrontiers);
    const startVersion = rootVersion.clone();
    for (const frontier of startFrontiers) {
      startVersion.set(frontier.peer, frontier.counter);
    }

    const latestVersion = this.#historyVersion();
    const latestFrontiers = this.#frontiersForVersion(latestVersion);
    const restoreVersion = this.version();

    this.#rebuildFromHistory(rootVersion);
    const rootStore = this.#buildStateStore(startFrontiers);
    this.#rebuildFromHistory(latestVersion);
    const latestStore = this.#buildStateStore();
    if (restoreVersion.compare(latestVersion) !== 0) {
      this.#rebuildFromHistory(restoreVersion);
    }

    const historyEntries = this.#recordsInVersionRange(startVersion, latestVersion).map(
      (record) => ({
        key: encodeChangeBlockKey(record.change.id),
        value: encodeChangeBlock({
          peers: [record.change.id.peer],
          keys: record.keys,
          containers: [],
          positions: [],
          changes: [record.change],
        }),
      }),
    );
    historyEntries.push(
      {
        key: VERSION_KEY,
        value: encodePostcardVersionVector(latestVersion.codecEntries()),
      },
      {
        key: FRONTIERS_KEY,
        value: encodePostcardFrontiers(latestFrontiers),
      },
      {
        key: START_VERSION_KEY,
        value: encodePostcardVersionVector(startVersion.codecEntries()),
      },
      {
        key: START_FRONTIERS_KEY,
        value: encodePostcardFrontiers(startFrontiers),
      },
    );

    return encodeDocument(
      EncodeMode.FastSnapshot,
      encodeFastSnapshotBody({
        oplog: encodeSstable(historyEntries, { compression: "auto" }),
        state: encodeStateSnapshotStore(latestStore, { compression: "auto" }),
        shallowRootState: encodeStateSnapshotStore(rootStore, {
          compression: "auto",
        }),
      }),
    );
  }

  #calculateShallowStart(requested: Frontiers): CodecId[] {
    const parsed = requested.map(parseOpId);
    for (const frontier of requested) {
      if (this.#recordContaining(parseOpId(frontier)) === undefined) {
        throw new RangeError(
          `shallow snapshot frontier ${frontier.counter}@${frontier.peer} is unknown`,
        );
      }
    }

    let start = parsed;
    if (start.length > 1) {
      const versions = start.map((frontier) => this.#causalVersionAt([frontier]));
      const peers = new Set(versions.flatMap((version) => [...version.keys()]));
      const common = new VersionVector();
      for (const peer of peers) {
        const counter = Math.min(...versions.map((version) => version.get(peer) ?? 0));
        if (counter > 0) common.set(peer, counter);
      }
      const commonFrontiers = this.#frontiersForVersion(common);
      start = commonFrontiers.length === 1 ? commonFrontiers : [];
    }

    if (start.length === 1) {
      const operation = this.#operationAt(start[0]!);
      if (operation?.content.type === "text-mark") {
        start = [{ peer: start[0]!.peer, counter: start[0]!.counter + 1 }];
      }
    }

    const candidateVersion = this.#causalVersionForKnownFrontiers(start);
    if (
      this.isShallow() &&
      !versionIncludes(candidateVersion, this.#shallowRootVersion)
    ) {
      return this.#shallowRootFrontiers.map((id) => ({ ...id }));
    }
    return start;
  }

  #causalVersionForKnownFrontiers(frontiers: readonly CodecId[]): VersionVector {
    const version = new VersionVector();
    for (const [peer, counter] of this.#causalVersionAt(frontiers)) {
      version.set(peer, counter);
    }
    return version;
  }

  #operationAt(id: CodecId): DecodedOperation | undefined {
    const record = this.#recordContaining(id);
    if (record === undefined) return undefined;
    let low = 0;
    let high = record.change.operations.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (record.change.operations[middle]!.counter <= id.counter) low = middle + 1;
      else high = middle;
    }
    const operation = record.change.operations[low - 1];
    return operation !== undefined && id.counter < operation.counter + operation.length
      ? operation
      : undefined;
  }

  #buildStateStore(frontiers?: readonly CodecId[]): StateSnapshotStore {
    const containers: StateSnapshotContainerEntry[] = [];
    for (const container of this.#containers.values()) {
      const id = container._codecId;
      if (id === undefined) continue;
      const parent = container.parent()?._codecId;
      containers.push({
        id,
        wrapper: {
          containerType: id.containerType,
          depth: BigInt(containerDepth(container)),
          parent,
          state: this.#containerState(container),
        },
      });
    }
    return containers.length === 0 && frontiers === undefined
      ? { kind: "empty" }
      : { kind: "sstable", frontiers, containers };
  }

  #containerState(container: LoroContainer): ContainerStateSnapshot {
    if (container instanceof LoroMap) {
      const peers: bigint[] = [];
      const peerIndices = new Map<bigint, number>();
      const peerIndex = (peer: bigint): bigint => {
        let index = peerIndices.get(peer);
        if (index === undefined) {
          index = peers.length;
          peers.push(peer);
          peerIndices.set(peer, index);
        }
        return BigInt(index);
      };
      const values: [string, EncodedLoroValue][] = [];
      const deletedKeys: string[] = [];
      const metadata: MapStateMetadata[] = [];
      for (const [key, record] of container._entries) {
        if (record.deleted) deletedKeys.push(key);
        else
          values.push([key, this.#encodeSnapshotValue(record.rawValue ?? record.value!)]);
        metadata.push({
          key,
          peerIndex: peerIndex(record.writer.peer),
          lamport: BigInt(record.writer.lamport),
        });
      }
      return { kind: CodecContainerType.Map, values, deletedKeys, peers, metadata };
    }
    if (container instanceof LoroText) {
      const peers: bigint[] = [];
      const peerIndices = new Map<bigint, number>();
      const peerIndex = (peer: bigint): bigint => {
        let index = peerIndices.get(peer);
        if (index === undefined) {
          index = peers.length;
          peers.push(peer);
          peerIndices.set(peer, index);
        }
        return BigInt(index);
      };
      const visible = container._visibleElements();
      const keys: string[] = [];
      const keyIndices = new Map<string, number>();
      const keyIndex = (key: string): number => {
        let index = keyIndices.get(key);
        if (index === undefined) {
          index = keys.length;
          keys.push(key);
          keyIndices.set(key, index);
        }
        return index;
      };
      const spans: {
        peerIndex: bigint;
        counter: number;
        lamportSub: number;
        length: number;
      }[] = [];
      const marks: {
        keyIndex: number;
        value: EncodedLoroValue;
        info: number;
      }[] = [];
      const metasAt = container._attributeMetasResolver();
      let active = new Map<string, TextStyleMeta>();
      for (let index = 0; index <= visible.length; index += 1) {
        const current = index < visible.length ? metasAt(visible[index]!) : new Map();
        for (const [key, meta] of active) {
          if (current.get(key) === meta) continue;
          spans.push({
            peerIndex: peerIndex(meta.startId.peer),
            counter: meta.startId.counter + 1,
            lamportSub: meta.lamport - meta.startId.counter,
            length: -1,
          });
        }
        for (const [key, meta] of current) {
          if (active.get(key) === meta) continue;
          spans.push({
            peerIndex: peerIndex(meta.startId.peer),
            counter: meta.startId.counter,
            lamportSub: meta.lamport - meta.startId.counter,
            length: 0,
          });
          marks.push({
            keyIndex: keyIndex(key),
            value: this.#encodeSnapshotValue(meta.value),
            info: meta.info,
          });
        }
        active = new Map(current);
        if (index < visible.length) {
          const element = visible[index]!;
          spans.push({
            peerIndex: peerIndex(element.id.peer),
            counter: element.id.counter,
            lamportSub: element.lamport - element.id.counter,
            length: 1,
          });
        }
      }
      return {
        kind: CodecContainerType.Text,
        text: visible.map((element) => element.value).join(""),
        peers,
        spans,
        keys,
        marks,
      };
    }
    if (container instanceof LoroTree) {
      const records = [...container._nodes.values()].sort(
        (left, right) => Number(left.deleted) - Number(right.deleted),
      );
      const peers: bigint[] = [];
      const peerIndices = new Map<bigint, number>();
      const peerIndex = (peer: bigint): bigint => {
        let index = peerIndices.get(peer);
        if (index === undefined) {
          index = peers.length;
          peers.push(peer);
          peerIndices.set(peer, index);
        }
        return BigInt(index);
      };
      const positions: Uint8Array[] = [];
      const positionIndices = new Map<string, number>();
      const recordIndices = new Map(
        records.map((record, index) => [idKey(record.id), index] as const),
      );
      return {
        kind: CodecContainerType.Tree,
        peers,
        nodes: records.map((record) => {
          const positionKey = bytesToHex(record.position);
          let positionIndex = positionIndices.get(positionKey);
          if (positionIndex === undefined) {
            positionIndex = positions.length;
            positions.push(record.position.slice());
            positionIndices.set(positionKey, positionIndex);
          }
          const parentIndex =
            record.parent === undefined
              ? 0n
              : record.deleted
                ? 1n
                : BigInt((recordIndices.get(idKey(record.parent)) ?? -2) + 2);
          return {
            peerIndex: peerIndex(record.id.peer),
            counter: record.id.counter,
            parentIndexPlusTwo: parentIndex,
            lastSetPeerIndex: peerIndex(record.writer.peer),
            lastSetCounter: record.id.counter,
            lastSetLamportSub: record.writer.lamport - record.id.counter,
            fractionalIndexIndex: positionIndex,
          };
        }),
        positions,
        reserved: new Uint8Array(),
      };
    }
    if (container instanceof LoroCounter) {
      const bytes = new Uint8Array(8);
      new DataView(bytes.buffer).setFloat64(0, container.value, true);
      let bits = 0n;
      for (let index = 7; index >= 0; index -= 1)
        bits = (bits << 8n) | BigInt(bytes[index]!);
      return { kind: CodecContainerType.Counter, bits };
    }
    if (!(container instanceof LoroList)) {
      throw new TypeError(`unsupported container kind ${container.kind()}`);
    }
    const visible = container._visibleElements();
    const peers: bigint[] = [];
    const peerIndices = new Map<bigint, number>();
    const peerIndex = (peer: bigint): bigint => {
      let index = peerIndices.get(peer);
      if (index === undefined) {
        index = peers.length;
        peers.push(peer);
        peerIndices.set(peer, index);
      }
      return BigInt(index);
    };
    if (container instanceof LoroMovableList) {
      return {
        kind: CodecContainerType.MovableList,
        values: visible.map((element) => this.#encodeSnapshotValue(element.value)),
        peers,
        items: [
          {
            invisibleListItems: 0n,
            positionIdEqualsElementId: true,
            elementIdEqualsLastSetId: true,
          },
          ...visible.map(() => ({
            invisibleListItems: 0n,
            positionIdEqualsElementId: true,
            elementIdEqualsLastSetId: true,
          })),
        ],
        listItemIds: visible.map((element) => ({
          peerIndex: peerIndex(element.id.peer),
          counter: element.id.counter,
          lamportSub: element.lamport - element.id.counter,
        })),
        elementIds: [],
        lastSetIds: [],
      };
    }
    return {
      kind: CodecContainerType.List,
      values: visible.map((element) => this.#encodeSnapshotValue(element.value)),
      peers,
      ids: visible.map((element) => ({
        peerIndex: peerIndex(element.id.peer),
        counter: element.id.counter,
        lamportSub: element.lamport - element.id.counter,
      })),
    };
  }

  #encodeSnapshotValue(value: RuntimeValue): EncodedLoroValue {
    if (isContainer(value)) {
      if (value._codecId === undefined)
        throw new Error("detached child in attached state");
      return { type: "container", value: value._codecId };
    }
    if (value === null) return { type: "null" };
    if (typeof value === "boolean") return { type: "bool", value };
    if (typeof value === "number") {
      return Number.isSafeInteger(value)
        ? { type: "i64", value: BigInt(value) }
        : { type: "double", value };
    }
    if (typeof value === "string") return { type: "string", value };
    if (value instanceof Uint8Array) return { type: "binary", value: value.slice() };
    if (Array.isArray(value))
      return {
        type: "list",
        value: value.map((item) => this.#encodeSnapshotValue(item)),
      };
    return {
      type: "map",
      value: Object.entries(value).map(([key, item]) => [
        key,
        this.#encodeSnapshotValue(item),
      ]),
    };
  }

  #hydrateState(store: StateSnapshotStore): void {
    if (store.kind !== "sstable") return;
    for (const { id } of store.containers) this.#getOrCreateContainer(id);
    for (const { id, wrapper } of store.containers) {
      const container = this.#getOrCreateContainer(id);
      const parent =
        wrapper.parent === undefined
          ? undefined
          : this.#getOrCreateContainer(wrapper.parent);
      container._attach(this, id, parent);
      container._reset();
    }
    for (const { id, wrapper } of store.containers) {
      const container = this.#getOrCreateContainer(id);
      const state = wrapper.state;
      if (container instanceof LoroMap && state.kind === CodecContainerType.Map) {
        const metadata = new Map(state.metadata.map((item) => [item.key, item]));
        for (const [key, value] of state.values) {
          const item = metadata.get(key)!;
          const rawValue = this.#decodeSnapshotValue(value, container);
          container._applyValue(
            key,
            this.#materializeMapValue(container, key, rawValue),
            {
              peer: state.peers[Number(item.peerIndex)]!,
              lamport: Number(item.lamport),
            },
            rawValue,
          );
        }
        for (const key of state.deletedKeys) {
          const item = metadata.get(key)!;
          container._applyDelete(key, {
            peer: state.peers[Number(item.peerIndex)]!,
            lamport: Number(item.lamport),
          });
        }
      } else if (
        container instanceof LoroText &&
        state.kind === CodecContainerType.Text
      ) {
        const characters = Array.from(state.text);
        const ids: CodecId[] = [];
        const lamports: number[] = [];
        const styleRuns: {
          readonly run: { readonly start: CodecId; readonly length: number };
          readonly key: string;
          readonly meta: TextStyleMeta;
        }[] = [];
        const stylesById = new Map<string, { key: string; meta: TextStyleMeta }>();
        const active = new Map<string, TextStyleMeta[]>();
        let characterIndex = 0;
        let markIndex = 0;
        for (const span of state.spans) {
          const peer = state.peers[Number(span.peerIndex)]!;
          if (span.length === 0) {
            const mark = state.marks[markIndex++]!;
            const key = state.keys[mark.keyIndex]!;
            const value = this.#decodeSnapshotValue(mark.value);
            const meta: TextStyleMeta = {
              startId: { peer, counter: span.counter },
              lamport: span.counter + span.lamportSub,
              info: mark.info,
              value,
            };
            stylesById.set(idKey(meta.startId), { key, meta });
            const stack = active.get(key) ?? [];
            stack.push(meta);
            active.set(key, stack);
            continue;
          }
          if (span.length === -1) {
            const style = stylesById.get(idKey({ peer, counter: span.counter - 1 }));
            if (style !== undefined) {
              const stack = active.get(style.key);
              if (stack !== undefined) {
                const index = stack.lastIndexOf(style.meta);
                if (index >= 0) stack.splice(index, 1);
                if (stack.length === 0) active.delete(style.key);
              }
            }
            continue;
          }
          if (span.length < -1) continue;
          for (const [key, stack] of active) {
            const meta = stack.at(-1);
            if (meta !== undefined) {
              styleRuns.push({
                run: { start: { peer, counter: span.counter }, length: span.length },
                key,
                meta,
              });
            }
          }
          for (let offset = 0; offset < span.length; offset += 1) {
            ids.push({ peer, counter: span.counter + offset });
            lamports.push(span.counter + offset + span.lamportSub);
            characterIndex += 1;
          }
        }
        container._insertVisible(0, characters.slice(0, characterIndex), ids, lamports);
        for (const { run, key, meta } of styleRuns) {
          container._styleIndex.add([run], key, meta);
        }
        container._attributeHistoryComplete = false;
      } else if (
        container instanceof LoroTree &&
        state.kind === CodecContainerType.Tree
      ) {
        const records: TreeNodeRecord[] = [];
        for (let index = 0; index < state.nodes.length; index += 1) {
          const node = state.nodes[index]!;
          const nodeId = {
            peer: state.peers[Number(node.peerIndex)]!,
            counter: node.counter,
          };
          const dataId: CodecContainerId = {
            kind: "normal",
            ...nodeId,
            containerType: CodecContainerType.Map,
          };
          const record: TreeNodeRecord = {
            id: nodeId,
            parent: undefined,
            position: state.positions[node.fractionalIndexIndex]!.slice(),
            deleted: node.parentIndexPlusTwo === 1n,
            writer: {
              peer: state.peers[Number(node.lastSetPeerIndex)]!,
              lamport: node.lastSetCounter + node.lastSetLamportSub,
            },
            lastMoveId: {
              peer: state.peers[Number(node.lastSetPeerIndex)]!,
              counter: node.lastSetCounter,
            },
            data: this.#getOrCreateContainer(dataId, container) as LoroMap,
          };
          records.push(record);
        }
        for (let index = 0; index < state.nodes.length; index += 1) {
          const parent = state.nodes[index]!.parentIndexPlusTwo;
          if (parent >= 2n) records[index]!.parent = records[Number(parent - 2n)]!.id;
        }
        for (const record of records) container._setRecord(record);
      } else if (
        container instanceof LoroCounter &&
        state.kind === CodecContainerType.Counter
      ) {
        const bytes = new Uint8Array(8);
        let bits = state.bits;
        for (let index = 0; index < 8; index += 1) {
          bytes[index] = Number(bits & 0xffn);
          bits >>= 8n;
        }
        container._value = new DataView(bytes.buffer).getFloat64(0, true);
      } else if (
        container instanceof LoroMovableList &&
        state.kind === CodecContainerType.MovableList
      ) {
        const ids = state.listItemIds.slice(0, state.values.length);
        container._insertVisible(
          0,
          state.values.map((value) => this.#decodeSnapshotValue(value, container)),
          ids.map((item) => ({
            peer: state.peers[Number(item.peerIndex)]!,
            counter: item.counter,
          })),
          ids.map((item) => item.counter + item.lamportSub),
        );
        container._valueHistoryComplete = false;
        container._moveHistoryComplete = false;
      } else if (
        container instanceof LoroList &&
        state.kind === CodecContainerType.List
      ) {
        container._insertVisible(
          0,
          state.values.map((value) => this.#decodeSnapshotValue(value, container)),
          state.ids.map((item) => ({
            peer: state.peers[Number(item.peerIndex)]!,
            counter: item.counter,
          })),
          state.ids.map((item) => item.counter + item.lamportSub),
        );
      }
    }
  }

  #decodeSnapshotValue(value: EncodedLoroValue, parent?: LoroContainer): RuntimeValue {
    switch (value.type) {
      case "null":
        return null;
      case "bool":
        return value.value;
      case "double":
        return value.value;
      case "i64":
        return Number(value.value);
      case "string":
        return value.value;
      case "binary":
        return value.value.slice();
      case "list":
        return value.value.map((item) => this.#decodeSnapshotValue(item));
      case "map":
        return Object.fromEntries(
          value.value.map(([key, item]) => [key, this.#decodeSnapshotValue(item)]),
        );
      case "container":
        return this.#getOrCreateContainer(value.value, parent);
    }
  }

  #materializeMapValue(
    parent: LoroMap,
    key: string,
    rawValue: RuntimeValue,
  ): RuntimeValue {
    const parentId = parent._codecId;
    if (parentId === undefined) return rawValue;
    const childType = parseMergeableMarker(parentId, key, rawValue);
    if (childType === undefined) return rawValue;
    return this.#getOrCreateContainer(
      newMergeableContainerId(parentId, key, childType),
      parent,
    );
  }

  #captureEventValues(records: readonly HistoryRecord[]): Map<string, unknown> {
    const values = new Map<string, unknown>();
    for (const { change } of records) {
      for (const operation of change.operations) {
        const id = this.#containerKey(operation.container);
        if (values.has(id)) continue;
        const container = this.#containers.get(id);
        values.set(
          id,
          container === undefined ? undefined : containerEventValue(container),
        );
      }
    }
    return values;
  }

  #captureContainerEventValues(ids: ReadonlySet<string>): Map<string, unknown> {
    const values = new Map<string, unknown>();
    for (const id of ids) {
      const container = this.#containers.get(id);
      values.set(
        id,
        container === undefined ? undefined : containerEventValue(container),
      );
    }
    return values;
  }

  #captureMapKeys(ids: ReadonlySet<string>): Map<string, Set<string>> {
    const keys = new Map<string, Set<string>>();
    for (const id of ids) {
      const container = this.#containers.get(id);
      if (container instanceof LoroMap) keys.set(id, new Set(container._entries.keys()));
    }
    return keys;
  }

  #emit(
    by: "local" | "import" | "checkout",
    origin: string | undefined,
    from: readonly CodecId[],
    to: readonly CodecId[],
    changed: ReadonlySet<string>,
    beforeValues: ReadonlyMap<string, unknown> = new Map(),
    preparedDiffs: ReadonlyMap<string, Diff> = new Map(),
  ): void {
    if (changed.size === 0 || !this.#hasEventSubscribers()) return;
    const events: LoroEvent[] = [...changed].flatMap((id) => {
      const container = this.#containers.get(id);
      if (container === undefined) return [];
      const diff =
        preparedDiffs.get(id) ?? containerDiff(container, beforeValues.get(id));
      return isEmptyContainerDiff(diff)
        ? []
        : [{ target: id as ContainerID, diff, path: containerPath(container) }];
    });
    if (events.length === 0) return;
    const base = {
      by,
      ...(origin === undefined ? {} : { origin }),
      events,
      from: from.map(formatOpId),
      to: to.map(formatOpId),
    } satisfies LoroEventBatch;
    for (const listener of this.#subscribers) listener(base);
    const relevantByTarget = new Map<string, LoroEvent[]>();
    for (const event of events) {
      let current = this.#containers.get(event.target);
      while (current !== undefined) {
        const target = current.id;
        if (this.#containerSubscribers.has(target)) {
          const relevant = relevantByTarget.get(target);
          if (relevant === undefined) relevantByTarget.set(target, [event]);
          else relevant.push(event);
        }
        current = current.parent();
      }
    }
    for (const [target, relevant] of relevantByTarget) {
      const listeners = this.#containerSubscribers.get(target);
      if (listeners === undefined) continue;
      const batch = { ...base, currentTarget: target as ContainerID, events: relevant };
      for (const listener of listeners) listener(batch);
    }
  }

  #hasEventSubscribers(): boolean {
    return this.#subscribers.size > 0 || this.#containerSubscribers.size > 0;
  }
}

export class ChangeModifier {
  readonly #options: MutableCommitOptions;

  constructor(options: MutableCommitOptions) {
    this.#options = options;
  }

  free(): void {}

  setMessage(message: string): this {
    this.#options.message = message;
    return this;
  }

  setTimestamp(timestamp: number): this {
    if (!Number.isFinite(timestamp)) throw new TypeError("timestamp must be finite");
    this.#options.timestamp = timestamp;
    return this;
  }
}

export class Loro<
  T extends Record<string, Container> = Record<string, Container>,
> extends LoroDoc<T> {}

export function callPendingEvents(): void {}

export function LORO_VERSION(): string {
  return packageMetadata.version;
}

export function encodeFrontiers(frontiers: Frontiers): Uint8Array {
  return encodePostcardFrontiers(frontiers.map(parseOpId));
}

export function decodeFrontiers(bytes: Uint8Array): Frontiers {
  return decodePostcardFrontiers(bytes).map(formatOpId);
}

export function setDebug(): void {}

export function decodeImportBlobMeta(
  blob: Uint8Array,
  checkChecksum = true,
): ImportBlobMetadata {
  const parsed = decodeDocument(blob, { checkChecksum });
  if (parsed.mode === EncodeMode.FastUpdates) {
    const records = decodeFastUpdatesBody(parsed.body).flatMap(decodeHistoryRecordBlock);
    const startVersion = new VersionVector();
    const endVersion = new VersionVector();
    for (const { change } of records) {
      const currentStart = startVersion.get(change.id.peer);
      if (currentStart === undefined || change.id.counter < currentStart) {
        startVersion.set(change.id.peer, change.id.counter);
      }
      const end = change.id.counter + changeLength(change);
      if (end > (endVersion.get(change.id.peer) ?? 0)) {
        endVersion.set(change.id.peer, end);
      }
    }
    const startFrontiers = new Map<string, CodecId>();
    for (const { change } of records) {
      for (const dependency of change.dependencies) {
        const start = startVersion.get(dependency.peer);
        if (
          (start !== undefined && start > dependency.counter) ||
          (start === undefined && endVersion.get(dependency.peer) === undefined)
        ) {
          startFrontiers.set(idKey(dependency), dependency);
        }
      }
    }
    return {
      partialStartVersionVector: startVersion,
      partialEndVersionVector: endVersion,
      startFrontiers: [...startFrontiers.values()].map(formatOpId),
      startTimestamp: Number(records[0]?.change.timestamp ?? 0n),
      endTimestamp: Number(records.at(-1)?.change.timestamp ?? 0n),
      mode: "update",
      changeNum: records.length,
    };
  }

  const snapshot = decodeFastSnapshotBody(parsed.body);
  const records: HistoryRecord[] = [];
  let startVersion = new VersionVector();
  let endVersion = new VersionVector();
  let startFrontiers: CodecId[] = [];
  let endFrontiers: CodecId[] = [];
  for (const entry of decodeSstable(snapshot.oplog)) {
    if (bytesEqual(entry.key, VERSION_KEY)) {
      endVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
    } else if (bytesEqual(entry.key, FRONTIERS_KEY)) {
      endFrontiers = decodePostcardFrontiers(entry.value);
    } else if (bytesEqual(entry.key, START_VERSION_KEY)) {
      startVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
    } else if (bytesEqual(entry.key, START_FRONTIERS_KEY)) {
      startFrontiers = decodePostcardFrontiers(entry.value);
    } else if (entry.key.length === 12) {
      records.push(...decodeHistoryRecordBlock(entry.value));
    }
  }
  if (endVersion.length() === 0) {
    for (const { change } of records) {
      const end = change.id.counter + changeLength(change);
      if (end > (endVersion.get(change.id.peer) ?? 0))
        endVersion.set(change.id.peer, end);
    }
  }
  const timestampAt = (frontiers: readonly CodecId[]): number => {
    const targetByPeer = new Map(frontiers.map((id) => [id.peer, id] as const));
    let timestamp = 0;
    for (const { change } of records) {
      const frontier = targetByPeer.get(change.id.peer);
      if (frontier === undefined) continue;
      const end = change.id.counter + changeLength(change);
      if (frontier.counter >= change.id.counter && frontier.counter < end) {
        timestamp = Math.max(timestamp, Number(change.timestamp));
      }
    }
    return timestamp;
  };
  let latestRecord: HistoryRecord | undefined;
  for (const record of records) {
    if (latestRecord === undefined || compareHistoryRecords(latestRecord, record) < 0) {
      latestRecord = record;
    }
  }
  return {
    partialStartVersionVector: startVersion,
    partialEndVersionVector: endVersion,
    startFrontiers: startFrontiers.map(formatOpId),
    startTimestamp: timestampAt(startFrontiers),
    endTimestamp:
      timestampAt(endFrontiers) || Number(latestRecord?.change.timestamp ?? 0n),
    mode: snapshot.shallowRootState.length > 0 ? "shallow-snapshot" : "snapshot",
    changeNum: records.length,
  };
}

function decodeHistoryRecordBlock(bytes: Uint8Array): HistoryRecord[] {
  const block = decodeChangeBlock(bytes);
  return block.changes.map((change) => ({ change, keys: block.keys }));
}

function versionVectorFromCodec(ids: readonly CodecId[]): VersionVector {
  return new VersionVector(
    new Map(ids.map((id) => [peerIdToString(id.peer), id.counter])),
  );
}

function diffContainerId(value: unknown): ContainerID | undefined {
  if (isContainer(value)) return value.id;
  if (typeof value !== "string" || !value.startsWith("🦜:")) return undefined;
  const id = value.slice("🦜:".length);
  return isContainerId(id) ? (id as ContainerID) : undefined;
}

function ensureMergeableChild(
  parent: LoroMap,
  key: string,
  type: ContainerType,
): Container {
  switch (type) {
    case "Map":
      return parent.ensureMergeableMap(key);
    case "List":
      return parent.ensureMergeableList(key);
    case "MovableList":
      return parent.ensureMergeableMovableList(key);
    case "Text":
      return parent.ensureMergeableText(key);
    case "Tree":
      return parent.ensureMergeableTree(key);
    case "Counter":
      return parent.ensureMergeableCounter(key);
  }
}

function diffKindMismatch(container: Container, diffType: Diff["type"]): TypeError {
  return new TypeError(
    `cannot apply a ${diffType} diff to a ${container.kind()} container`,
  );
}

function createContainer(type: ContainerType): LoroContainer {
  switch (type) {
    case "Map":
      return new LoroMap();
    case "List":
      return new LoroList();
    case "Text":
      return new LoroText();
    case "Tree":
      return new LoroTree();
    case "MovableList":
      return new LoroMovableList();
    case "Counter":
      return new LoroCounter();
  }
}

function registerKey(pending: PendingChange, key: string): number {
  const existing = pending.keyIndices.get(key);
  if (existing !== undefined) return existing;
  const index = pending.keys.length;
  pending.keys.push(key);
  pending.keyIndices.set(key, index);
  return index;
}

function changeKey(id: CodecId): string {
  return `${id.peer}:${id.counter}`;
}
function idKey(id: CodecId): string {
  return `${id.peer}:${id.counter}`;
}
function frontierSetsEqual(left: Frontiers, right: Frontiers): boolean {
  if (left.length !== right.length) return false;
  const rightIds = new Set(right.map((id) => idKey(parseOpId(id))));
  return left.every((id) => rightIds.has(idKey(parseOpId(id))));
}
const changeLengthCache = new WeakMap<DecodedChange, number>();
function changeLength(change: DecodedChange): number {
  const cached = changeLengthCache.get(change);
  if (cached !== undefined) return cached;
  const length = change.operations.reduce((sum, operation) => sum + operation.length, 0);
  changeLengthCache.set(change, length);
  return length;
}

function changedContainerIds(records: readonly HistoryRecord[]): Set<string> {
  return new Set(
    records.flatMap(({ change }) =>
      change.operations.map((operation) => formatContainerId(operation.container)),
    ),
  );
}

function longestIncreasingSubsequenceIndices(values: readonly number[]): Set<number> {
  if (values.length === 0) return new Set();
  const tails: number[] = [];
  const previous = new Int32Array(values.length);
  previous.fill(-1);
  for (let index = 0; index < values.length; index += 1) {
    let low = 0;
    let high = tails.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (values[tails[middle]!]! < values[index]!) low = middle + 1;
      else high = middle;
    }
    if (low > 0) previous[index] = tails[low - 1]!;
    tails[low] = index;
  }
  const indices = new Set<number>();
  let index = tails.at(-1)!;
  while (index >= 0) {
    indices.add(index);
    index = previous[index]!;
  }
  return indices;
}

function movableMoveTransitionMode(
  retreat: readonly HistoryRecord[],
  forward: readonly HistoryRecord[],
  movePeers: ReadonlyMap<string, ReadonlySet<bigint>>,
): MovableMoveTransitionMode {
  const changedMoveContainers = new Set<string>();
  for (const { change } of [...retreat, ...forward]) {
    for (const operation of change.operations) {
      if (operation.content.type === "movable-list-move") {
        changedMoveContainers.add(formatContainerId(operation.container));
      }
    }
  }
  if (changedMoveContainers.size === 0) return "anchors";
  if (retreat.length > 0 && forward.length > 0) return "replay";
  for (const containerId of changedMoveContainers) {
    if ((movePeers.get(containerId)?.size ?? 0) > 1) return "replay";
  }
  return "anchors";
}

function hasMaterializedSequenceInsertions(
  records: readonly HistoryRecord[],
  containers: ReadonlyMap<string, LoroContainer>,
): boolean {
  for (const { change } of records) {
    for (const operation of change.operations) {
      const content = operation.content;
      if (
        content.type !== "text-insert" &&
        content.type !== "list-insert" &&
        content.type !== "movable-list-insert"
      ) {
        continue;
      }
      const container = containers.get(formatContainerId(operation.container));
      if (
        (container instanceof LoroText || container instanceof LoroList) &&
        container._sequence.findById({
          peer: change.id.peer,
          counter: operation.counter,
        }) !== undefined
      ) {
        return true;
      }
    }
  }
  return false;
}

function canMergeChanges(
  left: DecodedChange,
  right: DecodedChange,
  interval: bigint,
): boolean {
  return (
    right.id.peer === left.id.peer &&
    right.id.counter === left.id.counter + changeLength(left) &&
    right.dependencies.length === 1 &&
    right.dependencies[0]!.peer === left.id.peer &&
    right.timestamp - left.timestamp <= interval &&
    right.message === left.message
  );
}

function appendHistoryRecord(
  left: HistoryRecord,
  right: HistoryRecord,
  leftLength: number,
): HistoryRecord {
  let keyIndices = left.keyIndices;
  let keys: string[];
  if (keyIndices === undefined) {
    keys = [...left.keys];
    keyIndices = new Map<string, number>();
    for (const [index, key] of keys.entries()) {
      if (!keyIndices.has(key)) keyIndices.set(key, index);
    }
    left.keys = keys;
    left.keyIndices = keyIndices;
  } else {
    keys = left.keys as string[];
  }
  const remappedKeyIndices = right.keys.map((key) => {
    const existing = keyIndices.get(key);
    if (existing !== undefined) return existing;
    const index = keys.length;
    keys.push(key);
    keyIndices.set(key, index);
    return index;
  });

  const appendedOperations = right.change.operations.map((operation) =>
    remapOperationKeys(operation, remappedKeyIndices),
  );
  const mergedOperations = left.change.operations as DecodedOperation[];
  for (const operation of appendedOperations) mergedOperations.push(operation);
  const rightLength = changeLength(right.change);
  changeLengthCache.set(left.change, leftLength + rightLength);

  const appended = {
    keys,
    change: {
      ...right.change,
      timestamp: left.change.timestamp,
      message: left.change.message,
      operations: appendedOperations,
    },
  };
  changeLengthCache.set(appended.change, rightLength);
  return appended;
}

function cloneHistoryRecord(record: HistoryRecord): HistoryRecord {
  return {
    keys: record.keys,
    change: { ...record.change, operations: [...record.change.operations] },
  };
}

function appendToTrailingListInsert(
  operations: DecodedOperation[],
  operation: DecodedOperation,
): boolean {
  const content = operation.content;
  if (content.type !== "list-insert" && content.type !== "movable-list-insert") {
    return false;
  }
  const previousIndex = operations.length - 1;
  const previous = operations[previousIndex];
  if (
    previous === undefined ||
    previous.content.type !== content.type ||
    !containerIdsEqual(previous.container, operation.container) ||
    previous.counter + previous.length !== operation.counter ||
    previous.content.position + previous.length !== content.position
  ) {
    return false;
  }

  const values = previous.content.values as ChangeLoroValue[];
  for (const value of content.values) values.push(value);
  operations[previousIndex] = {
    ...previous,
    length: previous.length + operation.length,
    content: { ...previous.content, values },
  };
  return true;
}

function remapOperationKeys(
  operation: DecodedOperation,
  remap: readonly number[],
): DecodedOperation {
  const content = operation.content;
  switch (content.type) {
    case "map-insert":
    case "text-mark":
    case "movable-list-set":
      return {
        ...operation,
        content: { ...content, value: remapLoroValueKeys(content.value, remap) },
      };
    case "list-insert":
    case "movable-list-insert":
      return {
        ...operation,
        content: {
          ...content,
          values: content.values.map((value) => remapLoroValueKeys(value, remap)),
        },
      };
    case "future":
      return {
        ...operation,
        content: { ...content, value: remapChangeValueKeys(content.value, remap) },
      };
    default:
      return operation;
  }
}

function remapChangeValueKeys(value: ChangeValue, remap: readonly number[]): ChangeValue {
  switch (value.type) {
    case "loro-value":
      return { ...value, value: remapLoroValueKeys(value.value, remap) };
    case "mark-start":
      return {
        ...value,
        keyIndex: remapKeyIndex(value.keyIndex, remap),
        value: remapLoroValueKeys(value.value, remap),
      };
    case "list-set":
      return { ...value, value: remapLoroValueKeys(value.value, remap) };
    default:
      return value;
  }
}

function remapLoroValueKeys(
  value: ChangeLoroValue,
  remap: readonly number[],
): ChangeLoroValue {
  if (value.type === "list") {
    return {
      ...value,
      value: value.value.map((item) => remapLoroValueKeys(item, remap)),
    };
  }
  if (value.type === "map") {
    return {
      ...value,
      value: value.value.map(([keyIndex, item]): readonly [bigint, ChangeLoroValue] => [
        remapKeyIndex(keyIndex, remap),
        remapLoroValueKeys(item, remap),
      ]),
    };
  }
  return value;
}

function remapKeyIndex(keyIndex: bigint, remap: readonly number[]): bigint {
  if (keyIndex < 0n || keyIndex > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error("change value map key index is out of range");
  }
  const mapped = remap[Number(keyIndex)];
  if (mapped === undefined) {
    throw new Error("change value map key index is out of range");
  }
  return BigInt(mapped);
}

const I64_MAX = 0x7fff_ffff_ffff_ffffn;
const I64_MIN = -0x8000_0000_0000_0000n;

function numberToI64(value: number): bigint {
  if (Number.isNaN(value)) return 0n;
  if (value >= Number(I64_MAX)) return I64_MAX;
  if (value <= Number(I64_MIN)) return I64_MIN;
  return BigInt(Math.trunc(value));
}

function maxBigInt(left: bigint, right: bigint): bigint {
  return left >= right ? left : right;
}

function sliceChange(change: DecodedChange, from: number, to: number): DecodedChange {
  const totalLength = changeLength(change);
  if (
    !Number.isSafeInteger(from) ||
    !Number.isSafeInteger(to) ||
    from < 0 ||
    from >= to ||
    to > totalLength
  ) {
    throw new RangeError("change slice is out of range");
  }

  const operations: DecodedOperation[] = [];
  let offset = 0;
  for (const operation of change.operations) {
    const operationStart = offset;
    const operationEnd = offset + operation.length;
    offset = operationEnd;
    if (operationEnd <= from) continue;
    if (operationStart >= to) break;
    const sliceFrom = Math.max(0, from - operationStart);
    const sliceTo = Math.min(operation.length, to - operationStart);
    if (sliceFrom === 0 && sliceTo === operation.length) {
      operations.push(operation);
    } else {
      operations.push(sliceOperation(operation, sliceFrom, sliceTo));
    }
  }
  return {
    ...change,
    id: { peer: change.id.peer, counter: change.id.counter + from },
    lamport: change.lamport + from,
    dependencies:
      from === 0
        ? change.dependencies
        : [{ peer: change.id.peer, counter: change.id.counter + from - 1 }],
    operations,
  };
}

function sliceOperation(
  operation: DecodedOperation,
  from: number,
  to: number,
): DecodedOperation {
  if (from < 0 || from >= to || to > operation.length) {
    throw new RangeError("partial operation length is out of range");
  }
  const length = to - from;
  const content = operation.content;
  switch (content.type) {
    case "text-insert":
      return {
        ...operation,
        counter: operation.counter + from,
        length,
        content: {
          ...content,
          position: content.position + from,
          value: Array.from(content.value).slice(from, to).join(""),
        },
      };
    case "list-insert":
    case "movable-list-insert":
      return {
        ...operation,
        counter: operation.counter + from,
        length,
        content: {
          ...content,
          position: content.position + from,
          values: content.values.slice(from, to),
        },
      };
    case "text-delete":
    case "list-delete":
    case "movable-list-delete": {
      const signedLength = Number(content.length);
      const positive = signedLength >= 0;
      return {
        ...operation,
        counter: operation.counter + from,
        length,
        content: {
          ...content,
          position: positive ? content.position : content.position - from,
          length: BigInt(positive ? length : -length),
          startId: {
            peer: content.startId.peer,
            counter: content.startId.counter + (positive ? from : operation.length - to),
          },
        },
      };
    }
    default:
      throw new Error(`cannot slice ${content.type} operation`);
  }
}

function versionIncludes(version: VersionVector, required: VersionVector): boolean {
  return required
    ._codecEntriesUnsorted()
    .every(({ peer, counter }) => (version.get(peer) ?? 0) >= counter);
}

function historyVersionForRecords(
  records: readonly HistoryRecord[],
  base?: VersionVector,
): VersionVector {
  const version = base?.clone() ?? new VersionVector();
  for (const { change } of records) {
    const end = change.id.counter + changeLength(change);
    if (end > (version.get(change.id.peer) ?? 0)) {
      version.set(change.id.peer, end);
    }
  }
  return version;
}

function versionDistance(start: VersionVector, end: VersionVector): number {
  let distance = 0;
  for (const { peer, counter } of end._codecEntriesUnsorted()) {
    distance += Math.max(0, counter - (start.get(peer) ?? 0));
  }
  return distance;
}

function mergeStateSnapshotStores(
  root: StateSnapshotStore,
  overlay: StateSnapshotStore,
): StateSnapshotStore {
  if (root.kind !== "sstable") return overlay;
  if (overlay.kind !== "sstable") {
    return { kind: "sstable", frontiers: undefined, containers: root.containers };
  }
  const containers = new Map(
    root.containers.map((entry) => [formatContainerId(entry.id), entry]),
  );
  for (const entry of overlay.containers) {
    containers.set(formatContainerId(entry.id), entry);
  }
  return {
    kind: "sstable",
    frontiers: undefined,
    containers: [...containers.values()],
  };
}

function compareIds(left: CodecId, right: CodecId): number {
  return left.peer < right.peer
    ? -1
    : left.peer > right.peer
      ? 1
      : left.counter - right.counter;
}

function compareHistoryRecords(left: HistoryRecord, right: HistoryRecord): number {
  return (
    left.change.lamport - right.change.lamport ||
    compareIds(left.change.id, right.change.id)
  );
}

function compareHistoryOperations(
  leftChange: DecodedChange,
  leftOperation: DecodedOperation,
  rightChange: DecodedChange,
  rightOperation: DecodedOperation,
): number {
  return compareWriter(
    operationWriter(leftChange, leftOperation),
    operationWriter(rightChange, rightOperation),
  );
}

function lowerBoundHistory(records: readonly HistoryRecord[], counter: number): number {
  let low = 0;
  let high = records.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (records[middle]!.change.id.counter < counter) low = middle + 1;
    else high = middle;
  }
  return low;
}

function lowerBoundOperation(
  operations: readonly DecodedOperation[],
  counter: number,
): number {
  let low = 0;
  let high = operations.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (operations[middle]!.counter < counter) low = middle + 1;
    else high = middle;
  }
  return low;
}

function lowerBoundIndexedOperation(
  operations: readonly IndexedHistoryOperation[],
  counter: number,
): number {
  let low = 0;
  let high = operations.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (operations[middle]!.operation.counter < counter) low = middle + 1;
    else high = middle;
  }
  return low;
}

function lowerBoundWriter(
  operations: readonly IndexedHistoryOperation[],
  writer: LastWriter,
): number {
  let low = 0;
  let high = operations.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (compareWriter(operations[middle]!.writer, writer) < 0) low = middle + 1;
    else high = middle;
  }
  return low;
}

function latestIncludedOperation(
  history: IndexedSubjectHistory | undefined,
  version: VersionVector,
): IndexedHistoryOperation | undefined {
  if (history === undefined) return undefined;
  let latest: IndexedHistoryOperation | undefined;
  for (const [peer, operations] of history.byPeer) {
    const index = lowerBoundIndexedOperation(operations, version.get(peer) ?? 0) - 1;
    const operation = operations[index];
    if (
      operation !== undefined &&
      (latest === undefined || compareWriter(latest.writer, operation.writer) < 0)
    ) {
      latest = operation;
    }
  }
  return latest;
}

function latestIncludedTreePlacement(
  history: IndexedSubjectHistory | undefined,
  version: VersionVector,
  before: LastWriter,
): IndexedHistoryOperation | undefined {
  if (history?.placementsByPeer === undefined) return undefined;
  let latest: IndexedHistoryOperation | undefined;
  for (const [peer, operations] of history.placementsByPeer) {
    const index =
      Math.min(
        lowerBoundIndexedOperation(operations, version.get(peer) ?? 0),
        lowerBoundWriter(operations, before),
      ) - 1;
    const operation = operations[index];
    if (
      operation !== undefined &&
      (latest === undefined || compareWriter(latest.writer, operation.writer) < 0)
    ) {
      latest = operation;
    }
  }
  return latest;
}

function latestIncludedSequenceValue(
  history: readonly SequenceValueMeta[] | undefined,
  version: VersionVector,
): SequenceValueMeta | undefined {
  if (history === undefined) return undefined;
  for (let index = history.length - 1; index >= 0; index -= 1) {
    const meta = history[index]!;
    if (meta.id.counter < (version.get(meta.id.peer) ?? 0)) return meta;
  }
  return undefined;
}

function hasSequenceValueMeta(
  history: readonly SequenceValueMeta[] | undefined,
  writer: LastWriter,
  peer: bigint,
  counter: number,
): boolean {
  if (history === undefined) return false;
  let low = 0;
  let high = history.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    const meta = history[middle]!;
    if (compareWriter({ peer: meta.id.peer, lamport: meta.lamport }, writer) < 0) {
      low = middle + 1;
    } else {
      high = middle;
    }
  }
  const meta = history[low];
  return meta?.id.peer === peer && meta.id.counter === counter;
}

function findSequenceMoveMeta(
  history: readonly SequenceMoveMeta[] | undefined,
  writer: LastWriter,
): SequenceMoveMeta | undefined {
  const index = findSequenceMoveMetaIndex(history, writer);
  return index < 0 ? undefined : history![index];
}

function findSequenceMoveMetaIndex(
  history: readonly SequenceMoveMeta[] | undefined,
  writer: LastWriter,
): number {
  if (history === undefined) return -1;
  let low = 0;
  let high = history.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    const meta = history[middle]!;
    if (compareWriter({ peer: meta.id.peer, lamport: meta.lamport }, writer) < 0) {
      low = middle + 1;
    } else {
      high = middle;
    }
  }
  return low < history.length ? low : -1;
}

function counterDelta(
  content: Extract<DecodedOperationContent, { type: "future" }>,
): number | undefined {
  if (content.value.type === "double") return content.value.value;
  if (content.value.type === "i64") return Number(content.value.value);
  if (content.value.type === "delta-int") return content.value.value;
  return undefined;
}

function publicChange(change: DecodedChange): Change {
  return {
    peer: peerIdToString(change.id.peer),
    counter: change.id.counter,
    lamport: change.lamport,
    length: changeLength(change),
    timestamp: Number(change.timestamp),
    deps: change.dependencies.map(formatOpId),
    message: change.message,
  };
}

type JsonPeerMap = ReadonlyMap<bigint, bigint>;

function historyRecordsToJsonSchema(
  records: readonly HistoryRecord[],
  startVersion: VersionVector,
  withPeerCompression: boolean,
  nullMessage = true,
): JsonSchema {
  const peers = collectJsonPeers(records, startVersion);
  const peerMap = withPeerCompression
    ? new Map(peers.map((peer, index) => [peer, BigInt(index)]))
    : undefined;
  return {
    schema_version: 1,
    start_version: Object.fromEntries(
      startVersion
        .codecEntries()
        .map(({ peer, counter }) => [(peerMap?.get(peer) ?? peer).toString(), counter]),
    ),
    peers: withPeerCompression ? peers.map(peerIdToString) : null,
    changes: records.map((record) =>
      historyRecordToJsonChange(record, peerMap, nullMessage),
    ),
  };
}

function historyRecordToJsonChange(
  record: HistoryRecord,
  peerMap: JsonPeerMap | undefined,
  nullMessage: boolean,
): JsonChange {
  const { change } = record;
  return {
    id: formatJsonOpId(change.id, peerMap),
    timestamp: Number(change.timestamp),
    deps: change.dependencies.map((id) => formatJsonOpId(id, peerMap)),
    lamport: change.lamport,
    msg: change.message ?? (nullMessage ? null : undefined),
    ops: change.operations.map((operation) =>
      decodedOperationToJson(operation, record.keys, change.id.peer, peerMap),
    ),
  };
}

function collectJsonPeers(
  records: readonly HistoryRecord[],
  startVersion: VersionVector,
): bigint[] {
  const peers = new Set(startVersion._codecEntriesUnsorted().map(({ peer }) => peer));
  const addId = (id: CodecId | undefined): void => {
    if (id !== undefined) peers.add(id.peer);
  };
  for (const { change } of records) {
    addId(change.id);
    for (const dependency of change.dependencies) addId(dependency);
    for (const operation of change.operations) {
      if (operation.container.kind === "normal") peers.add(operation.container.peer);
      const content = operation.content;
      if (
        content.type === "text-delete" ||
        content.type === "list-delete" ||
        content.type === "movable-list-delete"
      ) {
        addId(content.startId);
      } else if (content.type === "tree-create" || content.type === "tree-move") {
        addId(content.subject);
        addId(content.parent);
      } else if (content.type === "tree-delete") {
        addId(content.subject);
      } else if (
        content.type === "movable-list-move" ||
        content.type === "movable-list-set"
      ) {
        peers.add(content.elementId.peer);
      }
    }
  }
  return [...peers].sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
}

function decodedOperationToJson(
  operation: DecodedOperation,
  keys: readonly string[],
  changePeer: bigint,
  peerMap: JsonPeerMap | undefined,
): JsonOp {
  const operationId = { peer: changePeer, counter: operation.counter };
  const content = operation.content;
  let json: JsonOpContent;
  switch (content.type) {
    case "map-insert":
      json = {
        type: "insert",
        key: content.key,
        value: changeLoroValueToJson(content.value, keys, operationId, peerMap),
      };
      break;
    case "map-delete":
      json = { type: "delete", key: content.key };
      break;
    case "text-insert":
      json = { type: "insert", pos: content.position, text: content.value };
      break;
    case "text-delete":
      json = {
        type: "delete",
        pos: content.position,
        len: Number(content.length),
        start_id: formatJsonOpId(content.startId, peerMap),
      };
      break;
    case "text-mark":
      json = {
        type: "mark",
        start: content.start,
        end: content.end,
        style_key: content.key,
        style_value: changeLoroValueToJson(content.value, keys, operationId, peerMap),
        info: content.info,
      };
      break;
    case "text-mark-end":
      json = { type: "mark_end" };
      break;
    case "list-insert":
    case "movable-list-insert":
      json = {
        type: "insert",
        pos: content.position,
        value: content.values.map((value, index) =>
          changeLoroValueToJson(
            value,
            keys,
            { peer: changePeer, counter: operation.counter + index },
            peerMap,
          ),
        ),
      };
      break;
    case "list-delete":
    case "movable-list-delete":
      json = {
        type: "delete",
        pos: content.position,
        len: Number(content.length),
        start_id: formatJsonOpId(content.startId, peerMap),
      };
      break;
    case "movable-list-move":
      json = {
        type: "move",
        from: content.from,
        to: content.to,
        elem_id: formatJsonOpId(
          { peer: content.elementId.peer, counter: content.elementId.lamport },
          peerMap,
        ),
      };
      break;
    case "movable-list-set":
      json = {
        type: "set",
        elem_id: formatJsonOpId(
          { peer: content.elementId.peer, counter: content.elementId.lamport },
          peerMap,
        ),
        value: changeLoroValueToJson(content.value, keys, operationId, peerMap),
      };
      break;
    case "tree-create":
    case "tree-move":
      json = {
        type: content.type === "tree-create" ? "create" : "move",
        target: formatJsonTreeId(content.subject, peerMap),
        parent:
          content.parent === undefined ? null : formatJsonTreeId(content.parent, peerMap),
        fractional_index: bytesToHex(content.position).toUpperCase(),
      };
      break;
    case "tree-delete":
      json = { type: "delete", target: formatJsonTreeId(content.subject, peerMap) };
      break;
    case "future": {
      const value = content.value;
      if (value.type === "double" || value.type === "i64") {
        json = {
          type: "counter",
          value: value.type === "double" ? value.value : Number(value.value),
          prop: content.property,
        };
      } else if (value.type === "delta-int") {
        json = { type: "counter", value: value.value, prop: content.property };
      } else {
        json = {
          type: "unknown",
          prop: content.property,
          value: changeValueToJsonUnknown(value),
        };
      }
      break;
    }
  }
  return {
    container: formatJsonContainerId(operation.container, peerMap),
    content: json,
    counter: operation.counter,
  };
}

function changeLoroValueToJson(
  value: ChangeLoroValue,
  keys: readonly string[],
  operationId: CodecId,
  peerMap: JsonPeerMap | undefined,
): JsonValue {
  switch (value.type) {
    case "null":
      return null;
    case "bool":
    case "double":
    case "string":
      return value.value;
    case "i64":
      return Number(value.value);
    case "binary":
      return value.value.slice();
    case "list":
      return value.value.map((item, index) =>
        changeLoroValueToJson(
          item,
          keys,
          { peer: operationId.peer, counter: operationId.counter + index },
          peerMap,
        ),
      );
    case "map":
      return Object.fromEntries(
        value.value.map(([keyIndex, item]) => {
          const key = keys[Number(keyIndex)];
          if (key === undefined) throw new RangeError("JSON value key is out of range");
          return [key, changeLoroValueToJson(item, keys, operationId, peerMap)];
        }),
      );
    case "container-type": {
      const child: CodecContainerId = {
        kind: "normal",
        ...operationId,
        containerType: containerTypeFromRawByte(value.value),
      };
      return `🦜:${formatJsonContainerId(child, peerMap)}` as JsonContainerID;
    }
  }
}

function changeValueToJsonUnknown(value: unknown): unknown {
  if (typeof value !== "object" || value === null) return value;
  if ("value" in value) {
    const inner = (value as { value: unknown }).value;
    return typeof inner === "bigint" ? Number(inner) : inner;
  }
  if ("data" in value) return (value as { data: Uint8Array }).data.slice();
  return value;
}

function formatJsonOpId(id: CodecId, peerMap?: JsonPeerMap): `${number}@${PeerID}` {
  return `${id.counter}@${(peerMap?.get(id.peer) ?? id.peer).toString()}` as `${number}@${PeerID}`;
}

function formatJsonTreeId(id: CodecId, peerMap?: JsonPeerMap): TreeID {
  return formatJsonOpId(id, peerMap) as TreeID;
}

function formatJsonContainerId(id: CodecContainerId, peerMap?: JsonPeerMap): ContainerID {
  return formatContainerId(
    id.kind === "normal" ? { ...id, peer: peerMap?.get(id.peer) ?? id.peer } : id,
  );
}

function jsonSchemaToHistoryRecords(schema: JsonSchema): HistoryRecord[] {
  if (!Array.isArray(schema.changes))
    throw new TypeError("JSON changes must be an array");
  const peers = Array.isArray(schema.peers) ? schema.peers.map(parsePeerId) : undefined;
  const resolvePeer = (peer: bigint): bigint => {
    if (peers === undefined) return peer;
    const resolved = peers[Number(peer)];
    if (resolved === undefined)
      throw new RangeError(`JSON peer index ${peer} is missing`);
    return resolved;
  };
  const parseJsonId = (value: string): CodecId => {
    const parsed = parseTreeId(value as TreeID);
    return { peer: resolvePeer(parsed.peer), counter: parsed.counter };
  };
  const parseJsonContainer = (value: string): CodecContainerId => {
    const parsed = parseContainerId(value);
    return parsed.kind === "normal"
      ? { ...parsed, peer: resolvePeer(parsed.peer) }
      : parsed;
  };

  return schema.changes.map((change) => {
    const id = parseJsonId(change.id);
    const keys: string[] = [];
    const keyIndices = new Map<string, number>();
    const registerJsonKey = (key: string): number => {
      const existing = keyIndices.get(key);
      if (existing !== undefined) return existing;
      const index = keys.length;
      keys.push(key);
      keyIndices.set(key, index);
      return index;
    };
    const operations = change.ops.map((operation) =>
      jsonOperationToDecoded(
        operation,
        id.peer,
        resolvePeer,
        parseJsonContainer,
        registerJsonKey,
      ),
    );
    let expectedCounter = id.counter;
    for (const operation of operations) {
      if (operation.counter !== expectedCounter) {
        throw new RangeError(
          `JSON operation counter ${operation.counter} does not follow ${expectedCounter}`,
        );
      }
      expectedCounter += operation.length;
    }
    return {
      keys,
      change: {
        id,
        timestamp: BigInt(Math.trunc(change.timestamp)),
        dependencies: change.deps.map(parseJsonId),
        lamport: change.lamport,
        message: change.msg ?? undefined,
        operations,
      },
    };
  });
}

function jsonOperationToDecoded(
  operation: JsonOp,
  changePeer: bigint,
  resolvePeer: (peer: bigint) => bigint,
  parseJsonContainer: (value: string) => CodecContainerId,
  registerKey: (key: string) => number,
): DecodedOperation {
  const container = parseJsonContainer(operation.container);
  const containerType = codecTypeToPublic(container.containerType);
  const content = operation.content as JsonOpContent & Record<string, unknown>;
  const operationId = { peer: changePeer, counter: operation.counter };
  const parseId = (value: unknown): CodecId => {
    if (typeof value !== "string")
      throw new TypeError("JSON operation ID must be a string");
    const parsed = parseTreeId(value as TreeID);
    return { peer: resolvePeer(parsed.peer), counter: parsed.counter };
  };
  const parseParent = (value: unknown): CodecId | undefined =>
    value === null || value === undefined ? undefined : parseId(value);
  const encodeValue = (value: unknown, id = operationId): ChangeLoroValue =>
    jsonValueToChangeLoroValue(value, id, resolvePeer, registerKey);

  if (containerType === "Map") {
    if (content.type === "delete") {
      if (typeof content.key !== "string")
        throw new TypeError("map delete key is missing");
      return {
        container,
        counter: operation.counter,
        length: 1,
        content: { type: "map-delete", key: content.key },
      };
    }
    if (content.type !== "insert" || typeof content.key !== "string") {
      throw new TypeError("invalid map JSON operation");
    }
    return {
      container,
      counter: operation.counter,
      length: 1,
      content: {
        type: "map-insert",
        key: content.key,
        value: encodeValue(content.value),
      },
    };
  }

  if (containerType === "Text") {
    if (content.type === "insert" && typeof content.text === "string") {
      return {
        container,
        counter: operation.counter,
        length: Array.from(content.text).length,
        content: {
          type: "text-insert",
          position: requireJsonInteger(content.pos, "text insert position"),
          value: content.text,
        },
      };
    }
    if (content.type === "delete") {
      const length = requireJsonInteger(content.len, "text delete length");
      return {
        container,
        counter: operation.counter,
        length: Math.abs(length),
        content: {
          type: "text-delete",
          position: requireJsonInteger(content.pos, "text delete position"),
          length: BigInt(length),
          startId: parseId(content.start_id),
        },
      };
    }
    if (content.type === "mark") {
      if (typeof content.style_key !== "string")
        throw new TypeError("text mark key is missing");
      return {
        container,
        counter: operation.counter,
        length: 1,
        content: {
          type: "text-mark",
          start: requireJsonInteger(content.start, "text mark start"),
          end: requireJsonInteger(content.end, "text mark end"),
          key: content.style_key,
          value: encodeValue(content.style_value),
          info: requireJsonInteger(content.info, "text mark info"),
        },
      };
    }
    if (content.type === "mark_end") {
      return {
        container,
        counter: operation.counter,
        length: 1,
        content: { type: "text-mark-end" },
      };
    }
    throw new TypeError("invalid text JSON operation");
  }

  if (containerType === "List" || containerType === "MovableList") {
    const movable = containerType === "MovableList";
    if (content.type === "insert") {
      if (!Array.isArray(content.value))
        throw new TypeError("list insert value must be an array");
      const values = content.value.map((value, index) =>
        encodeValue(value, { peer: changePeer, counter: operation.counter + index }),
      );
      return {
        container,
        counter: operation.counter,
        length: values.length,
        content: {
          type: movable ? "movable-list-insert" : "list-insert",
          position: requireJsonInteger(content.pos, "list insert position"),
          values,
        },
      };
    }
    if (content.type === "delete") {
      const length = requireJsonInteger(content.len, "list delete length");
      return {
        container,
        counter: operation.counter,
        length: Math.abs(length),
        content: {
          type: movable ? "movable-list-delete" : "list-delete",
          position: requireJsonInteger(content.pos, "list delete position"),
          length: BigInt(length),
          startId: parseId(content.start_id),
        },
      };
    }
    if (movable && content.type === "move") {
      const elementId = parseId(content.elem_id);
      return {
        container,
        counter: operation.counter,
        length: 1,
        content: {
          type: "movable-list-move",
          from: requireJsonInteger(content.from, "movable-list source"),
          to: requireJsonInteger(content.to, "movable-list destination"),
          elementId: { peer: elementId.peer, lamport: elementId.counter },
        },
      };
    }
    if (movable && content.type === "set") {
      const elementId = parseId(content.elem_id);
      return {
        container,
        counter: operation.counter,
        length: 1,
        content: {
          type: "movable-list-set",
          elementId: { peer: elementId.peer, lamport: elementId.counter },
          value: encodeValue(content.value),
        },
      };
    }
    throw new TypeError("invalid list JSON operation");
  }

  if (containerType === "Tree") {
    if (content.type === "delete") {
      return {
        container,
        counter: operation.counter,
        length: 1,
        content: { type: "tree-delete", subject: parseId(content.target) },
      };
    }
    if (content.type !== "create" && content.type !== "move") {
      throw new TypeError("invalid tree JSON operation");
    }
    const position =
      typeof content.fractional_index === "string"
        ? hexToBytes(content.fractional_index)
        : Uint8Array.of(0x80);
    return {
      container,
      counter: operation.counter,
      length: 1,
      content: {
        type: content.type === "create" ? "tree-create" : "tree-move",
        subject: parseId(content.target),
        parent: parseParent(content.parent),
        position,
      },
    };
  }

  if (containerType === "Counter") {
    const value =
      typeof content === "number"
        ? content
        : typeof content.value === "number"
          ? content.value
          : Number.NaN;
    if (!Number.isFinite(value)) throw new TypeError("counter JSON value must be finite");
    return {
      container,
      counter: operation.counter,
      length: 1,
      content: {
        type: "future",
        property:
          typeof content === "object" && content !== null && "prop" in content
            ? requireJsonInteger(content.prop, "counter property")
            : 0,
        value: { type: "double", value },
      },
    };
  }

  throw new TypeError("JSON updates do not support this container type");
}

function jsonValueToChangeLoroValue(
  value: unknown,
  operationId: CodecId,
  resolvePeer: (peer: bigint) => bigint,
  registerKey: (key: string) => number,
): ChangeLoroValue {
  if (value === undefined || value === null) return { type: "null" };
  if (typeof value === "boolean") return { type: "bool", value };
  if (typeof value === "number") {
    if (!Number.isFinite(value))
      throw new TypeError("JSON update numbers must be finite");
    return Number.isSafeInteger(value)
      ? { type: "i64", value: BigInt(value) }
      : { type: "double", value };
  }
  if (typeof value === "bigint") return { type: "i64", value };
  if (typeof value === "string") {
    if (value.startsWith("🦜:")) {
      const rawId = value.slice("🦜:".length);
      if (isContainerId(rawId)) {
        const child = parseContainerId(rawId);
        if (child.kind !== "normal") {
          throw new TypeError("JSON child references must use normal container IDs");
        }
        const peer = resolvePeer(child.peer);
        if (peer !== operationId.peer || child.counter !== operationId.counter) {
          throw new RangeError("JSON child container ID does not match its operation ID");
        }
        return {
          type: "container-type",
          value: containerTypeToRawByte(child.containerType),
        };
      }
    }
    return { type: "string", value };
  }
  if (value instanceof Uint8Array) return { type: "binary", value: value.slice() };
  if (Array.isArray(value)) {
    return {
      type: "list",
      value: value.map((item, index) =>
        jsonValueToChangeLoroValue(
          item,
          { peer: operationId.peer, counter: operationId.counter + index },
          resolvePeer,
          registerKey,
        ),
      ),
    };
  }
  if (typeof value === "object") {
    return {
      type: "map",
      value: Object.entries(value).map(([key, item]) => [
        BigInt(registerKey(key)),
        jsonValueToChangeLoroValue(item, operationId, resolvePeer, registerKey),
      ]),
    };
  }
  throw new TypeError(`unsupported JSON update value type: ${typeof value}`);
}

function requireJsonInteger(value: unknown, label: string): number {
  if (!Number.isSafeInteger(value)) throw new TypeError(`${label} must be an integer`);
  return value as number;
}

function unicodeScalarLength(value: string): number {
  let length = 0;
  for (const _scalar of value) length += 1;
  return length;
}

function compareWriter(left: LastWriter, right: LastWriter): number {
  return (
    left.lamport - right.lamport ||
    (left.peer < right.peer ? -1 : left.peer > right.peer ? 1 : 0)
  );
}

function operationWriter(change: DecodedChange, operation: DecodedOperation): LastWriter {
  return {
    peer: change.id.peer,
    lamport: change.lamport + operation.counter - change.id.counter,
  };
}

function captureBlueprint(container: Container): ContainerBlueprint {
  if (container instanceof LoroText) return { kind: "Text", value: container.toDelta() };
  if (container instanceof LoroCounter)
    return { kind: "Counter", value: container.value };
  if (container instanceof LoroList) {
    return {
      kind: container.kind(),
      value: container._visibleElements().map((element) => element.value),
    };
  }
  return {
    kind: container.kind(),
    value: container instanceof LoroMap ? container.entries() : container.toJSON(),
  };
}

function restoreBlueprint(container: Container, blueprint: ContainerBlueprint): void {
  if (container instanceof LoroMap) {
    for (const [key, value] of blueprint.value as [string, unknown][]) {
      if (isContainer(value)) container.setContainer(key, value);
      else container.set(key, value);
    }
  } else if (container instanceof LoroText) {
    container.applyDelta(blueprint.value as never);
  } else if (container instanceof LoroCounter) {
    if ((blueprint.value as number) !== 0) container.increment(blueprint.value as number);
  } else if (container instanceof LoroList) {
    for (const value of blueprint.value as unknown[]) {
      if (isContainer(value)) container.pushContainer(value);
      else container.push(value);
    }
  }
}

function containerDepth(container: LoroContainer): number {
  let depth = 1;
  let parent = container.parent();
  while (parent !== undefined) {
    depth += 1;
    parent = parent.parent();
  }
  return depth;
}

function assertTextStyleExpand(value: string): asserts value is TextStyleExpand {
  if (value !== "before" && value !== "after" && value !== "none" && value !== "both") {
    throw new TypeError(`invalid text style expand mode: ${value}`);
  }
}

function textStyleInfoByte(expand: TextStyleExpand, deleting: boolean): number {
  const effective = deleting
    ? expand === "none"
      ? "both"
      : expand === "both"
        ? "none"
        : expand
    : expand;
  return (
    0x80 |
    (effective === "before" || effective === "both" ? 0x02 : 0) |
    (effective === "after" || effective === "both" ? 0x04 : 0)
  );
}

function generatePeerId(): bigint {
  const cryptoObject = globalThis.crypto;
  if (cryptoObject !== undefined) {
    const bytes = new Uint8Array(8);
    cryptoObject.getRandomValues(bytes);
    let value = 0n;
    for (const byte of bytes) value = (value << 8n) | BigInt(byte);
    if (value !== 0n) return value;
  }
  return fallbackPeer++;
}

function importStatus(
  records: readonly HistoryRecord[],
  pendingRecords: readonly HistoryRecord[],
): ImportStatus {
  const success = new Map<PeerID, { start: number; end: number }>();
  for (const { change } of records) {
    const peer = peerIdToString(change.id.peer);
    const start = change.id.counter;
    const end = start + changeLength(change);
    const current = success.get(peer);
    success.set(
      peer,
      current === undefined
        ? { start, end }
        : { start: Math.min(start, current.start), end: Math.max(end, current.end) },
    );
  }
  const pending = historySpans(pendingRecords);
  return { success, pending: pending.size === 0 ? null : pending };
}

function importStatusBetweenVersions(
  before: VersionVector,
  retainedStart: VersionVector,
  end: VersionVector,
): ImportStatus {
  const success = new Map<PeerID, { start: number; end: number }>();
  for (const { peer, counter } of end._codecEntriesUnsorted()) {
    const start = Math.max(before.get(peer) ?? 0, retainedStart.get(peer) ?? 0);
    if (counter > start) {
      success.set(peerIdToString(peer), { start, end: counter });
    }
  }
  return { success, pending: null };
}

function historySpans(
  records: readonly HistoryRecord[],
): Map<PeerID, { start: number; end: number }> {
  const spans = new Map<PeerID, { start: number; end: number }>();
  for (const { change } of records) {
    const peer = peerIdToString(change.id.peer);
    const start = change.id.counter;
    const end = start + changeLength(change);
    const current = spans.get(peer);
    spans.set(
      peer,
      current === undefined
        ? { start, end }
        : { start: Math.min(start, current.start), end: Math.max(end, current.end) },
    );
  }
  return spans;
}

function isEmptyJson(value: unknown): boolean {
  return (
    value === "" ||
    (Array.isArray(value) && value.length === 0) ||
    (typeof value === "object" && value !== null && Object.keys(value).length === 0) ||
    value === 0
  );
}

interface TextEventValue {
  readonly text: string;
  readonly delta: ReturnType<LoroText["toDelta"]>;
}

interface TreeEventNode {
  readonly id: TreeID;
  readonly parent: TreeID | undefined;
  readonly index: number;
  readonly fractionalIndex: string;
}

function containerEventValue(container: LoroContainer): unknown {
  if (container instanceof LoroMap) return new Map(container.entries());
  if (container instanceof LoroText) {
    return {
      text: container.toString(),
      delta: container.toDelta(),
    } satisfies TextEventValue;
  }
  if (container instanceof LoroTree) {
    return container.getNodes().map((node) => ({
      id: node.id,
      parent: node.parent()?.id,
      index: node.index(),
      fractionalIndex: node.fractionalIndex() ?? "",
    })) satisfies TreeEventNode[];
  }
  if (container instanceof LoroCounter) return container.value;
  return (container as LoroList).toArray();
}

function containerDiff(
  container: LoroContainer,
  beforeValue: unknown,
  mapOperationKeys: {
    readonly from: ReadonlySet<string> | undefined;
    readonly to: ReadonlySet<string> | undefined;
  } = { from: undefined, to: undefined },
): LoroEvent["diff"] {
  if (container instanceof LoroMap) {
    const before = beforeValue instanceof Map ? beforeValue : new Map<string, unknown>();
    const after = new Map(container.entries());
    const updated: Record<string, unknown> = {};
    const keys = new Set([
      ...before.keys(),
      ...after.keys(),
      ...(mapOperationKeys.from ?? []),
      ...(mapOperationKeys.to ?? []),
    ]);
    for (const key of keys) {
      const previous = before.get(key);
      const next = after.get(key);
      const tombstonePresenceChanged =
        !before.has(key) &&
        !after.has(key) &&
        mapOperationKeys.from?.has(key) !== mapOperationKeys.to?.has(key);
      if (
        tombstonePresenceChanged ||
        !eventValuesEqual(previous, next) ||
        before.has(key) !== after.has(key)
      ) {
        updated[key] = next;
      }
    }
    return { type: "map", updated };
  }
  if (container instanceof LoroText) {
    const before = isTextEventValue(beforeValue)
      ? beforeValue
      : ({ text: "", delta: [] } satisfies TextEventValue);
    const after = containerEventValue(container) as TextEventValue;
    if (eventValuesEqual(before.delta, after.delta)) return { type: "text", diff: [] };
    const styled = [...before.delta, ...after.delta].some(
      (item) => "attributes" in item && item.attributes !== undefined,
    );
    if (styled) {
      return {
        type: "text",
        diff: [
          ...(before.text.length === 0 ? [] : [{ delete: before.text.length } as const]),
          ...after.delta,
        ],
      };
    }
    return { type: "text", diff: stringDelta(before.text, after.text) };
  }
  if (container instanceof LoroTree) {
    return {
      type: "tree",
      diff: treeDelta(
        Array.isArray(beforeValue) ? (beforeValue as TreeEventNode[]) : [],
        containerEventValue(container) as TreeEventNode[],
      ),
    };
  }
  if (container instanceof LoroCounter) {
    return {
      type: "counter",
      increment: container.value - (typeof beforeValue === "number" ? beforeValue : 0),
    };
  }
  return {
    type: "list",
    diff: listDelta(
      Array.isArray(beforeValue) ? beforeValue : [],
      (container as LoroList).toArray(),
    ),
  };
}

function diffForJson(diff: Diff): JsonDiff {
  if (diff.type === "map") {
    return {
      type: "map",
      updated: Object.fromEntries(
        Object.entries(diff.updated).map(([key, value]) => [key, jsonDiffValue(value)]),
      ),
    };
  }
  if (diff.type === "list") {
    return {
      type: "list",
      diff: diff.diff.map((operation) =>
        "insert" in operation
          ? { ...operation, insert: operation.insert.map(jsonDiffValue) }
          : operation,
      ),
    };
  }
  return diff;
}

function jsonDiffValue(value: unknown): Value {
  if (isContainer(value)) return `🦜:${value.id}` as JsonContainerID;
  if (value instanceof Uint8Array || value === null) return value;
  if (Array.isArray(value)) return value.map(jsonDiffValue);
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value).map(([key, child]) => [key, jsonDiffValue(child)]),
    );
  }
  return value as Value;
}

function stringDelta(before: string, after: string): Delta<string>[] {
  const beforeCharacters = Array.from(before);
  const afterCharacters = Array.from(after);
  let prefix = 0;
  while (
    prefix < beforeCharacters.length &&
    prefix < afterCharacters.length &&
    beforeCharacters[prefix] === afterCharacters[prefix]
  ) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < beforeCharacters.length - prefix &&
    suffix < afterCharacters.length - prefix &&
    beforeCharacters[beforeCharacters.length - suffix - 1] ===
      afterCharacters[afterCharacters.length - suffix - 1]
  ) {
    suffix += 1;
  }
  const diff: Delta<string>[] = [];
  const retained = beforeCharacters.slice(0, prefix).join("").length;
  if (retained > 0) diff.push({ retain: retained });
  const deleted = beforeCharacters
    .slice(prefix, beforeCharacters.length - suffix)
    .join("").length;
  if (deleted > 0) diff.push({ delete: deleted });
  const inserted = afterCharacters
    .slice(prefix, afterCharacters.length - suffix)
    .join("");
  if (inserted.length > 0) diff.push({ insert: inserted });
  return diff;
}

function listDelta(
  before: readonly unknown[],
  after: readonly unknown[],
): Delta<unknown[]>[] {
  let prefix = 0;
  while (
    prefix < before.length &&
    prefix < after.length &&
    eventValuesEqual(before[prefix], after[prefix])
  ) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < before.length - prefix &&
    suffix < after.length - prefix &&
    eventValuesEqual(before[before.length - suffix - 1], after[after.length - suffix - 1])
  ) {
    suffix += 1;
  }
  const diff: Delta<unknown[]>[] = [];
  if (prefix > 0) diff.push({ retain: prefix });
  const deleted = before.length - prefix - suffix;
  if (deleted > 0) diff.push({ delete: deleted });
  const inserted = after.slice(prefix, after.length - suffix);
  if (inserted.length > 0) diff.push({ insert: [...inserted] });
  return diff;
}

function treeDelta(
  before: readonly TreeEventNode[],
  after: readonly TreeEventNode[],
): TreeDiffItem[] {
  const beforeById = new Map(before.map((node) => [node.id, node]));
  const afterById = new Map(after.map((node) => [node.id, node]));
  const diff: TreeDiffItem[] = [];
  for (const node of before) {
    if (!afterById.has(node.id)) {
      diff.push({
        target: node.id,
        action: "delete",
        oldParent: node.parent,
        oldIndex: node.index,
      });
    }
  }
  for (const node of after) {
    const previous = beforeById.get(node.id);
    if (previous === undefined) {
      diff.push({
        target: node.id,
        action: "create",
        parent: node.parent,
        index: node.index,
        fractionalIndex: node.fractionalIndex,
      });
    } else if (
      previous.parent !== node.parent ||
      previous.index !== node.index ||
      previous.fractionalIndex !== node.fractionalIndex
    ) {
      diff.push({
        target: node.id,
        action: "move",
        parent: node.parent,
        index: node.index,
        fractionalIndex: node.fractionalIndex,
        oldParent: previous.parent,
        oldIndex: previous.index,
      });
    }
  }
  return diff;
}

function eventValuesEqual(left: unknown, right: unknown): boolean {
  if (left === right) return true;
  if (left instanceof Uint8Array && right instanceof Uint8Array) {
    return bytesEqual(left, right);
  }
  if (isContainer(left) && isContainer(right)) return left.id === right.id;
  if (Array.isArray(left) && Array.isArray(right)) {
    return (
      left.length === right.length &&
      left.every((value, index) => eventValuesEqual(value, right[index]))
    );
  }
  if (left instanceof Map && right instanceof Map) {
    return (
      left.size === right.size &&
      [...left].every(
        ([key, value]) => right.has(key) && eventValuesEqual(value, right.get(key)),
      )
    );
  }
  if (
    typeof left === "object" &&
    left !== null &&
    typeof right === "object" &&
    right !== null
  ) {
    const leftEntries = Object.entries(left);
    const rightRecord = right as Record<string, unknown>;
    return (
      leftEntries.length === Object.keys(rightRecord).length &&
      leftEntries.every(
        ([key, value]) => key in rightRecord && eventValuesEqual(value, rightRecord[key]),
      )
    );
  }
  return false;
}

function subtractSequenceIdRuns(
  runs: readonly SequenceIdRun[],
  removedRuns: readonly SequenceIdRun[],
): SequenceIdRun[] {
  if (removedRuns.length === 0) return [...runs];
  const removalsByPeer = new Map<bigint, { start: number; end: number }[]>();
  for (const run of removedRuns) {
    let removals = removalsByPeer.get(run.start.peer);
    if (removals === undefined) {
      removals = [];
      removalsByPeer.set(run.start.peer, removals);
    }
    removals.push({
      start: run.start.counter,
      end: run.start.counter + run.length,
    });
  }
  for (const removals of removalsByPeer.values()) {
    removals.sort((left, right) => left.start - right.start);
    let write = 0;
    for (const removal of removals) {
      const previous = removals[write - 1];
      if (previous !== undefined && removal.start <= previous.end) {
        previous.end = Math.max(previous.end, removal.end);
      } else {
        removals[write++] = removal;
      }
    }
    removals.length = write;
  }

  const retained: SequenceIdRun[] = [];
  for (const run of runs) {
    const removals = removalsByPeer.get(run.start.peer);
    if (removals === undefined) {
      retained.push(run);
      continue;
    }
    const end = run.start.counter + run.length;
    let low = 0;
    let high = removals.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (removals[middle]!.end <= run.start.counter) low = middle + 1;
      else high = middle;
    }
    let cursor = run.start.counter;
    for (let index = low; index < removals.length; index += 1) {
      const removal = removals[index]!;
      if (removal.start >= end) break;
      if (cursor < removal.start) {
        retained.push({
          start: { peer: run.start.peer, counter: cursor },
          length: Math.min(removal.start, end) - cursor,
        });
      }
      cursor = Math.max(cursor, removal.end);
      if (cursor >= end) break;
    }
    if (cursor < end) {
      retained.push({
        start: { peer: run.start.peer, counter: cursor },
        length: end - cursor,
      });
    }
  }
  return retained;
}

function normalizeComparableValue(value: unknown): unknown {
  if (value === undefined) return null;
  if (value instanceof Uint8Array || isContainer(value)) return value;
  if (Array.isArray(value)) return value.map(normalizeComparableValue);
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value).map(([key, child]) => [key, normalizeComparableValue(child)]),
    );
  }
  return value;
}

function isTextEventValue(value: unknown): value is TextEventValue {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as TextEventValue).text === "string" &&
    Array.isArray((value as TextEventValue).delta)
  );
}

function isEmptyContainerDiff(diff: LoroEvent["diff"]): boolean {
  if (diff.type === "map") return Object.keys(diff.updated).length === 0;
  if (diff.type === "counter") return diff.increment === 0;
  return diff.diff.length === 0;
}

function recoverParentBinding(
  child: LoroContainer,
  parent: LoroContainer,
):
  | { readonly kind: "map"; readonly key: string }
  | { readonly kind: "sequence"; readonly element: SequenceElement }
  | { readonly kind: "tree"; readonly record: TreeNodeRecord }
  | undefined {
  if (parent instanceof LoroMap) {
    for (const [key, record] of parent._entries) {
      if (record.value !== child) continue;
      const binding = { kind: "map" as const, key };
      child._setParentBinding(parent, binding);
      return binding;
    }
  } else if (parent instanceof LoroList) {
    for (const element of parent._sequence.all()) {
      if (element.value !== child) continue;
      const binding = { kind: "sequence" as const, element };
      child._setParentBinding(parent, binding);
      return binding;
    }
  } else if (parent instanceof LoroTree) {
    for (const record of parent._nodes.values()) {
      if (record.data !== child) continue;
      const binding = { kind: "tree" as const, record };
      child._setParentBinding(parent, binding);
      return binding;
    }
  }
  return undefined;
}

function containerPath(container: LoroContainer): Path {
  const path: Path = [];
  let current: LoroContainer | undefined = container;
  while (current !== undefined) {
    const id = current._codecId;
    if (id?.kind === "root" && !isMergeableContainerId(id)) {
      path.unshift(id.name);
      break;
    }
    const parent = current.parent();
    const binding =
      current._parentLink?.binding ??
      (parent === undefined ? undefined : recoverParentBinding(current, parent));
    if (parent instanceof LoroMap) {
      if (binding?.kind === "map") {
        path.unshift(binding.key);
      }
    } else if (parent instanceof LoroList) {
      const index =
        binding?.kind === "sequence"
          ? parent._sequence.visibleIndexOf(binding.element)
          : undefined;
      if (index !== undefined && index >= 0) path.unshift(index);
    } else if (parent instanceof LoroTree) {
      if (binding?.kind === "tree") path.unshift(formatTreeId(binding.record.id));
      else if (id?.kind === "normal") path.unshift(formatTreeId(id));
    }
    current = parent;
  }
  return path;
}

function parseOptionalPathIndex(value: string): number | undefined {
  if (!/^(0|[1-9]\d*)$/u.test(value)) return undefined;
  const index = Number(value);
  return Number.isSafeInteger(index) ? index : undefined;
}

function parsePathIndex(value: string): number {
  return parseOptionalPathIndex(value) ?? -1;
}

function treeNodeAtPath(
  tree: LoroTree,
  _parent: LoroTreeNode | undefined,
  part: string,
): LoroTreeNode | undefined {
  if (part.includes("@")) return tree.getNodeByID(part as TreeID);
  const index = parseOptionalPathIndex(part);
  return index === undefined ? undefined : tree._nodeAt(undefined, index);
}
