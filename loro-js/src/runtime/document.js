import packageMetadata from "../../package.json" with { type: "json" };
import { bytesEqual, bytesToHex, hexToBytes } from "../codec/bytes";
import { decodeChangeBlock, encodeSingleChangeBlock, validateChangeBlock, } from "../codec/change-block-codec";
import { containerTypeFromRawByte, containerTypeToRawByte } from "../codec/container-id";
import { decodeDocument, decodeFastSnapshotBody, decodeFastUpdatesBody, encodeDocument, encodeFastSnapshotBody, encodeFastUpdatesBody, EncodeMode, } from "../codec/document";
import { decodeChangeBlockKey, encodeChangeBlockKey } from "../codec/id";
import { decodeSstable, encodeSstable } from "../codec/sstable";
import { decodeStateSnapshotStore, encodeStateSnapshotStore, } from "../codec/state-snapshot";
import { ContainerType as CodecContainerType, } from "../codec/types";
import { decodePostcardFrontiers, decodePostcardVersionVector, encodePostcardFrontiers, encodePostcardVersionVector, } from "../codec/version";
import { cloneRuntimeValue, containerDeepValueWithId, Cursor, isContainer, LoroCounter, LoroList, LoroMap, LoroMovableList, LoroText, LoroTree, LoroTreeNode, runtimeValueToJson, } from "./containers";
import { SequenceEventDiff } from "./event-diff";
import { codecTypeToPublic, containerIdsEqual, formatContainerId, formatOpId, formatTreeId, idsEqual, isContainerId, parseContainerId, parseOpId, parsePeerId, parseTreeId, peerIdToString, publicTypeToCodec, } from "./ids";
import { compileJsonPathEventMatcher, evaluateJsonPath } from "./jsonpath";
import { isMergeableContainerId, mergeableMarker, newMergeableContainerId, parseMergeableMarker, } from "./mergeable";
import { OrderedIndex } from "./ordered-index";
import { VersionVector } from "./version-vector";
const VERSION_KEY = Uint8Array.of(0x76, 0x76);
const FRONTIERS_KEY = Uint8Array.of(0x66, 0x72);
const START_VERSION_KEY = Uint8Array.of(0x73, 0x76);
const START_FRONTIERS_KEY = Uint8Array.of(0x73, 0x66);
let fallbackPeer = 1n;
// Scratch buffer for Counter snapshot float64 <-> bits conversion. Used
// synchronously and never retained.
const float64BitsScratch = new DataView(new ArrayBuffer(8));
// Shared item for MovableList snapshots; the state-snapshot encoder only
// reads item fields, so every slot can reference this frozen object.
const SHARED_MOVABLE_LIST_STATE_ITEM = Object.freeze({
    invisibleListItems: 0n,
    positionIdEqualsElementId: true,
    elementIdEqualsLastSetId: true,
});
// Scratch single-run list reused for synchronous, non-retaining SequenceIndex
// queries (containsIdRuns / canShowIdRunsAt). Those callees read the runs
// synchronously and never store them, so reuse is safe on this
// single-threaded runtime. Never pass these objects to anything that retains
// them.
const scratchSequenceIdRun = { start: { peer: 0n, counter: 0 }, length: 0 };
const scratchSequenceIdRuns = [scratchSequenceIdRun];
export class LoroDoc {
    #peer = generatePeerId();
    #history = new Map();
    #historyOrder = new OrderedIndex(compareHistoryRecords);
    #historyByPeer = new Map();
    #historyEndByPeer = new Map();
    #historyOperationCount = 0;
    #sortedHistoryCache;
    #historyFrontiers = new Map();
    #dependencyVersionCache = new WeakMap();
    #mapOperationHistory = new Map();
    #treeOperationHistory = new Map();
    #movableOrderHistory = new Map();
    #movableMovePeers = new Map();
    #containersWithOperations = new Set();
    #containerKeys = new WeakMap();
    #pendingHistory = new Map();
    #deferredSnapshotHistory;
    #containers = new Map();
    #roots = new Map();
    #pending;
    #nextCounter = 0;
    #recordTimestamp = false;
    #changeMergeInterval = 1000n;
    #nextCommitOptions = {};
    #subscribers = new Set();
    #localUpdateSubscribers = new Set();
    #containerSubscribers = new Map();
    #firstCommitSubscribers = new Set();
    #preCommitSubscribers = new Set();
    #seenCommittedPeers = new Set();
    #committing = false;
    #preCommitRecord;
    #detached = false;
    #detachedEditing = false;
    #checkoutVersion;
    #hideEmptyRoots = false;
    #shallowStartVersion = new VersionVector();
    #shallowRootVersion = new VersionVector();
    #shallowRootFrontiers = [];
    #shallowRootStore;
    #textStyles = new Map([
        ["bold", "after"],
        ["italic", "after"],
        ["underline", "after"],
        ["link", "none"],
        ["highlight", "none"],
        ["comment", "none"],
        ["code", "none"],
    ]);
    #defaultTextStyle;
    static fromSnapshot(snapshot) {
        const doc = new LoroDoc();
        doc.import(snapshot);
        return doc;
    }
    free() { }
    get peerId() {
        return this.#peer;
    }
    get peerIdStr() {
        return peerIdToString(this.#peer);
    }
    setPeerId(peer) {
        if (this.#pending !== undefined)
            throw new Error("cannot change peer id with pending operations");
        this.#peer = parsePeerId(peer);
        this.#nextCounter = this.oplogVersion().get(this.#peer) ?? 0;
    }
    setRecordTimestamp(enabled) {
        this.#recordTimestamp = enabled;
    }
    setChangeMergeInterval(interval) {
        this.#changeMergeInterval = numberToI64(interval);
    }
    setDetachedEditing(enabled) {
        this.#detachedEditing = enabled;
        if (enabled && this.#detached) {
            this.#commit({}, true);
            this.#renewPeerId();
        }
    }
    isDetachedEditingEnabled() {
        return this.#detachedEditing;
    }
    configTextStyle(styles) {
        this.#textStyles.clear();
        for (const [key, style] of Object.entries(styles)) {
            if (key.includes(":"))
                throw new TypeError("text style keys cannot contain ':'");
            assertTextStyleExpand(style.expand);
            this.#textStyles.set(key, style.expand);
        }
    }
    configDefaultTextStyle(style) {
        if (style === undefined) {
            this.#defaultTextStyle = undefined;
            return;
        }
        assertTextStyleExpand(style.expand);
        this.#defaultTextStyle = style.expand;
    }
    getMap(name) {
        return this.#getContainer(name, "Map");
    }
    getList(name) {
        return this.#getContainer(name, "List");
    }
    getMovableList(name) {
        return this.#getContainer(name, "MovableList");
    }
    getText(name) {
        return this.#getContainer(name, "Text");
    }
    getTree(name) {
        return this.#getContainer(name, "Tree");
    }
    getCounter(name) {
        return this.#getContainer(name, "Counter");
    }
    getContainerById(id) {
        const parsed = parseContainerId(id);
        if (parsed.kind === "root" && !isMergeableContainerId(parsed)) {
            return this.#getOrCreateContainer(parsed);
        }
        const container = this.#containers.get(formatContainerId(parsed));
        if (container !== undefined &&
            isMergeableContainerId(parsed) &&
            this._isContainerDeleted(container) &&
            !this.#containerHasOperations(parsed)) {
            return undefined;
        }
        return container;
    }
    hasContainer(id) {
        return this.getContainerById(id) !== undefined;
    }
    #containerHasOperations(id) {
        this.#materializeDeferredHistory();
        return this.#containersWithOperations.has(formatContainerId(id));
    }
    deleteRootContainer(id) {
        const parsed = parseContainerId(id);
        if (parsed.kind !== "root")
            throw new TypeError("only root containers can be deleted directly");
        this.#roots.delete(parsed.name);
        this.#containers.delete(formatContainerId(parsed));
    }
    setHideEmptyRootContainers(hide) {
        this.#hideEmptyRoots = hide;
    }
    commit(options = {}) {
        this.#commit(options, false);
    }
    #commit(options, preserveOptionsOnEmpty) {
        if (this.#committing)
            return;
        const pending = this.#pending;
        if (pending === undefined || pending.operations.length === 0) {
            if (!preserveOptionsOnEmpty)
                this.#nextCommitOptions = {};
            return;
        }
        const mergedOptions = {
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
            if (this.#preCommitSubscribers.size > 0) {
                const initialTimestamp = maxBigInt(latestTimestamp, numberToI64(mergedOptions.timestamp ?? (this.#recordTimestamp ? Date.now() / 1000 : 0)));
                const provisional = {
                    id: pending.id,
                    dependencies: pending.dependencies,
                    lamport: pending.lamport,
                    timestamp: initialTimestamp,
                    message: mergedOptions.message,
                    operations: pending.operations,
                };
                const modifier = new ChangeModifier(mergedOptions);
                this.#preCommitRecord = {
                    change: provisional,
                    keys: pending.keys,
                    keyIndices: undefined,
                };
                try {
                    for (const listener of this.#preCommitSubscribers) {
                        listener({
                            changeMeta: publicChange(provisional),
                            origin: mergedOptions.origin ?? "",
                            modifier,
                        });
                    }
                }
                finally {
                    this.#preCommitRecord = undefined;
                }
            }
            const atomLength = pending.operationLength;
            const timestamp = maxBigInt(latestTimestamp, numberToI64(mergedOptions.timestamp ?? (this.#recordTimestamp ? Date.now() / 1000 : 0)));
            const change = {
                id: pending.id,
                dependencies: pending.dependencies,
                lamport: pending.lamport,
                timestamp,
                message: mergedOptions.message,
                operations: pending.operations,
            };
            this.#pending = undefined;
            const committedRecord = {
                change,
                keys: pending.keys,
                keyIndices: undefined,
            };
            const previous = this.#mergeablePreviousRecord(change, this.#changeMergeInterval);
            let updateRecord = committedRecord;
            if (previous === undefined) {
                this.#setHistoryRecord(changeKey(change.id), committedRecord, committedRecord);
            }
            else {
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
                for (const listener of this.#localUpdateSubscribers)
                    listener(bytes.slice());
            }
            if (this.#hasEventSubscribers()) {
                this.#emit("local", mergedOptions.origin, pending.from, this.#frontiersCodec(), pending.changedContainers, pending.beforeValues, this.#recordedEventDiffs(pending));
            }
        }
        catch (error) {
            if (isFirstCommit)
                this.#seenCommittedPeers.delete(pending.id.peer);
            throw error;
        }
        finally {
            this.#committing = false;
        }
    }
    getPendingTxnLength() {
        return this.#pending?.operationLength ?? 0;
    }
    setNextCommitMessage(message) {
        this.#nextCommitOptions = { ...this.#nextCommitOptions, message };
    }
    setNextCommitOrigin(origin) {
        this.#nextCommitOptions = { ...this.#nextCommitOptions, origin };
    }
    setNextCommitTimestamp(timestamp) {
        this.#nextCommitOptions = { ...this.#nextCommitOptions, timestamp };
    }
    setNextCommitOptions(options) {
        this.#nextCommitOptions = { ...options };
    }
    clearNextCommitOptions() {
        this.#nextCommitOptions = {};
    }
    export(mode) {
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
    exportJsonUpdates(start, end, withPeerCompression = true) {
        this.#commit({}, true);
        const startVersion = new VersionVector(start);
        const endVersion = end === undefined || end === null ? this.#historyVersion() : new VersionVector(end);
        const records = this.#recordsInVersionRange(startVersion, endVersion);
        return historyRecordsToJsonSchema(records, startVersion, withPeerCompression);
    }
    exportJsonInIdSpan(span) {
        this.#commit({}, true);
        const records = this.#recordsInSpans([
            { id: { peer: span.peer, counter: span.counter }, len: span.length },
        ]);
        return historyRecordsToJsonSchema(records, new VersionVector(), false, false).changes;
    }
    importJsonUpdates(input) {
        const parsed = typeof input === "string" ? JSON.parse(input) : input;
        if (parsed.schema_version !== 1) {
            throw new TypeError(`unsupported JSON schema version ${String(parsed.schema_version)}`);
        }
        const records = jsonSchemaToHistoryRecords(parsed);
        return this.import(this.#encodeUpdates(records));
    }
    import(bytes) {
        this.#commit({}, true);
        // Event inputs are only consumed by #emit, which returns immediately when
        // nobody listens, so skip building them in that case.
        const emitting = this.#hasEventSubscribers();
        const before = emitting ? this.#frontiersCodec() : [];
        const beforeVersion = this.#historyVersion();
        const parsed = decodeDocument(bytes);
        if (this.#deferredSnapshotHistory !== undefined) {
            this.#materializeDeferredHistory();
        }
        const imported = [];
        let integration = { added: [], pending: [] };
        let beforeValues = new Map();
        let preparedDiffs = new Map();
        let deferredChanged;
        let deferredStatus;
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
                if (recording !== undefined) {
                    this.#recordConcurrentTreeSubjects(recording, added, beforeVersion);
                }
                this.#applyRecords(added, recording);
                this.#canonicalizeImportedMovableMoves(added, recording);
                if (recording !== undefined) {
                    beforeValues = recording.beforeValues;
                    preparedDiffs = this.#recordedEventDiffs(recording);
                }
            }
        }
        else {
            const snapshot = decodeFastSnapshotBody(parsed.body);
            const oplogEntries = decodeSstable(snapshot.oplog.slice());
            let encodedStartVersion;
            let encodedStartFrontiers;
            let encodedEndVersion;
            let encodedEndFrontiers;
            for (const entry of oplogEntries) {
                if (bytesEqual(entry.key, START_VERSION_KEY)) {
                    encodedStartVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
                }
                else if (bytesEqual(entry.key, START_FRONTIERS_KEY)) {
                    encodedStartFrontiers = decodePostcardFrontiers(entry.value);
                }
                else if (bytesEqual(entry.key, VERSION_KEY)) {
                    encodedEndVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
                }
                else if (bytesEqual(entry.key, FRONTIERS_KEY)) {
                    encodedEndFrontiers = decodePostcardFrontiers(entry.value);
                }
            }
            const initializeFromSnapshot = this.#history.size === 0 &&
                this.#pendingHistory.size === 0 &&
                this.#shallowRootStore === undefined;
            const isShallowSnapshot = snapshot.shallowRootState.length > 0;
            let rootStore;
            let stagedShallowStartVersion;
            let stagedShallowRootVersion;
            let stagedShallowRootFrontiers;
            let canonicalStartMetadata = !isShallowSnapshot &&
                encodedStartVersion === undefined &&
                encodedStartFrontiers === undefined;
            if (initializeFromSnapshot && isShallowSnapshot) {
                rootStore = decodeStateSnapshotStore(snapshot.shallowRootState.slice());
                if (rootStore.kind !== "sstable") {
                    throw new Error("shallow snapshot root state must be an SSTable");
                }
                if (rootStore.frontiers !== undefined &&
                    encodedStartFrontiers !== undefined &&
                    !frontierSetsEqual(rootStore.frontiers.map(formatOpId), encodedStartFrontiers.map(formatOpId))) {
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
            const hydratedStore = rootStore === undefined
                ? stateStore
                : mergeStateSnapshotStores(rootStore, stateStore);
            const preparedTextSnapshotIds = validateStateSnapshotTextIds(hydratedStore);
            if (stagedShallowRootVersion !== undefined &&
                encodedEndVersion !== undefined &&
                !versionIncludes(encodedEndVersion, stagedShallowRootVersion)) {
                throw new Error("shallow snapshot root version exceeds its end version");
            }
            const installStagedShallowRoot = () => {
                if (rootStore === undefined ||
                    stagedShallowStartVersion === undefined ||
                    stagedShallowRootVersion === undefined ||
                    stagedShallowRootFrontiers === undefined) {
                    return;
                }
                this.#shallowStartVersion = stagedShallowStartVersion;
                this.#shallowRootVersion = stagedShallowRootVersion;
                this.#shallowRootFrontiers = stagedShallowRootFrontiers;
                this.#shallowRootStore = rootStore;
            };
            const canDeferHistory = initializeFromSnapshot &&
                !this.#detached &&
                canonicalStartMetadata &&
                encodedEndVersion !== undefined &&
                encodedEndFrontiers !== undefined &&
                stateStore.kind === "sstable" &&
                hydratedStore.kind === "sstable";
            if (canDeferHistory) {
                const endVersion = encodedEndVersion;
                const endFrontiers = encodedEndFrontiers;
                this.#validateDeferredFrontierBlocks(oplogEntries, endVersion, endFrontiers);
                installStagedShallowRoot();
                if (emitting) {
                    deferredChanged = new Set(hydratedStore.containers.map(({ id }) => formatContainerId(id)));
                    beforeValues = this.#captureContainerEventValues(deferredChanged);
                }
                this.#hydrateState(hydratedStore, preparedTextSnapshotIds);
                this.#deferredSnapshotHistory = {
                    entries: oplogEntries,
                    endVersion,
                    frontiers: endFrontiers.map((id) => ({ ...id })),
                    operationCount: versionDistance(stagedShallowStartVersion ?? this.#shallowStartVersion, endVersion),
                };
                for (const { peer } of endVersion._codecEntriesUnsorted()) {
                    this.#seenCommittedPeers.add(peer);
                }
                deferredStatus = importStatusBetweenVersions(beforeVersion, stagedShallowStartVersion ?? this.#shallowStartVersion, endVersion);
            }
            else {
                for (const entry of oplogEntries) {
                    if (entry.key.length !== 12)
                        continue;
                    const expected = decodeChangeBlockKey(entry.key);
                    const records = this.#readChangeBlock(entry.value);
                    if (records[0] !== undefined && !idsEqual(records[0].change.id, expected)) {
                        throw new Error("snapshot change key does not match its block");
                    }
                    imported.push(...records);
                }
                installStagedShallowRoot();
                if (!initializeFromSnapshot)
                    this.#assertImportsNotOutdated(imported);
                integration = this.#integrateHistory(imported);
                if (!this.#detached && initializeFromSnapshot) {
                    if (this.#hasEventSubscribers()) {
                        beforeValues = this.#captureEventValues(integration.added);
                    }
                    if (rootStore !== undefined && stateStore.kind === "empty") {
                        this.#rebuildFromHistory();
                    }
                    else {
                        this.#hydrateState(hydratedStore, preparedTextSnapshotIds);
                    }
                }
                else if (!this.#detached && integration.added.length > 0) {
                    const recording = this.#hasEventSubscribers()
                        ? { beforeValues: new Map(), eventStates: new Map() }
                        : undefined;
                    if (recording !== undefined) {
                        this.#recordConcurrentTreeSubjects(recording, integration.added, beforeVersion);
                    }
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
        const changed = this.#detached || !emitting
            ? new Set()
            : (deferredChanged ??
                new Set(integration.added.flatMap(({ change }) => change.operations.map((op) => formatContainerId(op.container)))));
        this.#emit("import", undefined, before, emitting ? this.#frontiersCodec() : [], changed, beforeValues, preparedDiffs);
        return deferredStatus ?? importStatus(integration.added, integration.pending);
    }
    importBatch(blobs) {
        if (blobs.length === 0)
            return { success: new Map(), pending: null };
        if (blobs.length === 1)
            return this.import(blobs[0]);
        this.#commit({}, true);
        this.#materializeDeferredHistory();
        const emitting = this.#hasEventSubscribers();
        const before = emitting ? this.#frontiersCodec() : [];
        const ordered = blobs
            .map((blob) => this.#decodeImportData(decodeDocument(blob)))
            .sort((left, right) => Number(left.mode === EncodeMode.FastUpdates) -
            Number(right.mode === EncodeMode.FastUpdates) ||
            right.records.length - left.records.length);
        const initializeFromSnapshot = this.#history.size === 0 &&
            this.#pendingHistory.size === 0 &&
            this.#shallowRootStore === undefined;
        const snapshotSeed = initializeFromSnapshot
            ? ordered.find(({ snapshot }) => snapshot !== undefined)
            : undefined;
        let rootStore;
        let stagedShallowRoot;
        if (snapshotSeed?.snapshot !== undefined &&
            snapshotSeed.snapshot.shallowRootState.length > 0) {
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
            stagedShallowRoot = {
                startVersion,
                rootVersion,
                frontiers: startFrontiers.map((id) => ({ ...id })),
            };
        }
        let snapshotStateStore;
        let snapshotHydratedStore;
        let preparedTextSnapshotIds;
        if (snapshotSeed?.snapshot !== undefined) {
            snapshotStateStore = decodeStateSnapshotStore(snapshotSeed.snapshot.state);
            snapshotHydratedStore =
                rootStore === undefined
                    ? snapshotStateStore
                    : mergeStateSnapshotStores(rootStore, snapshotStateStore);
            preparedTextSnapshotIds = validateStateSnapshotTextIds(snapshotHydratedStore);
        }
        if (rootStore !== undefined && stagedShallowRoot !== undefined) {
            this.#shallowStartVersion = stagedShallowRoot.startVersion;
            this.#shallowRootVersion = stagedShallowRoot.rootVersion;
            this.#shallowRootFrontiers = stagedShallowRoot.frontiers;
            this.#shallowRootStore = rootStore;
        }
        for (const decoded of ordered) {
            if (decoded !== snapshotSeed)
                this.#assertImportsNotOutdated(decoded.records);
        }
        const integration = this.#integrateHistory(ordered.flatMap(({ records }) => records));
        let beforeValues = new Map();
        let preparedDiffs = new Map();
        if (integration.added.length > 0 && !this.#detached) {
            if (snapshotSeed?.snapshot !== undefined) {
                if (this.#hasEventSubscribers()) {
                    beforeValues = this.#captureEventValues(integration.added);
                }
                if (rootStore !== undefined && snapshotStateStore.kind === "empty") {
                    this.#rebuildFromHistory();
                }
                else {
                    this.#hydrateState(snapshotHydratedStore, preparedTextSnapshotIds);
                    const snapshotVersion = snapshotSeed.endVersion?.clone() ??
                        historyVersionForRecords(snapshotSeed.records, snapshotSeed.startVersion);
                    const forwardRecords = this.#recordsInVersionRange(snapshotVersion, this.#historyVersion());
                    this.#applyRecords(forwardRecords);
                    this.#canonicalizeImportedMovableMoves(forwardRecords);
                }
            }
            else {
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
        const changed = this.#detached || !emitting
            ? new Set()
            : changedContainerIds(integration.added);
        this.#emit("import", undefined, before, emitting ? this.#frontiersCodec() : [], changed, beforeValues, preparedDiffs);
        return importStatus(integration.added, integration.pending);
    }
    importUpdateBatch(blobs) {
        return this.importBatch(blobs);
    }
    toJSON() {
        const output = {};
        for (const [name, container] of this.#roots) {
            const value = container.toJSON();
            if (this.#hideEmptyRoots && isEmptyJson(value))
                continue;
            output[name] = value;
        }
        return output;
    }
    toJsonWithReplacer(replacer) {
        const processed = new Set();
        const run = (value) => {
            if (Array.isArray(value)) {
                return value.flatMap((item, index) => {
                    const next = visit(index, item);
                    return next === undefined ? [] : [next];
                });
            }
            if (typeof value === "object" && value !== null && !(value instanceof Uint8Array)) {
                return Object.fromEntries(Object.entries(value).flatMap(([key, item]) => {
                    const next = visit(key, item);
                    return next === undefined ? [] : [[key, next]];
                }));
            }
            return value;
        };
        const visit = (key, value) => {
            if (typeof value === "string" &&
                isContainerId(value) &&
                !processed.has(value)) {
                const id = value;
                processed.add(id);
                const container = this.getContainerById(id);
                if (container === undefined)
                    throw new RangeError(`container ${id} does not exist`);
                const replaced = replacer(key, container);
                if (replaced === container)
                    return run(container.getShallowValue());
                if (isContainer(replaced)) {
                    throw new TypeError("replacer cannot substitute a different container");
                }
                return replaced === undefined ? undefined : run(replaced);
            }
            if (typeof value === "object" && value !== null && !(value instanceof Uint8Array)) {
                return run(value);
            }
            const replaced = replacer(key, value);
            if (isContainer(replaced)) {
                throw new TypeError("replacer cannot introduce a container");
            }
            return replaced;
        };
        return run(this.getShallowValue());
    }
    diff(from, to, forJson = true) {
        this.#commit({}, true);
        const fromVersion = this.#versionForExistingFrontiers(from);
        const toVersion = this.#versionForExistingFrontiers(to);
        this.#assertVersionNotBeforeShallowRoot(fromVersion);
        this.#assertVersionNotBeforeShallowRoot(toVersion);
        if (fromVersion.compare(toVersion) === 0)
            return [];
        const restoreVersion = this.version();
        const currentToFromForward = this.#recordsInVersionRange(restoreVersion, fromVersion);
        const currentToFromRetreat = this.#recordsInVersionRange(fromVersion, restoreVersion);
        const forwardRecords = this.#recordsInVersionRange(fromVersion, toVersion);
        const retreatRecords = this.#recordsInVersionRange(toVersion, fromVersion);
        const currentToFromMoveMode = movableMoveTransitionMode(currentToFromRetreat, currentToFromForward, this.#movableMovePeers);
        const fromToToMoveMode = movableMoveTransitionMode(retreatRecords, forwardRecords, this.#movableMovePeers);
        const useIncrementalTransition = this.#canTransitionRecords([...currentToFromRetreat, ...currentToFromForward], currentToFromMoveMode) &&
            this.#canTransitionRecords([...retreatRecords, ...forwardRecords], fromToToMoveMode);
        let materializedVersion = restoreVersion;
        let transitionFailed = false;
        try {
            if (materializedVersion.compare(fromVersion) !== 0) {
                if (useIncrementalTransition) {
                    this.#applyVersionTransition(currentToFromRetreat, currentToFromForward, fromVersion, undefined, currentToFromMoveMode);
                }
                else {
                    this.#rebuildFromHistory(fromVersion);
                }
                materializedVersion = fromVersion;
            }
            const changed = changedContainerIds([...forwardRecords, ...retreatRecords]);
            let before = new Map();
            let calculated = new Map();
            let mapKeysAtFrom = new Map();
            let mapKeysAtTo = new Map();
            if (useIncrementalTransition || retreatRecords.length === 0) {
                const recording = {
                    beforeValues: new Map(),
                    eventStates: new Map(),
                };
                if (useIncrementalTransition) {
                    this.#applyVersionTransition(retreatRecords, forwardRecords, toVersion, recording, fromToToMoveMode);
                }
                else {
                    this.#applyRecords(forwardRecords, recording);
                }
                before = recording.beforeValues;
                calculated = this.#recordedEventDiffs(recording, true);
            }
            else {
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
                const diff = calculated.get(container.id) ??
                    containerDiff(container, before.get(container.id), {
                        from: mapKeysAtFrom.get(container.id),
                        to: mapKeysAtTo.get(container.id),
                    });
                if (isEmptyContainerDiff(diff))
                    return [];
                return [
                    [container.id, forJson ? diffForJson(diff) : diff],
                ];
            });
        }
        catch (error) {
            transitionFailed = useIncrementalTransition;
            throw error;
        }
        finally {
            if (transitionFailed) {
                this.#rebuildFromHistory(restoreVersion);
            }
            else if (materializedVersion.compare(restoreVersion) !== 0) {
                if (useIncrementalTransition) {
                    if (materializedVersion.compare(toVersion) === 0) {
                        this.#applyVersionTransition(forwardRecords, retreatRecords, fromVersion, undefined, fromToToMoveMode);
                        materializedVersion = fromVersion;
                    }
                    if (materializedVersion.compare(restoreVersion) !== 0) {
                        this.#applyVersionTransition(currentToFromForward, currentToFromRetreat, restoreVersion, undefined, currentToFromMoveMode);
                    }
                }
                else {
                    this.#rebuildFromHistory(restoreVersion);
                }
            }
        }
    }
    applyDiff(diffBatch) {
        if (!Array.isArray(diffBatch))
            throw new TypeError("diff batch must be an array");
        if (this.#detached && !this.#detachedEditing) {
            throw new Error("cannot edit a detached document; call attach() first");
        }
        const containerRemap = new Map();
        const treeRemap = new Map();
        for (const entry of diffBatch) {
            if (!Array.isArray(entry) || entry.length !== 2 || typeof entry[0] !== "string") {
                throw new TypeError("each diff entry must be a [ContainerID, Diff] tuple");
            }
            const sourceId = entry[0];
            const diff = entry[1];
            const container = this.#resolveDiffContainer(sourceId, containerRemap);
            if (container === undefined)
                continue;
            this.#applyContainerDiff(container, diff, containerRemap, treeRemap);
        }
    }
    revertTo(frontiers) {
        if (this.#detached && !this.#detachedEditing) {
            throw new Error("cannot edit a detached document; call attach() first");
        }
        this.#commit({}, true);
        const diff = this.diff(this.frontiers(), frontiers, false);
        this.applyDiff(diff);
    }
    #resolveDiffContainer(sourceId, remap) {
        const mapped = remap.get(sourceId);
        if (mapped !== undefined)
            return mapped;
        const parsed = parseContainerId(sourceId);
        if (parsed.kind === "root" && !isMergeableContainerId(parsed)) {
            return this.#getOrCreateContainer(parsed);
        }
        const existing = this.#containers.get(formatContainerId(parsed));
        return existing !== undefined && !this._isContainerDeleted(existing)
            ? existing
            : undefined;
    }
    #applyContainerDiff(container, diff, containerRemap, treeRemap) {
        if (diff.type === "map") {
            if (!(container instanceof LoroMap))
                throw diffKindMismatch(container, diff.type);
            for (const [key, value] of Object.entries(diff.updated)) {
                if (value === undefined) {
                    container.delete(key);
                    continue;
                }
                const sourceChildId = diffContainerId(value);
                if (sourceChildId === undefined) {
                    container.set(key, value);
                }
                else {
                    this.#applyMapChildDiff(container, key, sourceChildId, containerRemap);
                }
            }
            return;
        }
        if (diff.type === "text") {
            if (!(container instanceof LoroText))
                throw diffKindMismatch(container, diff.type);
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
            if (!(container instanceof LoroTree))
                throw diffKindMismatch(container, diff.type);
            this.#applyTreeDiff(container, diff.diff, containerRemap, treeRemap);
            return;
        }
        if (!(container instanceof LoroList))
            throw diffKindMismatch(container, diff.type);
        let position = 0;
        for (const operation of diff.diff) {
            if ("retain" in operation) {
                position += operation.retain;
            }
            else if ("delete" in operation) {
                container.delete(position, operation.delete);
            }
            else {
                for (const value of operation.insert) {
                    const sourceChildId = diffContainerId(value);
                    if (sourceChildId === undefined) {
                        container.insert(position, value);
                    }
                    else {
                        const parsed = parseContainerId(sourceChildId);
                        if (parsed.kind === "root") {
                            throw new TypeError("a root container cannot be inserted as a child");
                        }
                        const child = createContainer(codecTypeToPublic(parsed.containerType));
                        const attached = container.insertContainer(position, child);
                        containerRemap.set(sourceChildId, attached);
                    }
                    position += 1;
                }
            }
        }
    }
    #applyMapChildDiff(parent, key, sourceId, remap) {
        const mapped = remap.get(sourceId);
        const current = parent.get(key);
        if (mapped !== undefined && current === mapped)
            return;
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
        const child = createContainer(type);
        remap.set(sourceId, parent.setContainer(key, child));
    }
    #applyTreeDiff(tree, diff, containerRemap, treeRemap) {
        const resolveNode = (source) => treeRemap.get(source) ?? (tree.has(source) ? source : undefined);
        for (const item of diff) {
            if (item.action !== "delete")
                continue;
            const target = resolveNode(item.target);
            if (target !== undefined)
                tree.delete(target);
        }
        const pending = diff.filter((item) => item.action !== "delete");
        while (pending.length > 0) {
            let progressed = false;
            for (let index = 0; index < pending.length;) {
                const item = pending[index];
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
                }
                else {
                    const target = resolveNode(item.target);
                    if (target !== undefined)
                        tree.move(target, parent, item.index);
                }
                pending.splice(index, 1);
                progressed = true;
            }
            if (!progressed) {
                throw new RangeError("tree diff refers to a parent that does not exist");
            }
        }
    }
    getDeepValueWithId() {
        return Object.fromEntries([...this.#roots].map(([name, container]) => [
            name,
            containerDeepValueWithId(container),
        ]));
    }
    getShallowValue() {
        return Object.fromEntries([...this.#roots].map(([name, container]) => [name, container.id]));
    }
    getDeepValueWithID() {
        return this.getDeepValueWithId();
    }
    version() {
        const version = this.#checkoutVersion?.clone() ?? this.#historyVersion();
        if (this.#pending !== undefined) {
            const end = this.#pending.id.counter + this.#pending.operationLength;
            if (end > (version.get(this.#peer) ?? 0))
                version.set(this.#peer, end);
        }
        return version;
    }
    oplogVersion() {
        return this.#historyVersion();
    }
    #historyVersion() {
        if (this.#deferredSnapshotHistory !== undefined) {
            return this.#deferredSnapshotHistory.endVersion.clone();
        }
        const version = this.#shallowStartVersion.clone();
        for (const [peer, end] of this.#historyEndByPeer) {
            if (end > (version.get(peer) ?? 0))
                version.set(peer, end);
        }
        return version;
    }
    frontiers() {
        return this.#frontiersCodec().map(formatOpId);
    }
    oplogFrontiers() {
        if (this.#deferredSnapshotHistory !== undefined) {
            return [...this.#deferredSnapshotHistory.frontiers]
                .sort(compareIds)
                .map(formatOpId);
        }
        return [...this.#historyFrontiers.values()].sort(compareIds).map(formatOpId);
    }
    frontiersToVV(frontiers) {
        const version = new VersionVector();
        for (const [peer, counter] of this.#causalVersionAt(frontiers.map(parseOpId))) {
            version.set(peer, counter);
        }
        return version;
    }
    #versionForExistingFrontiers(frontiers) {
        for (const frontier of frontiers) {
            if (this.#recordContaining(parseOpId(frontier)) === undefined) {
                throw new RangeError(`frontiers include unknown id ${frontier.counter}@${frontier.peer}`);
            }
        }
        return this.frontiersToVV(frontiers);
    }
    vvToFrontiers(version) {
        return this.#frontiersForVersion(version).map(formatOpId);
    }
    cmpWithFrontiers(frontiers) {
        const current = this.oplogFrontiers();
        if (frontierSetsEqual(current, frontiers))
            return 0;
        const version = this.#historyVersion();
        return frontiers.every((frontier) => {
            const id = parseOpId(frontier);
            return id.counter < (version.get(id.peer) ?? 0);
        })
            ? 1
            : -1;
    }
    cmpFrontiers(left, right) {
        return this.#versionForExistingFrontiers(left).compare(this.#versionForExistingFrontiers(right));
    }
    changeCount() {
        this.#materializeDeferredHistory();
        return this.#history.size;
    }
    debugHistory() {
        this.#sortedHistory();
    }
    opCount() {
        return this.#deferredSnapshotHistory?.operationCount ?? this.#historyOperationCount;
    }
    getAllChanges() {
        const changes = new Map();
        for (const { change } of this.#sortedHistory()) {
            const peer = peerIdToString(change.id.peer);
            const peerChanges = changes.get(peer);
            if (peerChanges === undefined)
                changes.set(peer, [publicChange(change)]);
            else
                peerChanges.push(publicChange(change));
        }
        return changes;
    }
    getChangeAt(id) {
        const target = parseOpId(id);
        const record = this.#recordContaining(target);
        if (record === undefined) {
            throw new RangeError(`change ${target.counter}@${target.peer} is unknown`);
        }
        return publicChange(record.change);
    }
    getChangeAtLamport(peer, lamport) {
        this.#materializeDeferredHistory();
        const parsedPeer = parsePeerId(peer);
        const records = this.#historyByPeer.get(parsedPeer) ?? [];
        let low = 0;
        let high = records.length;
        while (low < high) {
            const middle = (low + high) >>> 1;
            if (records[middle].change.lamport <= lamport)
                low = middle + 1;
            else
                high = middle;
        }
        const record = low > 0 ? records[low - 1] : undefined;
        return record === undefined ? undefined : publicChange(record.change);
    }
    getOpsInChange(id) {
        this.#commit({}, true);
        const record = this.#recordContaining(parseOpId(id));
        if (record === undefined) {
            throw new RangeError(`change ${id.counter}@${id.peer} is unknown`);
        }
        return historyRecordToJsonChange(record, undefined, false).ops;
    }
    getUncommittedOpsAsJson() {
        const pending = this.#pending;
        if (pending === undefined || pending.operations.length === 0)
            return undefined;
        const timestamp = this.#nextCommitOptions.timestamp ??
            (this.#recordTimestamp ? Date.now() / 1000 : 0);
        const record = {
            change: {
                id: pending.id,
                dependencies: pending.dependencies,
                lamport: pending.lamport,
                timestamp: numberToI64(timestamp),
                message: this.#nextCommitOptions.message,
                operations: pending.operations,
            },
            keys: pending.keys,
            keyIndices: undefined,
        };
        return historyRecordsToJsonSchema([record], this.#historyVersion(), false);
    }
    travelChangeAncestors(ids, callback) {
        this.#commit({}, true);
        const visited = new Set();
        let stopped = false;
        const visit = (id) => {
            if (stopped)
                return;
            const record = this.#recordContaining(id);
            if (record === undefined) {
                if (id.counter < (this.#shallowStartVersion.get(id.peer) ?? 0))
                    return;
                throw new RangeError(`change ${id.counter}@${id.peer} is unknown`);
            }
            const key = changeKey(record.change.id);
            if (visited.has(key))
                return;
            for (const dependency of record.change.dependencies)
                visit(dependency);
            if (stopped || visited.has(key))
                return;
            visited.add(key);
            if (callback(publicChange(record.change)) === false)
                stopped = true;
        };
        for (const id of ids.map(parseOpId))
            visit(id);
    }
    findIdSpansBetween(from, to) {
        const fromVersion = this.#versionForExistingFrontiers(from);
        const toVersion = this.#versionForExistingFrontiers(to);
        this.#assertVersionNotBeforeShallowRoot(fromVersion);
        this.#assertVersionNotBeforeShallowRoot(toVersion);
        const retreat = [];
        const forward = [];
        const peers = new Set([
            ...fromVersion._codecEntriesUnsorted().map(({ peer }) => peer),
            ...toVersion._codecEntriesUnsorted().map(({ peer }) => peer),
        ]);
        for (const peer of [...peers].sort((left, right) => left < right ? -1 : left > right ? 1 : 0)) {
            const fromCounter = fromVersion.get(peer) ?? 0;
            const toCounter = toVersion.get(peer) ?? 0;
            if (fromCounter < toCounter) {
                forward.push({
                    peer: peerIdToString(peer),
                    counter: fromCounter,
                    length: toCounter - fromCounter,
                });
            }
            else if (fromCounter > toCounter) {
                retreat.push({
                    peer: peerIdToString(peer),
                    counter: toCounter,
                    length: fromCounter - toCounter,
                });
            }
        }
        return { retreat, forward };
    }
    getChangedContainersIn(id, len) {
        this.#commit({}, true);
        this.#materializeDeferredHistory();
        if (!Number.isSafeInteger(len) || len < 0) {
            throw new RangeError(`change range length is out of range: ${len}`);
        }
        const start = parseOpId(id);
        const end = start.counter + len;
        const containers = new Set();
        const records = this.#historyByPeer.get(start.peer) ?? [];
        let recordIndex = Math.max(0, lowerBoundHistory(records, start.counter + 1) - 1);
        for (; recordIndex < records.length; recordIndex += 1) {
            const { change } = records[recordIndex];
            if (change.id.counter >= end)
                break;
            let operationIndex = lowerBoundOperation(change.operations, start.counter + 1) - 1;
            operationIndex = Math.max(0, operationIndex);
            for (; operationIndex < change.operations.length; operationIndex += 1) {
                const operation = change.operations[operationIndex];
                if (operation.counter >= end)
                    break;
                if (operation.counter < end &&
                    operation.counter + operation.length > start.counter) {
                    containers.add(formatContainerId(operation.container));
                }
            }
        }
        return [...containers];
    }
    #recordContaining(id) {
        this.#materializeDeferredHistory();
        const records = this.#historyByPeer.get(id.peer);
        if (records === undefined)
            return undefined;
        const index = lowerBoundHistory(records, id.counter + 1) - 1;
        const record = index >= 0 ? records[index] : undefined;
        return record !== undefined &&
            id.counter < record.change.id.counter + changeLength(record.change)
            ? record
            : undefined;
    }
    #greatestTimestampAt(frontiers) {
        const version = this.#causalVersionAt(frontiers);
        let greatest = 0n;
        for (const [peer, counter] of version) {
            if (counter === 0)
                continue;
            const record = this.#recordContaining({ peer, counter: counter - 1 });
            if (record !== undefined)
                greatest = maxBigInt(greatest, record.change.timestamp);
        }
        return greatest;
    }
    #mergeablePreviousRecord(change, interval) {
        if (change.id.counter === 0)
            return undefined;
        const previous = this.#recordContaining({
            peer: change.id.peer,
            counter: change.id.counter - 1,
        });
        return previous !== undefined && canMergeChanges(previous.change, change, interval)
            ? previous
            : undefined;
    }
    fork() {
        return this.forkAt(this.frontiers());
    }
    forkAt(frontiers) {
        this.#commit({}, true);
        const version = this.#versionForExistingFrontiers(frontiers);
        this.#assertVersionNotBeforeShallowRoot(version);
        const fork = new LoroDoc();
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
    attach() {
        this.checkoutToLatest();
    }
    checkout(frontiers) {
        this.#commit({}, true);
        for (const frontier of frontiers) {
            if (this.#recordContaining(parseOpId(frontier)) === undefined) {
                throw new RangeError(`frontier ${frontier.counter}@${frontier.peer} is unknown`);
            }
        }
        const before = this.#hasEventSubscribers() ? this.#frontiersCodec() : [];
        const currentVersion = this.version();
        const targetVersion = this.frontiersToVV(frontiers);
        this.#assertVersionNotBeforeShallowRoot(targetVersion);
        const forwardRecords = this.#recordsInVersionRange(currentVersion, targetVersion);
        const retreatRecords = this.#recordsInVersionRange(targetVersion, currentVersion);
        const changed = this.#hasEventSubscribers()
            ? changedContainerIds([...forwardRecords, ...retreatRecords])
            : new Set();
        let beforeValues = new Map();
        let preparedDiffs = new Map();
        this.#checkoutVersion = targetVersion;
        this.#detached = true;
        const changedRecords = [...forwardRecords, ...retreatRecords];
        const movableMoveMode = movableMoveTransitionMode(retreatRecords, forwardRecords, this.#movableMovePeers);
        if (this.#canTransitionRecords(changedRecords, movableMoveMode)) {
            const recording = this.#hasEventSubscribers()
                ? { beforeValues: new Map(), eventStates: new Map() }
                : undefined;
            this.#applyVersionTransition(retreatRecords, forwardRecords, targetVersion, recording, movableMoveMode);
            if (recording !== undefined) {
                beforeValues = recording.beforeValues;
                preparedDiffs = this.#recordedEventDiffs(recording);
            }
        }
        else if (retreatRecords.length === 0 &&
            !hasMaterializedSequenceInsertions(forwardRecords, this.#containers)) {
            const recording = this.#hasEventSubscribers()
                ? { beforeValues: new Map(), eventStates: new Map() }
                : undefined;
            this.#applyRecords(forwardRecords, recording);
            if (recording !== undefined) {
                beforeValues = recording.beforeValues;
                preparedDiffs = this.#recordedEventDiffs(recording);
            }
        }
        else {
            if (this.#hasEventSubscribers()) {
                beforeValues = this.#captureContainerEventValues(changed);
            }
            this.#rebuildFromHistory(targetVersion);
        }
        this.#emit("checkout", undefined, before, changed.size > 0 ? this.#frontiersCodec() : [], changed, beforeValues, preparedDiffs);
        if (this.#detachedEditing)
            this.#renewPeerId();
    }
    detach() {
        this.#commit({}, true);
        this.#checkoutVersion = this.version();
        this.#detached = true;
    }
    isDetached() {
        return this.#detached;
    }
    checkoutToLatest() {
        this.#commit({}, true);
        const wasDetached = this.#detached;
        // Without subscribers (or when already attached) no event is emitted, so
        // skip building the frontier array and the changed-container set.
        const emitting = wasDetached && this.#hasEventSubscribers();
        const before = emitting ? this.#frontiersCodec() : [];
        const currentVersion = this.version();
        const latestVersion = this.#historyVersion();
        const forwardRecords = this.#recordsInVersionRange(currentVersion, latestVersion);
        const retreatRecords = this.#recordsInVersionRange(latestVersion, currentVersion);
        const changed = emitting
            ? changedContainerIds([...forwardRecords, ...retreatRecords])
            : new Set();
        let beforeValues = new Map();
        let preparedDiffs = new Map();
        this.#checkoutVersion = undefined;
        this.#detached = false;
        if (wasDetached) {
            const changedRecords = [...forwardRecords, ...retreatRecords];
            const movableMoveMode = movableMoveTransitionMode(retreatRecords, forwardRecords, this.#movableMovePeers);
            if (this.#canTransitionRecords(changedRecords, movableMoveMode)) {
                const recording = this.#hasEventSubscribers()
                    ? { beforeValues: new Map(), eventStates: new Map() }
                    : undefined;
                this.#applyVersionTransition(retreatRecords, forwardRecords, latestVersion, recording, movableMoveMode);
                if (recording !== undefined) {
                    beforeValues = recording.beforeValues;
                    preparedDiffs = this.#recordedEventDiffs(recording);
                }
            }
            else if (retreatRecords.length === 0 &&
                !hasMaterializedSequenceInsertions(forwardRecords, this.#containers)) {
                const recording = this.#hasEventSubscribers()
                    ? { beforeValues: new Map(), eventStates: new Map() }
                    : undefined;
                this.#applyRecords(forwardRecords, recording);
                if (recording !== undefined) {
                    beforeValues = recording.beforeValues;
                    preparedDiffs = this.#recordedEventDiffs(recording);
                }
            }
            else {
                if (this.#hasEventSubscribers()) {
                    beforeValues = this.#captureContainerEventValues(changed);
                }
                this.#rebuildFromHistory();
            }
            this.#emit("checkout", undefined, before, changed.size > 0 ? this.#frontiersCodec() : [], changed, beforeValues, preparedDiffs);
            if (this.#detachedEditing)
                this.#renewPeerId();
        }
    }
    #renewPeerId() {
        let peer = generatePeerId();
        while (peer === this.#peer || peer === 0xffffffffffffffffn) {
            peer = generatePeerId();
        }
        this.#peer = peer;
        this.#nextCounter = this.oplogVersion().get(peer) ?? 0;
    }
    isShallow() {
        return this.#shallowRootStore !== undefined;
    }
    shallowSinceVV() {
        return this.#shallowStartVersion.clone();
    }
    shallowSinceFrontiers() {
        return this.#shallowRootFrontiers.map(formatOpId);
    }
    subscribe(listener) {
        this.#subscribers.add(listener);
        return () => this.#subscribers.delete(listener);
    }
    subscribeLocalUpdates(listener) {
        this.#localUpdateSubscribers.add(listener);
        return () => this.#localUpdateSubscribers.delete(listener);
    }
    subscribeFirstCommitFromPeer(listener) {
        this.#firstCommitSubscribers.add(listener);
        return () => this.#firstCommitSubscribers.delete(listener);
    }
    subscribePreCommit(listener) {
        this.#preCommitSubscribers.add(listener);
        return () => this.#preCommitSubscribers.delete(listener);
    }
    subscribeJsonpath(path, callback) {
        const matches = compileJsonPathEventMatcher(path);
        return this.subscribe((event) => {
            if (event.events.some(matches))
                callback();
        });
    }
    JSONPath(path) {
        if (path === "$")
            return [Object.fromEntries(this.#roots)];
        return evaluateJsonPath(this.#roots, path);
    }
    getPathToContainer(id) {
        const container = this.getContainerById(id);
        if (container === undefined || this._isContainerDeleted(container))
            return undefined;
        return containerPath(container);
    }
    getByPath(path) {
        const parts = path.split("/").filter(Boolean);
        if (parts.length === 0)
            return undefined;
        let value = this.#roots.get(parts[0]);
        for (const part of parts.slice(1)) {
            if (value instanceof LoroMap) {
                value = value.get(part);
            }
            else if (value instanceof LoroList) {
                value = value.get(parsePathIndex(part));
            }
            else if (value instanceof LoroTree) {
                value = treeNodeAtPath(value, undefined, part);
            }
            else if (value instanceof LoroTreeNode) {
                const index = parseOptionalPathIndex(part);
                value = index === undefined ? value.data.get(part) : value._childAt(index);
            }
            else if (Array.isArray(value)) {
                value = value[parsePathIndex(part)];
            }
            else if (typeof value === "object" && value !== null) {
                value = value[part];
            }
            else {
                return undefined;
            }
            if (value === undefined)
                return undefined;
        }
        return value instanceof LoroTreeNode ? value.data : value;
    }
    getCursorPos(cursor) {
        const container = this.getContainerById(cursor.containerId());
        if (container === undefined || this._isContainerDeleted(container))
            return undefined;
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
        if (target === undefined)
            return undefined;
        const publicOffset = (element) => container instanceof LoroText
            ? (container._sequence.visibleMetricOffsetOf(element, "utf16") ??
                0)
            : (container._sequence.visibleIndexOf(element) ?? 0);
        if (!target.deleted) {
            const offset = publicOffset(target);
            return {
                offset: offset +
                    (cursor.side() === 1
                        ? container instanceof LoroText
                            ? target.value.length
                            : 1
                        : 0),
                side: cursor.side(),
            };
        }
        const next = container._sequence.nextVisible(target);
        if (next !== undefined) {
            const offset = publicOffset(next);
            return {
                offset,
                side: -1,
                update: new Cursor(container.id, next.id, -1, offset),
            };
        }
        const previous = container._sequence.previousVisible(target);
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
    _subscribeContainer(container, listener) {
        let listeners = this.#containerSubscribers.get(container.id);
        if (listeners === undefined) {
            listeners = new Set();
            this.#containerSubscribers.set(container.id, listeners);
        }
        listeners.add(listener);
        return () => {
            listeners.delete(listener);
            if (listeners.size === 0 &&
                this.#containerSubscribers.get(container.id) === listeners) {
                this.#containerSubscribers.delete(container.id);
            }
        };
    }
    _isContainerDeleted(container) {
        if (container._codecId?.kind === "root" &&
            !isMergeableContainerId(container._codecId))
            return !this.#roots.has(container._codecId.name);
        const parent = container.parent();
        if (container._codecId !== undefined &&
            isMergeableContainerId(container._codecId) &&
            parent === undefined)
            return true;
        const binding = container._parentLink?.binding ??
            (parent === undefined ? undefined : recoverParentBinding(container, parent));
        if (binding?.kind === "map" && parent instanceof LoroMap) {
            const record = parent._entries.get(binding.key);
            return record === undefined || record.deleted || record.value !== container;
        }
        if (binding?.kind === "sequence" && parent instanceof LoroList) {
            return binding.element.deleted || binding.element.value !== container;
        }
        if (binding?.kind === "tree" && parent instanceof LoroTree) {
            return parent._isRecordDeleted(binding.record) || binding.record.data !== container;
        }
        return parent !== undefined;
    }
    _undoIdSpan(peer, range) {
        this.#commit({}, true);
        this.#materializeDeferredHistory();
        const parsedPeer = parsePeerId(peer);
        const records = this.#historyByPeer.get(parsedPeer) ?? [];
        let recordIndex = Math.min(records.length - 1, lowerBoundHistory(records, range.end) - 1);
        for (; recordIndex >= 0; recordIndex -= 1) {
            const record = records[recordIndex];
            const changeEnd = record.change.id.counter + changeLength(record.change);
            if (changeEnd <= range.start)
                break;
            let operationIndex = Math.min(record.change.operations.length - 1, lowerBoundOperation(record.change.operations, range.end) - 1);
            for (; operationIndex >= 0; operationIndex -= 1) {
                const operation = record.change.operations[operationIndex];
                if (operation.counter + operation.length <= range.start)
                    break;
                this.#undoOperation(record, operation, parsedPeer, range);
            }
        }
    }
    _transformUndoCursors(cursors) {
        return cursors.map((cursor) => {
            const container = this.getContainerById(cursor.containerId());
            if (!(container instanceof LoroText || container instanceof LoroList)) {
                return cursor;
            }
            const position = Math.min(cursor._originPositionValue(), container.length);
            return container.getCursor(position, cursor.side()) ?? cursor;
        });
    }
    #undoOperation(record, operation, peer, range) {
        const container = this.#containers.get(formatContainerId(operation.container));
        if (container === undefined)
            return;
        const content = operation.content;
        const overlapStart = Math.max(operation.counter, range.start);
        const overlapEnd = Math.min(operation.counter + operation.length, range.end);
        if (overlapStart >= overlapEnd)
            return;
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
                if (container instanceof LoroCounter &&
                    (content.value.type === "double" || content.value.type === "i64")) {
                    container.increment(-(content.value.type === "double"
                        ? content.value.value
                        : Number(content.value.value)));
                }
                else if (container instanceof LoroCounter &&
                    content.value.type === "delta-int") {
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
    #deleteInsertedElements(container, peer, start, end) {
        const positions = [];
        for (let counter = start; counter < end; counter += 1) {
            const element = container._sequence.findById({ peer, counter });
            if (element === undefined || element.deleted)
                continue;
            const index = container._sequence.visibleIndexOf(element);
            if (index !== undefined)
                positions.push(index);
        }
        positions.sort((left, right) => left - right);
        const ranges = [];
        for (const position of positions) {
            const previous = ranges.at(-1);
            if (previous !== undefined && previous.start + previous.length === position) {
                previous.length += 1;
            }
            else {
                ranges.push({ start: position, length: 1 });
            }
        }
        for (const selected of ranges.reverse()) {
            if (container instanceof LoroText) {
                this._textDelete(container, selected.start, selected.length);
            }
            else {
                this._sequenceDelete(container, selected.start, selected.length);
            }
        }
    }
    #restoreDeletedElements(container, peer, range, overlapStart, overlapEnd) {
        const belongsToUndo = (id) => id.peer === peer && id.counter >= range.start && id.counter < range.end;
        const belongsToOperation = (id) => id.peer === peer && id.counter >= overlapStart && id.counter < overlapEnd;
        const targets = container._sequence
            .elementsDeletedBy(peer, overlapStart, overlapEnd)
            .filter((element) => !belongsToUndo(element.id) &&
            container._sequence.someDeletion(element, belongsToOperation) &&
            container._sequence.everyDeletion(element, belongsToUndo));
        const groups = [];
        for (const target of targets) {
            const previous = groups.at(-1)?.at(-1);
            if (previous !== undefined &&
                container._sequence.physicalIndexOf(previous) + 1 ===
                    container._sequence.physicalIndexOf(target)) {
                groups.at(-1).push(target);
            }
            else {
                groups.push([target]);
            }
        }
        for (const group of groups) {
            const position = container._sequence.visibleIndexOf(group[0]);
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
            }
            else {
                let offset = 0;
                for (const element of group) {
                    if (isContainer(element.value)) {
                        const child = createContainer(element.value.kind());
                        restoreBlueprint(child, captureBlueprint(element.value));
                        this._listInsertContainer(container, position + offset, child);
                    }
                    else {
                        this._listInsert(container, position + offset, cloneRuntimeValue(element.value));
                    }
                    offset += 1;
                }
            }
        }
    }
    #undoMapOperation(map, record, operation) {
        const content = operation.content;
        if (content.type !== "map-insert" && content.type !== "map-delete")
            return;
        const writer = operationWriter(record.change, operation);
        const current = map._entries.get(content.key);
        if (current === undefined || compareWriter(current.writer, writer) !== 0)
            return;
        const previous = this.#previousMapOperation(operation.container, content.key, writer);
        if (previous === undefined || previous.operation.content.type === "map-delete") {
            map.delete(content.key);
            return;
        }
        const previousContent = previous.operation.content;
        if (previousContent.type !== "map-insert")
            return;
        const previousId = {
            peer: previous.record.change.id.peer,
            counter: previous.operation.counter,
        };
        const rawValue = this.#decodeRuntimeValue(previousContent.value, previous.record.keys, previousId, map);
        const value = this.#materializeMapValue(map, content.key, rawValue);
        if (isContainer(value)) {
            const child = createContainer(value.kind());
            restoreBlueprint(child, captureBlueprint(value));
            map.setContainer(content.key, child);
        }
        else {
            map.set(content.key, cloneRuntimeValue(value));
        }
    }
    #previousMapOperation(containerId, key, before) {
        const operations = this.#mapOperationHistory
            .get(formatContainerId(containerId))
            ?.get(key);
        if (operations === undefined)
            return undefined;
        const previous = operations.byWriter.at(operations.byWriter._lowerBoundBy((operation) => compareWriter(operation.writer, before)) - 1);
        return previous === undefined
            ? undefined
            : { record: previous.record, operation: previous.operation };
    }
    #undoTreeOperation(tree, record, operation) {
        const content = operation.content;
        if (content.type !== "tree-create" &&
            content.type !== "tree-move" &&
            content.type !== "tree-delete") {
            return;
        }
        const writer = operationWriter(record.change, operation);
        const node = tree._nodes.get(formatTreeId(content.subject));
        if (node === undefined || compareWriter(node.writer, writer) !== 0)
            return;
        const previous = this.#previousTreeOperation(operation.container, content.subject, writer);
        if (previous === undefined || previous.operation.content.type === "tree-delete") {
            this.#appendAndApply(tree, { type: "tree-delete", subject: content.subject }, 1);
            return;
        }
        const previousContent = previous.operation.content;
        if (previousContent.type !== "tree-create" && previousContent.type !== "tree-move") {
            return;
        }
        this.#appendAndApply(tree, {
            type: "tree-move",
            subject: content.subject,
            parent: previousContent.parent,
            position: previousContent.position.slice(),
        }, 1);
    }
    #previousTreeOperation(containerId, subject, before) {
        const operations = this.#treeOperationHistory
            .get(formatContainerId(containerId))
            ?.get(idKey(subject));
        if (operations === undefined)
            return undefined;
        const previous = operations.byWriter.at(operations.byWriter._lowerBoundBy((operation) => compareWriter(operation.writer, before)) - 1);
        return previous === undefined
            ? undefined
            : { record: previous.record, operation: previous.operation };
    }
    _mapSet(container, key, value) {
        const current = container._entries.get(key);
        if (current !== undefined &&
            !current.deleted &&
            eventValuesEqual(current.value, value)) {
            return;
        }
        const encoded = this.#encodeRuntimeValue(value, this.#ensurePending());
        this.#appendAndApply(container, { type: "map-insert", key, value: encoded }, 1);
    }
    _mapDelete(container, key) {
        const current = container._entries.get(key);
        if (current === undefined || current.deleted)
            return;
        this.#appendAndApply(container, { type: "map-delete", key }, 1);
    }
    _mapSetContainer(container, key, child) {
        return this.#attachChild(container, child, (rawType) => ({
            type: "map-insert",
            key,
            value: { type: "container-type", value: rawType },
        }));
    }
    _mapEnsureMergeable(container, key, type) {
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
                throw new TypeError(`cannot create a mergeable ${type} at key ${JSON.stringify(key)}: ` +
                    "the key already holds a non-mergeable value");
            }
            if (existing.rawValue instanceof Uint8Array &&
                bytesEqual(existing.rawValue, marker)) {
                return this.#getOrCreateContainer(childId, container);
            }
        }
        const child = this.#getOrCreateContainer(childId, container);
        this.#appendAndApply(container, { type: "map-insert", key, value: { type: "binary", value: marker } }, 1);
        return child;
    }
    _listInsert(container, position, value) {
        const encoded = this.#encodeRuntimeValue(value, this.#ensurePending());
        const type = container instanceof LoroMovableList ? "movable-list-insert" : "list-insert";
        this.#appendAndApply(container, { type, position, values: [encoded] }, 1);
    }
    _listInsertContainer(container, position, child) {
        return this.#attachChild(container, child, (rawType) => ({
            type: container instanceof LoroMovableList ? "movable-list-insert" : "list-insert",
            position,
            values: [{ type: "container-type", value: rawType }],
        }));
    }
    _sequenceDelete(container, position, length) {
        this.#deleteSequenceRuns(container, position, length, container instanceof LoroMovableList ? "movable-list-delete" : "list-delete");
    }
    _textInsert(container, position, text) {
        this.#appendAndApply(container, { type: "text-insert", position, value: text }, unicodeScalarLength(text));
    }
    _textDelete(container, position, length) {
        this.#deleteTextRuns(container, position, length);
    }
    _textMark(container, start, end, key, value) {
        const encoded = this.#encodeRuntimeValue(value, this.#ensurePending());
        const info = textStyleInfoByte(this.#textStyleExpand(key), value === null);
        this.#appendAndApply(container, { type: "text-mark", start, end, key, value: encoded, info }, 1);
        this.#appendAndApply(container, { type: "text-mark-end" }, 1);
    }
    _movableMove(container, from, to) {
        const element = container._visibleElementAt(from);
        this.#appendAndApply(container, {
            type: "movable-list-move",
            from,
            to,
            elementId: { peer: element.id.peer, lamport: element.lamport },
        }, 1);
    }
    _movableSet(container, position, value) {
        const element = container._visibleElementAt(position);
        if (eventValuesEqual(element.value, value))
            return;
        const encoded = this.#encodeRuntimeValue(value, this.#ensurePending());
        this.#appendAndApply(container, {
            type: "movable-list-set",
            elementId: { peer: element.id.peer, lamport: element.lamport },
            value: encoded,
        }, 1);
    }
    _movableSetContainer(container, position, child) {
        const element = container._visibleElementAt(position);
        return this.#attachChild(container, child, (rawType) => ({
            type: "movable-list-set",
            elementId: { peer: element.id.peer, lamport: element.lamport },
            value: { type: "container-type", value: rawType },
        }));
    }
    _counterIncrement(container, value) {
        if (value === 0)
            return;
        this.#appendAndApply(container, { type: "future", property: 0, value: { type: "double", value } }, 1);
    }
    #textStyleExpand(key) {
        const baseKey = key.split(":", 1)[0];
        const configured = this.#textStyles.get(baseKey) ?? this.#defaultTextStyle;
        if (configured === undefined) {
            throw new RangeError(`text style ${JSON.stringify(baseKey)} is not configured`);
        }
        return configured;
    }
    _treeCreate(tree, parent, index) {
        const pending = this.#ensurePending();
        const subject = { peer: this.#peer, counter: this.#nextOperationCounter(pending) };
        const parentId = parent === undefined ? undefined : parseTreeId(parent);
        if (parent !== undefined && !tree.has(parent)) {
            throw new RangeError(`tree parent ${parent} does not exist`);
        }
        const position = tree._positionFor(parentId, index);
        this.#appendAndApply(tree, { type: "tree-create", subject, parent: parentId, position }, 1);
        return new LoroTreeNode(tree, subject);
    }
    _treeMove(tree, target, parent, index) {
        if (!tree.has(target))
            throw new RangeError(`tree node ${target} does not exist`);
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
        this.#appendAndApply(tree, { type: "tree-move", subject, parent: parentId, position }, 1);
    }
    _treeDelete(tree, target) {
        this.#appendAndApply(tree, { type: "tree-delete", subject: parseTreeId(target) }, 1);
    }
    #getContainer(nameOrId, expectedType) {
        const byId = isContainerId(nameOrId);
        const id = byId
            ? parseContainerId(nameOrId)
            : { kind: "root", name: nameOrId, containerType: publicTypeToCodec(expectedType) };
        if (codecTypeToPublic(id.containerType) !== expectedType) {
            throw new TypeError(`container ${nameOrId} is not a ${expectedType}`);
        }
        if (byId &&
            (id.kind === "normal" || isMergeableContainerId(id)) &&
            !this.#containers.has(formatContainerId(id))) {
            throw new RangeError(`container ${nameOrId} does not exist in this document`);
        }
        const container = this.#getOrCreateContainer(id);
        if (id.kind === "root" && !isMergeableContainerId(id)) {
            this.#roots.set(id.name, container);
        }
        return container;
    }
    #getOrCreateContainer(id, parent) {
        const key = this.#containerKey(id);
        const existing = this.#containers.get(key);
        if (existing !== undefined) {
            if (parent !== undefined)
                existing._parentLink = { container: parent };
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
    #containerKey(id) {
        let key = this.#containerKeys.get(id);
        if (key === undefined) {
            key = formatContainerId(id);
            this.#containerKeys.set(id, key);
        }
        return key;
    }
    #ensurePending() {
        if (this.#pending !== undefined)
            return this.#pending;
        this.#materializeDeferredHistory();
        if (this.#detached && !this.#detachedEditing) {
            throw new Error("cannot edit a detached document; call attach() first");
        }
        const from = this.#frontiersCodec();
        const lamport = from.reduce((max, dependency) => Math.max(max, this.#lamportAt(dependency) + 1), 0);
        this.#nextCounter = Math.max(this.#nextCounter, this.oplogVersion().get(this.#peer) ?? 0);
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
    #nextOperationCounter(pending) {
        return pending.id.counter + pending.operationLength;
    }
    #appendAndApply(container, content, length, preferredChild) {
        if (container._codecId === undefined || container._doc !== this)
            throw new Error("container is detached");
        const pending = this.#ensurePending();
        const counter = this.#nextOperationCounter(pending);
        const operation = {
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
        causalVersion.set(pending.id.peer, Math.max(causalVersion.get(pending.id.peer) ?? 0, operation.counter));
        const finishEvent = this.#prepareEvent(pending, container, operation, pending.keys, pending.id, causalVersion);
        this.#applyOperation(operation, pending.keys, pending.id, pending.lamport, causalVersion, container);
        finishEvent?.();
        return operation;
    }
    #prepareEvent(recording, container, operation, keys, changeId, causalVersion, force = false) {
        if (!force && !this.#hasEventSubscribers())
            return undefined;
        const content = operation.content;
        if (container instanceof LoroText) {
            const state = this.#sequenceEventState(recording, container, "text");
            if (content.type === "text-insert") {
                return () => {
                    const first = container._sequence.findById({
                        peer: changeId.peer,
                        counter: operation.counter,
                    });
                    if (first === undefined || first.deleted)
                        return;
                    const position = container._sequence.visibleMetricOffsetOf(first, "utf16");
                    if (position === undefined)
                        return;
                    const attributes = Object.fromEntries([...container._attributesAt(first)].map(([key, value]) => [
                        key,
                        runtimeValueToJson(value),
                    ]));
                    state.diff.insertText(position, content.value, attributes);
                };
            }
            if (content.type === "text-delete") {
                for (const range of this.#sequenceEventDeletionRanges(container, content.startId, Math.abs(Number(content.length))).reverse()) {
                    state.diff.delete(range.position, range.length);
                }
                return undefined;
            }
            if (content.type === "text-mark") {
                const value = this.#decodeRuntimeValue(content.value, keys, { peer: changeId.peer, counter: operation.counter }, container);
                const runs = container._styleRuns(content.start, content.end, causalVersion);
                for (const range of container._sequence.visibleMetricRangesForIdRuns(runs, "utf16")) {
                    state.diff.formatText(range.start, range.end - range.start, content.key, runtimeValueToJson(value));
                }
                return undefined;
            }
            if (content.type === "text-mark-end")
                return undefined;
        }
        else if (container instanceof LoroList) {
            const state = this.#sequenceEventState(recording, container, "list");
            if (content.type === "list-insert" || content.type === "movable-list-insert") {
                return () => {
                    const first = container._sequence.findById({
                        peer: changeId.peer,
                        counter: operation.counter,
                    });
                    if (first === undefined || first.deleted)
                        return;
                    const position = container._sequence.visibleIndexOf(first);
                    if (position === undefined)
                        return;
                    state.diff.insertList(position, container
                        ._visibleElementsRange(position, position + content.values.length)
                        .map((element) => cloneRuntimeValue(element.value)));
                };
            }
            if (content.type === "list-delete" || content.type === "movable-list-delete") {
                for (const range of this.#sequenceEventDeletionRanges(container, content.startId, Math.abs(Number(content.length))).reverse()) {
                    state.diff.delete(range.position, range.length);
                }
                return undefined;
            }
            if (content.type === "movable-list-move") {
                const element = container._sequence.findByLamport(content.elementId.peer, content.elementId.lamport);
                const from = element === undefined || element.deleted
                    ? undefined
                    : container._sequence.visibleIndexOf(element);
                if (element === undefined || from === undefined)
                    return undefined;
                const value = cloneRuntimeValue(element.value);
                return () => {
                    const to = element.deleted
                        ? undefined
                        : container._sequence.visibleIndexOf(element);
                    if (to === undefined || to === from)
                        return;
                    state.diff.delete(from, 1);
                    state.diff.insertList(to, [value]);
                };
            }
            if (content.type === "movable-list-set") {
                const element = container._sequence.findByLamport(content.elementId.peer, content.elementId.lamport);
                const position = element === undefined || element.deleted
                    ? undefined
                    : container._sequence.visibleIndexOf(element);
                if (element === undefined || position === undefined)
                    return undefined;
                const selected = element;
                const previous = cloneRuntimeValue(element.value);
                return () => {
                    const value = cloneRuntimeValue(selected.value);
                    if (eventValuesEqual(previous, value))
                        return;
                    state.diff.delete(position, 1);
                    state.diff.insertList(position, [value]);
                };
            }
        }
        else if (container instanceof LoroMap) {
            if (content.type === "map-insert" || content.type === "map-delete") {
                const state = this.#mapEventState(recording, container);
                const beforeRecord = container._entries.get(content.key);
                const beforeVisible = beforeRecord !== undefined && !beforeRecord.deleted;
                const beforeValue = beforeVisible
                    ? cloneRuntimeValue(beforeRecord.value)
                    : undefined;
                if (!state.originals.has(content.key)) {
                    state.originals.set(content.key, {
                        present: beforeRecord !== undefined,
                        visible: beforeVisible,
                        value: beforeValue,
                    });
                }
                return () => {
                    const afterRecord = container._entries.get(content.key);
                    const afterVisible = afterRecord !== undefined && !afterRecord.deleted;
                    const afterValue = afterVisible
                        ? cloneRuntimeValue(afterRecord.value)
                        : undefined;
                    if (beforeVisible !== afterVisible ||
                        !eventValuesEqual(beforeValue, afterValue)) {
                        state.changed.add(content.key);
                    }
                };
            }
        }
        else if (container instanceof LoroTree) {
            if (content.type === "tree-create" ||
                content.type === "tree-move" ||
                content.type === "tree-delete") {
                const state = this.#treeEventState(recording, container);
                const id = formatTreeId(content.subject);
                if (!state.originals.has(id)) {
                    const record = container._nodes.get(id);
                    state.originals.set(id, record === undefined || container._isRecordDeleted(record)
                        ? undefined
                        : this.#treeEventNode(container, record));
                }
                return undefined;
            }
        }
        else if (container instanceof LoroCounter) {
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
    #sequenceEventState(recording, container, kind) {
        const existing = recording.eventStates.get(container.id);
        if (existing !== undefined) {
            if (existing.kind !== "sequence")
                throw new Error("event state kind changed");
            return existing;
        }
        const state = {
            kind: "sequence",
            diff: new SequenceEventDiff(kind, container.length),
        };
        recording.eventStates.set(container.id, state);
        return state;
    }
    #mapEventState(recording, container) {
        const existing = recording.eventStates.get(container.id);
        if (existing !== undefined) {
            if (existing.kind !== "map")
                throw new Error("event state kind changed");
            return existing;
        }
        const state = {
            kind: "map",
            originals: new Map(),
            changed: new Set(),
        };
        recording.eventStates.set(container.id, state);
        return state;
    }
    #treeEventState(recording, container) {
        const existing = recording.eventStates.get(container.id);
        if (existing !== undefined) {
            if (existing.kind !== "tree")
                throw new Error("event state kind changed");
            return existing;
        }
        const state = {
            kind: "tree",
            originals: new Map(),
        };
        recording.eventStates.set(container.id, state);
        return state;
    }
    #treeEventNode(tree, record) {
        return {
            id: formatTreeId(record.id),
            parent: record.parent === undefined ? undefined : formatTreeId(record.parent),
            index: tree._indexOf(record),
            fractionalIndex: bytesToHex(record.position).toUpperCase(),
        };
    }
    #sequenceEventDeletionRanges(container, startId, length) {
        return container._sequence
            .visibleMetricRangesForIdRuns([{ start: { peer: startId.peer, counter: startId.counter }, length }], "utf16")
            .map(({ start, end }) => ({ position: start, length: end - start }));
    }
    #recordedEventDiffs(recording, includeMapTombstones = false) {
        const diffs = new Map();
        for (const [id, state] of recording.eventStates) {
            const container = this.#containers.get(id);
            if (container === undefined)
                continue;
            if (state.kind === "sequence") {
                diffs.set(id, state.diff.toDiff());
            }
            else if (state.kind === "map" && container instanceof LoroMap) {
                const updated = [];
                for (const [key, original] of state.originals) {
                    const record = container._entries.get(key);
                    const present = record !== undefined;
                    const visible = record !== undefined && !record.deleted;
                    const value = visible ? cloneRuntimeValue(record.value) : undefined;
                    if (state.changed.has(key) ||
                        (includeMapTombstones && present !== original.present) ||
                        visible !== original.visible ||
                        !eventValuesEqual(value, original.value)) {
                        updated.push([key, value]);
                    }
                }
                diffs.set(id, { type: "map", updated: Object.fromEntries(updated) });
            }
            else if (state.kind === "counter" && container instanceof LoroCounter) {
                diffs.set(id, { type: "counter", increment: container.value - state.before });
            }
            else if (state.kind === "tree" && container instanceof LoroTree) {
                const before = [];
                const after = [];
                for (const [nodeId, original] of state.originals) {
                    if (original !== undefined)
                        before.push(original);
                    const record = container._nodes.get(nodeId);
                    if (record !== undefined && !container._isRecordDeleted(record)) {
                        after.push(this.#treeEventNode(container, record));
                    }
                }
                diffs.set(id, { type: "tree", diff: treeDelta(before, after) });
            }
        }
        return diffs;
    }
    #attachChild(parent, child, content) {
        if (child._doc !== undefined && child._doc !== this) {
            throw new Error("cannot attach a container from another document");
        }
        const blueprint = captureBlueprint(child);
        const attached = createContainer(child.kind());
        const rawType = containerTypeToRawByte(publicTypeToCodec(child.kind()));
        this.#appendAndApply(parent, content(rawType), 1, attached);
        restoreBlueprint(attached, blueprint);
        if (!child.isAttached())
            child._attached = attached;
        return attached;
    }
    #childIdForOperation(operation, type) {
        return {
            kind: "normal",
            peer: this.#peer,
            counter: operation.counter,
            containerType: publicTypeToCodec(type),
        };
    }
    #deleteSequenceRuns(container, position, length, type) {
        for (const run of container._sequence.visibleIdRuns(position, position + length)) {
            this.#appendAndApply(container, { type, position, length: BigInt(run.length), startId: run.start }, run.length);
        }
    }
    #deleteTextRuns(container, position, length) {
        for (const run of container._sequence.visibleIdRuns(position, position + length)) {
            this.#appendAndApply(container, {
                type: "text-delete",
                position,
                length: BigInt(run.length),
                startId: run.start,
            }, run.length);
            this.#mergeTrailingTextDeletes();
        }
    }
    #mergeTrailingTextDeletes() {
        const operations = this.#pending?.operations;
        if (operations === undefined || operations.length < 2)
            return;
        const left = operations[operations.length - 2];
        const right = operations[operations.length - 1];
        if (left.content.type !== "text-delete" ||
            right.content.type !== "text-delete" ||
            !containerIdsEqual(left.container, right.container)) {
            return;
        }
        const leftLength = Number(left.content.length);
        const rightLength = Number(right.content.length);
        if (rightLength <= 0 || left.content.startId.peer !== right.content.startId.peer) {
            return;
        }
        let mergedLength;
        let startId = left.content.startId;
        if (leftLength > 0 &&
            left.content.position === right.content.position &&
            right.content.startId.counter === left.content.startId.counter + leftLength) {
            mergedLength = leftLength + rightLength;
        }
        else if (leftLength === 1 &&
            right.content.position + rightLength === left.content.position &&
            right.content.startId.counter + rightLength === left.content.startId.counter) {
            mergedLength = -(leftLength + rightLength);
            startId = right.content.startId;
        }
        else if (leftLength < 0 &&
            right.content.position + rightLength - 1 === left.content.position + leftLength &&
            right.content.startId.counter + rightLength === left.content.startId.counter) {
            mergedLength = leftLength - rightLength;
            startId = right.content.startId;
        }
        if (mergedLength === undefined)
            return;
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
    #encodeRuntimeValue(value, pending) {
        if (value == null)
            return { type: "null" };
        if (typeof value === "boolean")
            return { type: "bool", value };
        if (typeof value === "number") {
            if (!Number.isFinite(value))
                throw new TypeError("Loro numbers must be finite");
            return Number.isSafeInteger(value)
                ? { type: "i64", value: BigInt(value) }
                : { type: "double", value };
        }
        if (typeof value === "string")
            return { type: "string", value };
        if (value instanceof Uint8Array)
            return { type: "binary", value: value.slice() };
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
    #decodeRuntimeValue(value, keys, operationId, parent) {
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
                return value.value.map((item, index) => this.#decodeRuntimeValue(item, keys, { peer: operationId.peer, counter: operationId.counter + index }, parent));
            case "map":
                return Object.fromEntries(value.value.map(([keyIndex, item]) => {
                    const key = keys[Number(keyIndex)];
                    if (key === undefined)
                        throw new Error("change value map key index is out of range");
                    return [key, this.#decodeRuntimeValue(item, keys, operationId, parent)];
                }));
            case "container-type": {
                const containerType = containerTypeFromRawByte(value.value);
                const id = { kind: "normal", ...operationId, containerType };
                return this.#getOrCreateContainer(id, parent);
            }
        }
    }
    #applyOperation(operation, keys, changeId, changeLamport, causalVersion, knownContainer) {
        const container = knownContainer ?? this.#getOrCreateContainer(operation.container);
        const operationId = { peer: changeId.peer, counter: operation.counter };
        const lamport = changeLamport + (operation.counter - changeId.counter);
        const content = operation.content;
        switch (content.type) {
            case "map-insert":
                {
                    const rawValue = this.#decodeRuntimeValue(content.value, keys, operationId, container);
                    const writer = { peer: changeId.peer, lamport };
                    container._applyValue(content.key, this.#materializeMapValue(container, content.key, rawValue), writer, rawValue);
                }
                return;
            case "map-delete":
                container._applyDelete(content.key, {
                    peer: changeId.peer,
                    lamport,
                });
                return;
            case "list-insert":
            case "movable-list-insert": {
                const values = content.values.map((value, index) => this.#decodeRuntimeValue(value, keys, { peer: operationId.peer, counter: operationId.counter + index }, container));
                container._insertFugue(content.position, values, values.map((_, index) => ({
                    peer: operationId.peer,
                    counter: operationId.counter + index,
                })), values.map((_, index) => lamport + index), causalVersion);
                return;
            }
            case "list-delete":
            case "movable-list-delete":
                container._deleteIdSpan(content.startId, Number(content.length), operationId);
                return;
            case "text-insert": {
                container._insertFugue(content.position, content.value, operationId, lamport, causalVersion);
                return;
            }
            case "text-delete":
                container._deleteIdSpan(content.startId, Number(content.length), operationId);
                return;
            case "text-mark":
                {
                    const value = this.#decodeRuntimeValue(content.value, keys, operationId, container);
                    const meta = {
                        startId: operationId,
                        lamport,
                        info: content.info,
                        value,
                    };
                    container._applyMark(content.start, content.end, content.key, value, meta, causalVersion);
                }
                return;
            case "text-mark-end":
                return;
            case "movable-list-move": {
                const list = container;
                const element = list._sequence.findByLamport(content.elementId.peer, content.elementId.lamport);
                const from = element === undefined || element.deleted
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
                const list = container;
                const element = list._sequence.findByLamport(content.elementId.peer, content.elementId.lamport);
                if (element !== undefined) {
                    const value = this.#decodeRuntimeValue(content.value, keys, operationId, container);
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
                const tree = container;
                const writer = { peer: changeId.peer, lamport };
                const nodeKey = formatTreeId(content.subject);
                let record = tree._nodes.get(nodeKey);
                if (record === undefined) {
                    const dataId = {
                        kind: "normal",
                        ...content.subject,
                        containerType: CodecContainerType.Map,
                    };
                    const data = this.#getOrCreateContainer(dataId, tree);
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
                }
                else if (compareWriter(record.writer, writer) <= 0) {
                    tree._updateRecord(record, content.parent, content.position.slice(), writer, operationId);
                }
                return;
            }
            case "tree-delete": {
                const record = container._nodes.get(formatTreeId(content.subject));
                if (record !== undefined) {
                    const writer = { peer: changeId.peer, lamport };
                    if (compareWriter(record.writer, writer) <= 0) {
                        container._deleteRecord(record, writer, operationId);
                    }
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
                    }
                    else if (content.value.type === "delta-int") {
                        container._value += content.value.value;
                    }
                }
        }
    }
    #readChangeBlock(bytes) {
        const block = decodeChangeBlock(bytes);
        return block.changes.map((change) => ({
            change,
            keys: block.keys,
            keyIndices: undefined,
        }));
    }
    #decodeImportData(parsed) {
        if (parsed.mode === EncodeMode.FastUpdates) {
            return {
                mode: parsed.mode,
                records: decodeFastUpdatesBody(parsed.body).flatMap((block) => this.#readChangeBlock(block)),
            };
        }
        const snapshot = decodeFastSnapshotBody(parsed.body);
        const records = [];
        let startVersion;
        let startFrontiers;
        let endVersion;
        for (const entry of decodeSstable(snapshot.oplog)) {
            if (bytesEqual(entry.key, VERSION_KEY)) {
                endVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
            }
            else if (bytesEqual(entry.key, START_VERSION_KEY)) {
                startVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
            }
            else if (bytesEqual(entry.key, START_FRONTIERS_KEY)) {
                startFrontiers = decodePostcardFrontiers(entry.value);
            }
            else if (entry.key.length === 12) {
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
    #integrateHistory(records, mergeInterval = 0n) {
        const added = [];
        const addedRecordIndices = new Map();
        for (const record of records) {
            const key = changeKey(record.change.id);
            const knownEnd = Math.max(this.#shallowStartVersion.get(record.change.id.peer) ?? 0, this.#historyEndByPeer.get(record.change.id.peer) ?? 0);
            const recordEnd = record.change.id.counter + changeLength(record.change);
            if (recordEnd <= knownEnd)
                continue;
            const pending = this.#pendingHistory.get(key);
            if (pending !== undefined &&
                pending.change.id.counter + changeLength(pending.change) >= recordEnd)
                continue;
            this.#pendingHistory.set(key, record);
        }
        // Hoist the sorted pending queue and the causal version out of the
        // promotion loop; the version is advanced locally as records promote and
        // stays in sync with #historyEndByPeer. The queue is compacted in place
        // after each pass, and only rebuilt from the map when a sliced record was
        // re-keyed (which may have overwritten another pending entry).
        const version = this.#historyVersion();
        let queue = [...this.#pendingHistory.values()].sort(compareHistoryRecords);
        let promoted = true;
        while (promoted) {
            promoted = false;
            let rebuildQueue = false;
            let kept = 0;
            for (let index = 0; index < queue.length; index += 1) {
                let record = queue[index];
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
                        change: sliceChange(change, knownEnd - change.id.counter, changeLength(change)),
                    };
                    change = record.change;
                    key = changeKey(change.id);
                    this.#pendingHistory.set(key, record);
                    promoted = true;
                    rebuildQueue = true;
                }
                if (change.id.counter !== (version.get(change.id.peer) ?? 0)) {
                    queue[kept] = record;
                    kept += 1;
                    continue;
                }
                if (change.dependencies.some((dependency) => !this.#historyContainsId(dependency))) {
                    queue[kept] = record;
                    kept += 1;
                    continue;
                }
                this.#pendingHistory.delete(key);
                const previous = mergeInterval === undefined
                    ? undefined
                    : this.#mergeablePreviousRecord(change, mergeInterval);
                if (previous === undefined) {
                    this.#setHistoryRecord(key, record, record);
                    addedRecordIndices.set(record, added.length);
                }
                else {
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
            queue.length = kept;
            if (rebuildQueue) {
                queue = [...this.#pendingHistory.values()].sort(compareHistoryRecords);
            }
        }
        return {
            added: added.sort(compareHistoryRecords),
            pending: [...this.#pendingHistory.values()].sort(compareHistoryRecords),
        };
    }
    #historyContainsId(id) {
        if (id.counter < (this.#shallowStartVersion.get(id.peer) ?? 0))
            return true;
        return id.counter < (this.#historyEndByPeer.get(id.peer) ?? 0);
    }
    #assertImportsNotOutdated(records) {
        if (this.#shallowRootStore === undefined)
            return;
        for (const { change } of records) {
            const knownEnd = this.#shallowStartVersion.get(change.id.peer) ?? 0;
            if (change.id.counter + changeLength(change) <= knownEnd)
                continue;
            if (change.dependencies.length === 0) {
                throw new Error("cannot import updates that depend on an outdated version");
            }
            const touchesShallowRoot = change.dependencies.some((dependency) => this.#shallowRootFrontiers.some((frontier) => idsEqual(frontier, dependency)));
            if (touchesShallowRoot)
                continue;
            const dependsOnPrunedHistory = change.dependencies.some((dependency) => dependency.counter < (this.#shallowStartVersion.get(dependency.peer) ?? 0));
            if (dependsOnPrunedHistory) {
                throw new Error("cannot import updates that depend on an outdated version");
            }
        }
    }
    #applyRecords(records, recording) {
        for (const record of records) {
            const causalVersion = this.#causalVersionAt(record.change.dependencies);
            for (const operation of record.change.operations) {
                const container = this.#getOrCreateContainer(operation.container);
                const finishEvent = recording === undefined
                    ? undefined
                    : this.#prepareEvent(recording, container, operation, record.keys, record.change.id, causalVersion, true);
                this.#applyOperation(operation, record.keys, record.change.id, record.change.lamport, causalVersion, container);
                finishEvent?.();
                causalVersion.set(record.change.id.peer, Math.max(causalVersion.get(record.change.id.peer) ?? 0, operation.counter + operation.length));
            }
        }
    }
    #canTransitionRecords(records, movableMoveMode = "anchors") {
        const replayedMoveContainers = new Set();
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
                if (this.#movableOrderHistory.get(containerId) === undefined)
                    return false;
            }
        }
        const moveSuffixes = new Map();
        for (const { change } of records) {
            const causalVersion = this.#causalVersionAt(change.dependencies);
            for (const operation of change.operations) {
                const container = this.#containers.get(this.#containerKey(operation.container));
                const content = operation.content;
                if (content.type === "movable-list-move") {
                    const element = container instanceof LoroMovableList
                        ? container._sequence.findByLamport(content.elementId.peer, content.elementId.lamport)
                        : undefined;
                    const history = element?.moveHistory;
                    const moveIndex = findSequenceMoveMetaIndex(history, operationWriter(change, operation));
                    if (!(container instanceof LoroMovableList) ||
                        !container._moveHistoryComplete ||
                        element === undefined ||
                        (movableMoveMode === "anchors" &&
                            (history === undefined ||
                                moveIndex < 0 ||
                                history[moveIndex].id.peer !== change.id.peer ||
                                history[moveIndex].id.counter !== operation.counter))) {
                        return false;
                    }
                    if (movableMoveMode === "anchors") {
                        let suffix = moveSuffixes.get(element);
                        if (suffix === undefined) {
                            suffix = { history: history, indices: new Set() };
                            moveSuffixes.set(element, suffix);
                        }
                        suffix.indices.add(moveIndex);
                    }
                }
                else if (content.type === "movable-list-set") {
                    const element = container instanceof LoroMovableList
                        ? container._sequence.findByLamport(content.elementId.peer, content.elementId.lamport)
                        : undefined;
                    if (!(container instanceof LoroMovableList) ||
                        !container._valueHistoryComplete ||
                        element === undefined ||
                        !hasSequenceValueMeta(element.valueHistory, operationWriter(change, operation), change.id.peer, operation.counter)) {
                        return false;
                    }
                }
                else if (content.type === "text-mark") {
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
                    if (!container._styleIndex.runsContainMeta(runs, content.key, {
                        peer: change.id.peer,
                        counter: operation.counter,
                    })) {
                        return false;
                    }
                }
                else if (content.type === "text-mark-end") {
                    if (!(container instanceof LoroText))
                        return false;
                }
                else if (content.type === "text-insert") {
                    if (!(container instanceof LoroText))
                        return false;
                    scratchSequenceIdRun.start.peer = change.id.peer;
                    scratchSequenceIdRun.start.counter = operation.counter;
                    scratchSequenceIdRun.length = operation.length;
                    if (!container._sequence.containsIdRuns(scratchSequenceIdRuns)) {
                        return false;
                    }
                }
                else if (content.type === "list-insert" ||
                    content.type === "movable-list-insert") {
                    if (!(container instanceof LoroList))
                        return false;
                    scratchSequenceIdRun.start.peer = change.id.peer;
                    scratchSequenceIdRun.start.counter = operation.counter;
                    scratchSequenceIdRun.length = operation.length;
                    if (!container._sequence.containsIdRuns(scratchSequenceIdRuns)) {
                        return false;
                    }
                }
                else if (content.type === "text-delete" ||
                    content.type === "list-delete" ||
                    content.type === "movable-list-delete") {
                    if (!(container instanceof LoroList || container instanceof LoroText)) {
                        return false;
                    }
                }
                else if (content.type === "map-insert" || content.type === "map-delete") {
                    if (!(container instanceof LoroMap))
                        return false;
                }
                else if (content.type === "tree-create" ||
                    content.type === "tree-move" ||
                    content.type === "tree-delete") {
                    if (!(container instanceof LoroTree))
                        return false;
                }
                else if (content.type === "future") {
                    if (!(container instanceof LoroCounter) ||
                        counterDelta(content) === undefined) {
                        return false;
                    }
                }
                causalVersion.set(change.id.peer, Math.max(causalVersion.get(change.id.peer) ?? 0, operation.counter + operation.length));
            }
        }
        for (const { history, indices } of moveSuffixes.values()) {
            const first = Math.min(...indices);
            if (history.length - first !== indices.size)
                return false;
            for (let index = first; index < history.length; index += 1) {
                if (!indices.has(index))
                    return false;
            }
        }
        return true;
    }
    #applyVersionTransition(retreat, forward, target, recording, movableMoveMode = "anchors") {
        const sequences = new Map();
        const bulkSequenceRemovals = new Map();
        const bulkSequenceRestorations = new Map();
        const textAttributeRuns = new Map();
        const textStyleContainers = new Set();
        const movableValues = new Map();
        const mapKeys = new Map();
        const treeSubjects = new Map();
        const counters = new Map();
        const sequenceElements = (container) => {
            let elements = sequences.get(container);
            if (elements === undefined) {
                elements = new Set();
                sequences.set(container, elements);
            }
            return elements;
        };
        const addTextAttributeRuns = (container, key, runs) => {
            let keys = textAttributeRuns.get(container);
            if (keys === undefined) {
                keys = new Map();
                textAttributeRuns.set(container, keys);
            }
            const existing = keys.get(key);
            if (existing === undefined)
                keys.set(key, [...runs]);
            else
                existing.push(...runs);
        };
        const addBulkSequenceRemovals = (container, runs) => {
            const existing = bulkSequenceRemovals.get(container);
            if (existing === undefined)
                bulkSequenceRemovals.set(container, [...runs]);
            else
                existing.push(...runs);
        };
        const addBulkSequenceRestorations = (container, runs) => {
            const existing = bulkSequenceRestorations.get(container);
            if (existing === undefined)
                bulkSequenceRestorations.set(container, [...runs]);
            else
                existing.push(...runs);
        };
        const collect = (records, direction) => {
            for (const { change } of records) {
                const causalVersion = this.#causalVersionAt(change.dependencies);
                for (const operation of change.operations) {
                    const container = this.#containers.get(this.#containerKey(operation.container));
                    const content = operation.content;
                    if (content.type === "text-insert" ||
                        content.type === "list-insert" ||
                        content.type === "movable-list-insert") {
                        const sequenceContainer = container;
                        const sequence = sequenceContainer._sequence;
                        scratchSequenceIdRun.start.peer = change.id.peer;
                        scratchSequenceIdRun.start.counter = operation.counter;
                        scratchSequenceIdRun.length = operation.length;
                        if (direction === -1 ||
                            (recording === undefined &&
                                sequence.canShowIdRunsAt(scratchSequenceIdRuns, target))) {
                            // The bulk lists retain their runs, so they get a fresh copy
                            // rather than the shared scratch run.
                            const insertedRuns = [
                                {
                                    start: { peer: change.id.peer, counter: operation.counter },
                                    length: operation.length,
                                },
                            ];
                            if (direction === -1) {
                                addBulkSequenceRemovals(sequenceContainer, insertedRuns);
                            }
                            else {
                                addBulkSequenceRestorations(sequenceContainer, insertedRuns);
                            }
                            causalVersion.set(change.id.peer, Math.max(causalVersion.get(change.id.peer) ?? 0, operation.counter + operation.length));
                            continue;
                        }
                        const elements = sequenceElements(sequenceContainer);
                        for (let offset = 0; offset < operation.length; offset += 1) {
                            elements.add(sequence.findById({
                                peer: change.id.peer,
                                counter: operation.counter + offset,
                            }));
                        }
                    }
                    else if (content.type === "text-delete" ||
                        content.type === "list-delete" ||
                        content.type === "movable-list-delete") {
                        const sequenceContainer = container;
                        const deletedRuns = sequenceContainer._sequence.idRunsDeletedBy(change.id.peer, operation.counter, operation.counter + operation.length);
                        if (direction === 1 ||
                            (recording === undefined &&
                                sequenceContainer._sequence.canShowIdRunsAt(deletedRuns, target))) {
                            if (direction === 1) {
                                addBulkSequenceRemovals(sequenceContainer, deletedRuns);
                            }
                            else {
                                addBulkSequenceRestorations(sequenceContainer, deletedRuns);
                            }
                            causalVersion.set(change.id.peer, Math.max(causalVersion.get(change.id.peer) ?? 0, operation.counter + operation.length));
                            continue;
                        }
                        const elements = sequenceElements(sequenceContainer);
                        for (const element of sequenceContainer._sequence.elementsDeletedBy(change.id.peer, operation.counter, operation.counter + operation.length)) {
                            elements.add(element);
                        }
                    }
                    else if (content.type === "text-mark") {
                        const text = container;
                        textStyleContainers.add(text);
                        if (recording !== undefined) {
                            addTextAttributeRuns(text, content.key, text._styleRuns(content.start, content.end, causalVersion));
                        }
                    }
                    else if (content.type === "movable-list-set") {
                        const list = container;
                        const element = list._sequence.findByLamport(content.elementId.peer, content.elementId.lamport);
                        let elements = movableValues.get(list);
                        if (elements === undefined) {
                            elements = new Set();
                            movableValues.set(list, elements);
                        }
                        elements.add(element);
                    }
                    else if (content.type === "map-insert" || content.type === "map-delete") {
                        const map = container;
                        let keys = mapKeys.get(map);
                        if (keys === undefined) {
                            keys = new Set();
                            mapKeys.set(map, keys);
                        }
                        keys.add(content.key);
                    }
                    else if (content.type === "tree-create" ||
                        content.type === "tree-move" ||
                        content.type === "tree-delete") {
                        const tree = container;
                        let subjects = treeSubjects.get(tree);
                        if (subjects === undefined) {
                            subjects = new Map();
                            treeSubjects.set(tree, subjects);
                        }
                        subjects.set(idKey(content.subject), content.subject);
                    }
                    else if (content.type === "future") {
                        const counter = container;
                        counters.set(counter, (counters.get(counter) ?? 0) + direction * counterDelta(content));
                    }
                    causalVersion.set(change.id.peer, Math.max(causalVersion.get(change.id.peer) ?? 0, operation.counter + operation.length));
                }
            }
        };
        collect(retreat, -1);
        collect(forward, 1);
        if (recording !== undefined) {
            for (const [container, runs] of [...bulkSequenceRemovals]) {
                if (!sequences.has(container))
                    continue;
                const elements = sequenceElements(container);
                for (const run of runs) {
                    for (let offset = 0; offset < run.length; offset += 1) {
                        const element = container._sequence.findById({
                            peer: run.start.peer,
                            counter: run.start.counter + offset,
                        });
                        if (element !== undefined)
                            elements.add(element);
                    }
                }
                bulkSequenceRemovals.delete(container);
            }
        }
        const includes = (id) => id.counter < (target.get(id.peer) ?? 0);
        const targetDeleted = (container, element) => !includes(element.id) || container._sequence.someDeletion(element, includes);
        const styleVersion = target.compare(this.#historyVersion()) === 0
            ? undefined
            : new Map(target
                ._codecEntriesUnsorted()
                .map(({ peer, counter }) => [peer, counter]));
        for (const [text, keys] of textAttributeRuns) {
            const state = this.#sequenceEventState(recording, text, "text");
            const removedRuns = [
                ...(bulkSequenceRemovals.get(text) ?? []),
                ...[...(sequences.get(text) ?? [])]
                    .filter((element) => !element.deleted && targetDeleted(text, element))
                    .map((element) => ({ start: element.id, length: 1 })),
            ];
            for (const [key, runs] of keys) {
                for (const transition of text._styleIndex.transitions(runs, key, text._styleVersion, styleVersion)) {
                    const beforePresent = transition.before !== undefined && transition.before.value !== null;
                    const beforeValue = beforePresent ? transition.before.value : undefined;
                    const afterPresent = transition.after !== undefined && transition.after.value !== null;
                    const afterValue = afterPresent ? transition.after.value : undefined;
                    if (beforePresent === afterPresent &&
                        eventValuesEqual(beforeValue, afterValue)) {
                        continue;
                    }
                    const retainedRuns = subtractSequenceIdRuns([transition.run], removedRuns);
                    for (const range of text._sequence.visibleMetricRangesForIdRuns(retainedRuns, "utf16")) {
                        state.diff.formatText(range.start, range.end - range.start, key, afterPresent ? runtimeValueToJson(afterValue) : null);
                    }
                }
            }
        }
        for (const text of textStyleContainers)
            text._setStyleVersion(styleVersion);
        for (const [container, runs] of bulkSequenceRestorations) {
            container._sequence.setIdRunsVisible(runs);
        }
        for (const [container, elements] of sequences) {
            const state = recording === undefined
                ? undefined
                : this.#sequenceEventState(recording, container, container instanceof LoroText ? "text" : "list");
            const removals = [...elements]
                .filter((element) => !element.deleted && targetDeleted(container, element))
                .map((element) => ({
                element,
                position: container instanceof LoroText
                    ? container._sequence.visibleMetricOffsetOf(element, "utf16")
                    : container._sequence.visibleIndexOf(element),
            }))
                .sort((left, right) => right.position - left.position);
            for (const { element, position } of removals) {
                state?.diff.delete(position, container instanceof LoroText ? element.value.length : 1);
                container._sequence.setDeleted(element, true);
            }
            const insertions = [...elements]
                .filter((element) => element.deleted && !targetDeleted(container, element))
                .sort((left, right) => container._sequence.physicalIndexOf(left) -
                container._sequence.physicalIndexOf(right));
            for (const element of insertions) {
                container._sequence.setDeleted(element, false);
                if (state === undefined)
                    continue;
                if (container instanceof LoroText) {
                    const textElement = element;
                    const position = container._sequence.visibleMetricOffsetOf(textElement, "utf16");
                    const attributes = Object.fromEntries([...container._attributesAt(textElement)].map(([key, value]) => [
                        key,
                        runtimeValueToJson(value),
                    ]));
                    state.diff.insertText(position, textElement.value, attributes);
                }
                else {
                    const position = container._sequence.visibleIndexOf(element);
                    state.diff.insertList(position, [cloneRuntimeValue(element.value)]);
                }
            }
        }
        for (const [container, runs] of bulkSequenceRemovals) {
            if (recording !== undefined) {
                const state = this.#sequenceEventState(recording, container, container instanceof LoroText ? "text" : "list");
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
        }
        else {
            this.#transitionMovableMoves(retreat, true, recording);
            this.#transitionMovableMoves(forward, false, recording);
        }
        for (const [list, elements] of movableValues) {
            const state = recording === undefined
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
                            value: record === undefined || record.deleted
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
                }
                else if (content.type === "map-insert") {
                    const operationId = {
                        peer: indexed.record.change.id.peer,
                        counter: indexed.operation.counter,
                    };
                    const rawValue = this.#decodeRuntimeValue(content.value, indexed.record.keys, operationId, map);
                    map._replaceRecord(key, {
                        value: this.#materializeMapValue(map, key, rawValue),
                        rawValue,
                        deleted: false,
                        writer: indexed.writer,
                    });
                }
            }
        }
        if (recording !== undefined) {
            for (const [tree, subjects] of treeSubjects) {
                const state = this.#treeEventState(recording, tree);
                for (const subject of subjects.values()) {
                    const nodeKey = formatTreeId(subject);
                    if (state.originals.has(nodeKey))
                        continue;
                    const record = tree._nodes.get(nodeKey);
                    state.originals.set(nodeKey, record === undefined || tree._isRecordDeleted(record)
                        ? undefined
                        : this.#treeEventNode(tree, record));
                }
            }
        }
        for (const [tree, subjects] of treeSubjects) {
            const history = this.#treeOperationHistory.get(tree.id);
            for (const [historyKey, subject] of subjects) {
                const nodeKey = formatTreeId(subject);
                const operations = history?.get(historyKey);
                const winner = latestIncludedOperation(operations, target);
                const existing = tree._nodes.get(nodeKey);
                if (winner === undefined) {
                    if (existing !== undefined)
                        tree._removeRecord(existing);
                    continue;
                }
                const winnerContent = winner.operation.content;
                const placement = winnerContent.type === "tree-create" || winnerContent.type === "tree-move"
                    ? winner
                    : latestIncludedTreePlacement(operations, target, winner.writer);
                if (placement === undefined)
                    continue;
                const placementContent = placement.operation.content;
                if (placementContent.type !== "tree-create" &&
                    placementContent.type !== "tree-move") {
                    continue;
                }
                let record = tree._nodes.get(nodeKey);
                const lastMoveId = {
                    peer: placement.record.change.id.peer,
                    counter: placement.operation.counter,
                };
                if (record === undefined) {
                    const data = this.#getOrCreateContainer({
                        kind: "normal",
                        ...placementContent.subject,
                        containerType: CodecContainerType.Map,
                    }, tree);
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
                }
                else {
                    tree._updateRecord(record, placementContent.parent, placementContent.position.slice(), placement.writer, lastMoveId);
                }
                if (winnerContent.type === "tree-delete") {
                    tree._deleteRecord(record, winner.writer, {
                        peer: winner.record.change.id.peer,
                        counter: winner.operation.counter,
                    });
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
    #transitionMovableMoves(records, retreat, recording) {
        const operations = records
            .flatMap(({ change }) => change.operations.flatMap((operation) => operation.content.type === "movable-list-move" ? [{ change, operation }] : []))
            .sort((left, right) => compareHistoryOperations(left.change, left.operation, right.change, right.operation));
        if (retreat)
            operations.reverse();
        for (const { change, operation } of operations) {
            const content = operation.content;
            if (content.type !== "movable-list-move")
                continue;
            const container = this.#containers.get(this.#containerKey(operation.container));
            if (!(container instanceof LoroMovableList))
                continue;
            const element = container._sequence.findByLamport(content.elementId.peer, content.elementId.lamport);
            if (element === undefined || element.deleted)
                continue;
            const meta = findSequenceMoveMeta(element.moveHistory, operationWriter(change, operation));
            if (meta === undefined)
                continue;
            const from = container._sequence.visibleIndexOf(element);
            if (from === undefined)
                continue;
            const value = cloneRuntimeValue(element.value);
            container._moveToAnchors(element, retreat ? meta.beforePrevious : meta.afterPrevious, retreat ? meta.beforeNext : meta.afterNext);
            const to = container._sequence.visibleIndexOf(element);
            if (recording === undefined || to === undefined || to === from)
                continue;
            const state = this.#sequenceEventState(recording, container, "list");
            state.diff.delete(from, 1);
            state.diff.insertList(to, [value]);
        }
    }
    #canonicalizeImportedMovableMoves(records, recording) {
        let hasMove = false;
        for (const { change } of records) {
            for (const operation of change.operations) {
                if (operation.content.type !== "movable-list-move")
                    continue;
                hasMove = true;
                const container = this.#containers.get(this.#containerKey(operation.container));
                if (!(container instanceof LoroMovableList) || !container._moveHistoryComplete) {
                    return;
                }
            }
        }
        if (hasMove)
            this.#replayMovableMoves([], records, this.#historyVersion(), recording);
    }
    #replayMovableMoves(retreat, forward, target, recording) {
        const containerIds = new Set();
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
            if (!(container instanceof LoroMovableList) || history === undefined)
                continue;
            const replay = new LoroMovableList();
            const ordered = history
                .values()
                .map((indexed) => ({
                indexed,
                orderRecord: this.#recordContaining({
                    peer: indexed.record.change.id.peer,
                    counter: indexed.operation.counter,
                }) ?? indexed.record,
            }))
                .sort((left, right) => compareHistoryRecords(left.orderRecord, right.orderRecord) ||
                left.indexed.operation.counter - right.indexed.operation.counter);
            for (const { indexed } of ordered) {
                const peer = indexed.record.change.id.peer;
                const includedLength = Math.min(indexed.operation.length, (target.get(peer) ?? 0) - indexed.operation.counter);
                if (includedLength <= 0)
                    continue;
                const operation = includedLength === indexed.operation.length
                    ? indexed.operation
                    : sliceOperation(indexed.operation, 0, includedLength);
                const content = operation.content;
                const operationId = { peer, counter: operation.counter };
                const lamport = indexed.record.change.lamport +
                    operation.counter -
                    indexed.record.change.id.counter;
                const causalVersion = this.#causalVersionAt(indexed.record.change.dependencies);
                causalVersion.set(peer, Math.max(causalVersion.get(peer) ?? 0, operation.counter));
                if (content.type === "movable-list-insert") {
                    replay._insertFugue(content.position, content.values.map(() => null), content.values.map((_, offset) => ({
                        peer,
                        counter: operation.counter + offset,
                    })), content.values.map((_, offset) => lamport + offset), causalVersion);
                }
                else if (content.type === "movable-list-delete") {
                    replay._deleteIdSpan(content.startId, Number(content.length), operationId);
                }
                else if (content.type === "movable-list-move") {
                    const element = replay._sequence.findByLamport(content.elementId.peer, content.elementId.lamport);
                    const from = element === undefined || element.deleted
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
            const currentIndex = new Map(current.map((element, index) => [element, index]));
            const targetElements = replay._visibleElements().map((replayed) => {
                const element = container._sequence.findById(replayed.id);
                if (element === undefined || element.deleted) {
                    throw new Error("movable-list replay produced an unavailable target element");
                }
                return element;
            });
            for (const replayed of replay._elements) {
                const element = container._sequence.findById(replayed.id);
                if (element === undefined)
                    continue;
                for (const meta of replayed.moveHistory ?? []) {
                    let history = element.moveHistory;
                    if (history === undefined) {
                        history = [];
                        element.moveHistory = history;
                    }
                    const existing = history.findIndex(({ id }) => id.peer === meta.id.peer && id.counter === meta.id.counter);
                    if (existing >= 0)
                        history[existing] = meta;
                    else {
                        history.push(meta);
                        history.sort((left, right) => compareWriter({ peer: left.id.peer, lamport: left.lamport }, { peer: right.id.peer, lamport: right.lamport }));
                    }
                }
            }
            if (targetElements.length !== current.length ||
                targetElements.some((element) => !currentIndex.has(element))) {
                throw new Error("movable-list replay changed the visible element set");
            }
            const stableTargetIndices = longestIncreasingSubsequenceIndices(targetElements.map((element) => currentIndex.get(element)));
            const state = recording === undefined
                ? undefined
                : this.#sequenceEventState(recording, container, "list");
            for (let index = targetElements.length - 1; index >= 0; index -= 1) {
                if (stableTargetIndices.has(index))
                    continue;
                const element = targetElements[index];
                const from = container._sequence.visibleIndexOf(element);
                container._sequence.moveBefore(element, targetElements[index + 1]);
                const to = container._sequence.visibleIndexOf(element);
                if (state !== undefined && from !== to) {
                    state.diff.delete(from, 1);
                    state.diff.insertList(to, [cloneRuntimeValue(element.value)]);
                }
            }
        }
    }
    #rebuildFromHistory(version) {
        for (const container of this.#containers.values())
            container._reset();
        if (this.#shallowRootStore !== undefined) {
            this.#hydrateState(this.#shallowRootStore);
            const target = version ?? this.#historyVersion();
            this.#assertVersionNotBeforeShallowRoot(target);
            this.#applyRecords(this.#recordsInVersionRange(this.#shallowRootVersion, target));
            return;
        }
        this.#applyRecords(version === undefined ? this.#sortedHistory() : this.#recordsAtVersion(version));
    }
    #setHistoryRecord(key, record, appended) {
        const previous = this.#history.get(key);
        this.#history.set(key, record);
        if (previous !== undefined)
            this.#historyOrder.delete(previous);
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
            if (peerRecords[previousIndex] === previous)
                peerRecords.splice(previousIndex, 1);
        }
        const index = lowerBoundHistory(peerRecords, record.change.id.counter);
        peerRecords.splice(index, 0, record);
        const last = peerRecords.at(-1);
        if (last === undefined) {
            this.#historyEndByPeer.delete(record.change.id.peer);
        }
        else {
            this.#historyEndByPeer.set(record.change.id.peer, last.change.id.counter + changeLength(last.change));
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
    #appendMergedHistoryRecord(record, appended, previousLength) {
        const appendedLength = changeLength(appended.change);
        this.#historyOperationCount += appendedLength;
        this.#historyEndByPeer.set(record.change.id.peer, record.change.id.counter + previousLength + appendedLength);
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
    #indexHistoryOperations(record) {
        for (const operation of record.change.operations) {
            this.#containersWithOperations.add(this.#containerKey(operation.container));
            const content = operation.content;
            if (content.type === "movable-list-insert" ||
                content.type === "movable-list-delete" ||
                content.type === "movable-list-move") {
                const indexed = {
                    record,
                    operation,
                    writer: operationWriter(record.change, operation),
                };
                const container = this.#containerKey(operation.container);
                let history = this.#movableOrderHistory.get(container);
                if (history === undefined) {
                    history = new OrderedIndex((left, right) => compareWriter(left.writer, right.writer));
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
            let bySubject;
            let subject;
            let treeOperation = false;
            if (content.type === "map-insert" || content.type === "map-delete") {
                const container = this.#containerKey(operation.container);
                bySubject = this.#mapOperationHistory.get(container);
                if (bySubject === undefined) {
                    bySubject = new Map();
                    this.#mapOperationHistory.set(container, bySubject);
                }
                subject = content.key;
            }
            else if (content.type === "tree-create" ||
                content.type === "tree-move" ||
                content.type === "tree-delete") {
                const container = this.#containerKey(operation.container);
                bySubject = this.#treeOperationHistory.get(container);
                if (bySubject === undefined) {
                    bySubject = new Map();
                    this.#treeOperationHistory.set(container, bySubject);
                }
                subject = idKey(content.subject);
                treeOperation = true;
            }
            if (bySubject === undefined || subject === undefined)
                continue;
            // Only movable/map/tree operations reach here, so the index entry and
            // its writer are allocated off the text/list hot path.
            const indexed = {
                record,
                operation,
                writer: operationWriter(record.change, operation),
            };
            let history = bySubject.get(subject);
            if (history === undefined) {
                history = {
                    byWriter: new OrderedIndex((left, right) => compareWriter(left.writer, right.writer)),
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
                let placements = history.placementsByPeer.get(record.change.id.peer);
                if (placements === undefined) {
                    placements = [];
                    history.placementsByPeer.set(record.change.id.peer, placements);
                }
                const placementIndex = lowerBoundIndexedOperation(placements, operation.counter);
                placements.splice(placementIndex, 0, indexed);
            }
        }
    }
    #validateDeferredFrontierBlocks(entries, endVersion, frontiers) {
        const changeEntriesByPeer = new Map();
        for (const entry of entries) {
            if (entry.key.length !== 12)
                continue;
            const start = decodeChangeBlockKey(entry.key);
            const peerEntries = changeEntriesByPeer.get(start.peer);
            if (peerEntries === undefined) {
                changeEntriesByPeer.set(start.peer, [{ entry, start }]);
            }
            else {
                peerEntries.push({ entry, start });
            }
        }
        for (const peerEntries of changeEntriesByPeer.values()) {
            peerEntries.sort((left, right) => left.start.counter - right.start.counter);
        }
        const seenPeers = new Set();
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
                if (peerEntries[middle].start.counter <= frontier.counter)
                    low = middle + 1;
                else
                    high = middle;
            }
            const candidate = peerEntries[low - 1];
            if (candidate === undefined) {
                throw new Error("snapshot frontier change block is missing");
            }
            const block = validateChangeBlock(candidate.entry.value);
            if (block.peer !== candidate.start.peer ||
                block.counterStart !== candidate.start.counter) {
                throw new Error("snapshot change key does not match its block");
            }
            if (block.peer !== frontier.peer ||
                frontier.counter < block.counterStart ||
                frontier.counter >= block.counterEnd) {
                throw new Error("snapshot frontier is not covered by its change block");
            }
        }
    }
    #materializeDeferredHistory() {
        const deferred = this.#deferredSnapshotHistory;
        if (deferred === undefined)
            return;
        const records = [];
        for (const entry of deferred.entries) {
            if (entry.key.length !== 12)
                continue;
            const expected = decodeChangeBlockKey(entry.key);
            const decoded = this.#readChangeBlock(entry.value);
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
    #sortedHistory() {
        this.#materializeDeferredHistory();
        return (this.#sortedHistoryCache ??= this.#historyOrder.values());
    }
    #recordsAtVersion(version) {
        return this.#recordsInVersionRange(new VersionVector(), version);
    }
    #recordsInVersionRange(from, to) {
        this.#materializeDeferredHistory();
        const selected = [];
        for (const { peer, counter: toCounter } of to._codecEntriesUnsorted()) {
            const fromCounter = from.get(peer) ?? 0;
            if (fromCounter >= toCounter)
                continue;
            const records = this.#historyByPeer.get(peer) ?? [];
            let index = Math.max(0, lowerBoundHistory(records, fromCounter + 1) - 1);
            for (; index < records.length; index += 1) {
                const record = records[index];
                const recordStart = record.change.id.counter;
                if (recordStart >= toCounter)
                    break;
                const length = changeLength(record.change);
                const start = Math.max(0, fromCounter - recordStart);
                const end = Math.min(length, toCounter - recordStart);
                if (start >= end)
                    continue;
                selected.push(start === 0 && end === length
                    ? record
                    : { ...record, change: sliceChange(record.change, start, end) });
            }
        }
        return selected.sort(compareHistoryRecords);
    }
    #recordsInSpans(spans) {
        this.#materializeDeferredHistory();
        const spansByPeer = new Map();
        for (const { id, len } of spans) {
            if (!Number.isSafeInteger(len) || len < 0) {
                throw new RangeError(`update span length is out of range: ${len}`);
            }
            const parsed = parseOpId(id);
            if (len === 0)
                continue;
            let peerSpans = spansByPeer.get(parsed.peer);
            if (peerSpans === undefined) {
                peerSpans = [];
                spansByPeer.set(parsed.peer, peerSpans);
            }
            peerSpans.push({ start: parsed.counter, end: parsed.counter + len });
        }
        const selected = [];
        for (const [peer, ranges] of spansByPeer) {
            ranges.sort((left, right) => left.start - right.start);
            const merged = [];
            for (const range of ranges) {
                const previous = merged.length > 0 ? merged[merged.length - 1] : undefined;
                if (previous !== undefined && range.start <= previous.end) {
                    previous.end = Math.max(previous.end, range.end);
                }
                else {
                    merged.push({ ...range });
                }
            }
            const records = this.#historyByPeer.get(peer) ?? [];
            for (const range of merged) {
                let index = Math.max(0, lowerBoundHistory(records, range.start + 1) - 1);
                for (; index < records.length; index += 1) {
                    const record = records[index];
                    const changeStart = record.change.id.counter;
                    if (changeStart >= range.end)
                        break;
                    const length = changeLength(record.change);
                    const start = Math.max(0, range.start - changeStart);
                    const end = Math.min(length, range.end - changeStart);
                    if (start >= end)
                        continue;
                    selected.push(start === 0 && end === length
                        ? record
                        : { ...record, change: sliceChange(record.change, start, end) });
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
                    if (start >= end)
                        continue;
                    selected.push(start === changeStart && end === changeEnd
                        ? preCommit
                        : {
                            ...preCommit,
                            change: sliceChange(preCommit.change, start - changeStart, end - changeStart),
                        });
                }
            }
        }
        return selected.sort(compareHistoryRecords);
    }
    #assertVersionNotBeforeShallowRoot(version) {
        if (!versionIncludes(version, this.#shallowRootVersion)) {
            throw new RangeError("cannot use a version before the shallow history root");
        }
    }
    #causalVersionAt(frontiers) {
        // Most documents have no shallow root; avoid the entry array and map
        // copies in that case.
        const version = this.#shallowStartVersion.length() === 0
            ? new Map()
            : new Map(this.#shallowStartVersion
                ._codecEntriesUnsorted()
                .map(({ peer, counter }) => [peer, counter]));
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
    #dependencyVersion(change) {
        const cached = this.#dependencyVersionCache.get(change);
        if (cached !== undefined)
            return cached;
        const version = this.#shallowStartVersion.length() === 0
            ? new Map()
            : new Map(this.#shallowStartVersion
                ._codecEntriesUnsorted()
                .map(({ peer, counter }) => [peer, counter]));
        for (const dependency of change.dependencies) {
            const dependencyRecord = this.#recordContaining(dependency);
            if (dependencyRecord !== undefined && dependencyRecord.change !== change) {
                for (const [peer, counter] of this.#dependencyVersion(dependencyRecord.change)) {
                    version.set(peer, Math.max(version.get(peer) ?? 0, counter));
                }
            }
            version.set(dependency.peer, Math.max(version.get(dependency.peer) ?? 0, dependency.counter + 1));
        }
        this.#dependencyVersionCache.set(change, version);
        return version;
    }
    #frontiersForVersion(version) {
        const candidates = new Map();
        const candidateByPeer = new Map();
        const known = this.#historyVersion();
        for (const { peer, counter } of version._codecEntriesUnsorted()) {
            const end = Math.min(counter, known.get(peer) ?? 0);
            if (end === 0)
                continue;
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
                if (candidate !== undefined &&
                    !idsEqual(candidate, frontier) &&
                    candidate.counter < counter) {
                    candidates.delete(idKey(candidate));
                }
            }
        }
        return [...candidates.values()].sort(compareIds);
    }
    #frontiersCodec() {
        const candidates = new Map();
        if (this.#checkoutVersion !== undefined) {
            for (const frontier of this.#frontiersForVersion(this.#checkoutVersion)) {
                candidates.set(idKey(frontier), frontier);
            }
        }
        else if (this.#deferredSnapshotHistory !== undefined) {
            for (const frontier of this.#deferredSnapshotHistory.frontiers) {
                candidates.set(idKey(frontier), frontier);
            }
        }
        else {
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
    #lamportAt(id) {
        const record = this.#recordContaining(id);
        return record === undefined
            ? 0
            : record.change.lamport + (id.counter - record.change.id.counter);
    }
    #encodeUpdates(records) {
        const blocks = records.map((record) => encodeSingleChangeBlock(record.change, record.keys));
        return encodeDocument(EncodeMode.FastUpdates, encodeFastUpdatesBody(blocks));
    }
    #encodeSnapshot() {
        if (this.isShallow()) {
            return this.#encodeShallowSnapshot(this.shallowSinceFrontiers());
        }
        const historyEntries = this.#sortedHistory().map((record) => ({
            key: encodeChangeBlockKey(record.change.id),
            value: encodeSingleChangeBlock(record.change, record.keys),
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
    #encodeShallowSnapshot(requestedFrontiers) {
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
        const historyEntries = this.#recordsInVersionRange(startVersion, latestVersion).map((record) => ({
            key: encodeChangeBlockKey(record.change.id),
            value: encodeSingleChangeBlock(record.change, record.keys),
        }));
        historyEntries.push({
            key: VERSION_KEY,
            value: encodePostcardVersionVector(latestVersion.codecEntries()),
        }, {
            key: FRONTIERS_KEY,
            value: encodePostcardFrontiers(latestFrontiers),
        }, {
            key: START_VERSION_KEY,
            value: encodePostcardVersionVector(startVersion.codecEntries()),
        }, {
            key: START_FRONTIERS_KEY,
            value: encodePostcardFrontiers(startFrontiers),
        });
        return encodeDocument(EncodeMode.FastSnapshot, encodeFastSnapshotBody({
            oplog: encodeSstable(historyEntries, { compression: "auto" }),
            state: encodeStateSnapshotStore(latestStore, { compression: "auto" }),
            shallowRootState: encodeStateSnapshotStore(rootStore, {
                compression: "auto",
            }),
        }));
    }
    #calculateShallowStart(requested) {
        const parsed = requested.map(parseOpId);
        for (const frontier of requested) {
            if (this.#recordContaining(parseOpId(frontier)) === undefined) {
                throw new RangeError(`shallow snapshot frontier ${frontier.counter}@${frontier.peer} is unknown`);
            }
        }
        let start = parsed;
        if (start.length > 1) {
            const versions = start.map((frontier) => this.#causalVersionAt([frontier]));
            const peers = new Set(versions.flatMap((version) => [...version.keys()]));
            const common = new VersionVector();
            for (const peer of peers) {
                const counter = Math.min(...versions.map((version) => version.get(peer) ?? 0));
                if (counter > 0)
                    common.set(peer, counter);
            }
            const commonFrontiers = this.#frontiersForVersion(common);
            start = commonFrontiers.length === 1 ? commonFrontiers : [];
        }
        if (start.length === 1) {
            const operation = this.#operationAt(start[0]);
            if (operation?.content.type === "text-mark") {
                start = [{ peer: start[0].peer, counter: start[0].counter + 1 }];
            }
        }
        const candidateVersion = this.#causalVersionForKnownFrontiers(start);
        if (this.isShallow() &&
            !versionIncludes(candidateVersion, this.#shallowRootVersion)) {
            return this.#shallowRootFrontiers.map((id) => ({ ...id }));
        }
        return start;
    }
    #causalVersionForKnownFrontiers(frontiers) {
        const version = new VersionVector();
        for (const [peer, counter] of this.#causalVersionAt(frontiers)) {
            version.set(peer, counter);
        }
        return version;
    }
    #operationAt(id) {
        const record = this.#recordContaining(id);
        if (record === undefined)
            return undefined;
        let low = 0;
        let high = record.change.operations.length;
        while (low < high) {
            const middle = (low + high) >>> 1;
            if (record.change.operations[middle].counter <= id.counter)
                low = middle + 1;
            else
                high = middle;
        }
        const operation = low > 0 ? record.change.operations[low - 1] : undefined;
        return operation !== undefined && id.counter < operation.counter + operation.length
            ? operation
            : undefined;
    }
    #buildStateStore(frontiers) {
        const containers = [];
        for (const container of this.#containers.values()) {
            const id = container._codecId;
            if (id === undefined)
                continue;
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
    #containerState(container) {
        if (container instanceof LoroMap) {
            const peers = [];
            const peerIndices = new Map();
            const peerIndex = (peer) => {
                let index = peerIndices.get(peer);
                if (index === undefined) {
                    index = peers.length;
                    peers.push(peer);
                    peerIndices.set(peer, index);
                }
                return BigInt(index);
            };
            const values = [];
            const deletedKeys = [];
            const metadata = [];
            for (const [key, record] of container._entries) {
                if (record.deleted)
                    deletedKeys.push(key);
                else
                    values.push([key, this.#encodeSnapshotValue(record.rawValue ?? record.value)]);
                metadata.push({
                    key,
                    peerIndex: peerIndex(record.writer.peer),
                    lamport: BigInt(record.writer.lamport),
                });
            }
            return { kind: CodecContainerType.Map, values, deletedKeys, peers, metadata };
        }
        if (container instanceof LoroText) {
            const peers = [];
            const peerIndices = new Map();
            const peerIndex = (peer) => {
                let index = peerIndices.get(peer);
                if (index === undefined) {
                    index = peers.length;
                    peers.push(peer);
                    peerIndices.set(peer, index);
                }
                return BigInt(index);
            };
            const keys = [];
            const keyIndices = new Map();
            const keyIndex = (key) => {
                let index = keyIndices.get(key);
                if (index === undefined) {
                    index = keys.length;
                    keys.push(key);
                    keyIndices.set(key, index);
                }
                return index;
            };
            const spans = [];
            const marks = [];
            const metasAt = container._attributeMetasResolver();
            let active = new Map();
            const updateActiveStyles = (current) => {
                for (const [key, meta] of active) {
                    if (current.get(key) === meta)
                        continue;
                    spans.push({
                        peerIndex: peerIndex(meta.startId.peer),
                        counter: meta.startId.counter + 1,
                        lamportSub: meta.lamport - meta.startId.counter,
                        length: -1,
                    });
                }
                for (const [key, meta] of current) {
                    if (active.get(key) === meta)
                        continue;
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
                active = current;
            };
            const textChunks = [];
            let textChunk = "";
            let textChunkLength = 0;
            const appendText = (text, scalarLength) => {
                if (textChunkLength > 0 && textChunkLength + scalarLength > 1024) {
                    textChunks.push(textChunk);
                    textChunk = "";
                    textChunkLength = 0;
                }
                textChunk += text;
                textChunkLength += scalarLength;
                if (textChunkLength === 1024) {
                    textChunks.push(textChunk);
                    textChunk = "";
                    textChunkLength = 0;
                }
            };
            let cachedPeer;
            let cachedPeerIndex = 0n;
            const appendId = (peer, counter, lamport) => {
                if (cachedPeer !== peer) {
                    cachedPeer = peer;
                    cachedPeerIndex = peerIndex(peer);
                }
                const lamportSub = lamport - counter;
                const previous = spans[spans.length - 1];
                if (previous !== undefined &&
                    previous.length > 0 &&
                    previous.peerIndex === cachedPeerIndex &&
                    previous.counter + previous.length === counter &&
                    previous.lamportSub === lamportSub) {
                    previous.length += 1;
                }
                else {
                    spans.push({
                        peerIndex: cachedPeerIndex,
                        counter,
                        lamportSub,
                        length: 1,
                    });
                }
            };
            if (container._styleIndex.isEmpty) {
                container._forEachVisibleSnapshotData(appendText, appendId);
            }
            else {
                container._sequence.forEachVisible((element) => {
                    updateActiveStyles(metasAt(element));
                    appendId(element.id.peer, element.id.counter, element.lamport);
                    appendText(element.value, 1);
                });
            }
            updateActiveStyles(new Map());
            if (textChunkLength > 0)
                textChunks.push(textChunk);
            return {
                kind: CodecContainerType.Text,
                text: textChunks.join(""),
                peers,
                spans,
                keys,
                marks,
            };
        }
        if (container instanceof LoroTree) {
            // Rust's fast-tree decoder historically accepted the canonical snapshot
            // order: live roots/subtrees first, then deleted roots/subtrees, with each
            // sibling group ordered by fractional position and last-move writer.
            // Preserve that order so snapshots remain byte-level interoperable and so
            // parent-local ordered insertion never sees an insertion-order permutation.
            const records = container._recordsForSnapshot();
            const peers = [];
            const peerIndices = new Map();
            const peerIndex = (peer) => {
                let index = peerIndices.get(peer);
                if (index === undefined) {
                    index = peers.length;
                    peers.push(peer);
                    peerIndices.set(peer, index);
                }
                return BigInt(index);
            };
            const positions = [];
            const positionIndices = new Map();
            const recordIndices = new Map(records.map((record, index) => [idKey(record.id), index]));
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
                    const parentIndex = record.deleted
                        ? 1n
                        : record.parent === undefined
                            ? 0n
                            : BigInt((recordIndices.get(idKey(record.parent)) ?? -2) + 2);
                    return {
                        peerIndex: peerIndex(record.id.peer),
                        counter: record.id.counter,
                        parentIndexPlusTwo: parentIndex,
                        lastSetPeerIndex: peerIndex(record.writer.peer),
                        lastSetCounter: record.lastMoveId.counter,
                        lastSetLamportSub: record.writer.lamport - record.lastMoveId.counter,
                        fractionalIndexIndex: positionIndex,
                    };
                }),
                positions,
                reserved: new Uint8Array(),
            };
        }
        if (container instanceof LoroCounter) {
            float64BitsScratch.setFloat64(0, container.value, true);
            return {
                kind: CodecContainerType.Counter,
                bits: float64BitsScratch.getBigUint64(0, true),
            };
        }
        if (!(container instanceof LoroList)) {
            throw new TypeError(`unsupported container kind ${container.kind()}`);
        }
        const visible = container._visibleElements();
        const peers = [];
        const peerIndices = new Map();
        const peerIndex = (peer) => {
            let index = peerIndices.get(peer);
            if (index === undefined) {
                index = peers.length;
                peers.push(peer);
                peerIndices.set(peer, index);
            }
            return BigInt(index);
        };
        if (container instanceof LoroMovableList) {
            // The state-snapshot encoder only reads item fields, so every slot can
            // reference the same frozen item instead of allocating one per element.
            const items = [SHARED_MOVABLE_LIST_STATE_ITEM];
            for (let index = 0; index < visible.length; index += 1) {
                items.push(SHARED_MOVABLE_LIST_STATE_ITEM);
            }
            return {
                kind: CodecContainerType.MovableList,
                values: visible.map((element) => this.#encodeSnapshotValue(element.value)),
                peers,
                items,
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
    #encodeSnapshotValue(value) {
        if (isContainer(value)) {
            if (value._codecId === undefined)
                throw new Error("detached child in attached state");
            return { type: "container", value: value._codecId };
        }
        if (value === null)
            return { type: "null" };
        if (typeof value === "boolean")
            return { type: "bool", value };
        if (typeof value === "number") {
            return Number.isSafeInteger(value)
                ? { type: "i64", value: BigInt(value) }
                : { type: "double", value };
        }
        if (typeof value === "string")
            return { type: "string", value };
        if (value instanceof Uint8Array)
            return { type: "binary", value: value.slice() };
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
    #hydrateState(store, textSnapshotIds = validateStateSnapshotTextIds(store)) {
        if (store.kind !== "sstable")
            return;
        let remainingTextCounterSlots = 10000000;
        for (const { id } of store.containers)
            this.#getOrCreateContainer(id);
        for (const { id, wrapper } of store.containers) {
            const container = this.#getOrCreateContainer(id);
            const parent = wrapper.parent === undefined
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
                    const item = metadata.get(key);
                    const rawValue = this.#decodeSnapshotValue(value, container);
                    container._applyValue(key, this.#materializeMapValue(container, key, rawValue), {
                        peer: state.peers[Number(item.peerIndex)],
                        lamport: Number(item.lamport),
                    }, rawValue);
                }
                for (const key of state.deletedKeys) {
                    const item = metadata.get(key);
                    container._applyDelete(key, {
                        peer: state.peers[Number(item.peerIndex)],
                        lamport: Number(item.lamport),
                    });
                }
            }
            else if (container instanceof LoroText &&
                state.kind === CodecContainerType.Text) {
                const snapshotIds = textSnapshotIds.get(state);
                for (const { peer, end, visible } of snapshotIds.reservations) {
                    if (end > 0 && end <= visible * 4 + 4096 && end <= remainingTextCounterSlots) {
                        container._sequence.reservePeerCounters(peer, end);
                        remainingTextCounterSlots -= end;
                    }
                }
                container._beginValidatedSnapshotLoad();
                const styleRuns = [];
                const stylesById = new Map();
                const active = new Map();
                let textUtf16Offset = 0;
                let chunkTextStart = 0;
                const chunkPeers = [];
                const chunkCounters = [];
                const chunkLamports = [];
                const flushChunk = () => {
                    if (chunkPeers.length === 0)
                        return;
                    container._insertValidatedSnapshotChunk(state.text.slice(chunkTextStart, textUtf16Offset), chunkPeers, chunkCounters, chunkLamports);
                    chunkPeers.length = 0;
                    chunkCounters.length = 0;
                    chunkLamports.length = 0;
                    chunkTextStart = textUtf16Offset;
                };
                let markIndex = 0;
                for (const span of state.spans) {
                    const peer = state.peers[Number(span.peerIndex)];
                    if (span.length === 0) {
                        const mark = state.marks[markIndex++];
                        const key = state.keys[mark.keyIndex];
                        const value = this.#decodeSnapshotValue(mark.value);
                        const meta = {
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
                                if (index >= 0)
                                    stack.splice(index, 1);
                                if (stack.length === 0)
                                    active.delete(style.key);
                            }
                        }
                        continue;
                    }
                    if (span.length < -1)
                        continue;
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
                        // Inline advanceUnicodeScalars(state.text, textUtf16Offset, 1).
                        if (textUtf16Offset >= state.text.length) {
                            throw new Error("text snapshot span exceeds the decoded text");
                        }
                        const first = state.text.charCodeAt(textUtf16Offset);
                        textUtf16Offset += 1;
                        if (first >= 0xd800 &&
                            first <= 0xdbff &&
                            textUtf16Offset < state.text.length &&
                            state.text.charCodeAt(textUtf16Offset) >= 0xdc00 &&
                            state.text.charCodeAt(textUtf16Offset) <= 0xdfff) {
                            textUtf16Offset += 1;
                        }
                        chunkPeers.push(peer);
                        chunkCounters.push(span.counter + offset);
                        chunkLamports.push(span.counter + offset + span.lamportSub);
                        if (chunkPeers.length === 32)
                            flushChunk();
                    }
                }
                flushChunk();
                if (textUtf16Offset !== state.text.length) {
                    throw new Error("text snapshot spans do not consume the decoded text");
                }
                container._endValidatedSnapshotLoad();
                for (const { run, key, meta } of styleRuns) {
                    container._styleIndex.add([run], key, meta);
                }
                container._attributeHistoryComplete = false;
            }
            else if (container instanceof LoroTree &&
                state.kind === CodecContainerType.Tree) {
                const records = [];
                for (let index = 0; index < state.nodes.length; index += 1) {
                    const node = state.nodes[index];
                    const nodeId = {
                        peer: state.peers[Number(node.peerIndex)],
                        counter: node.counter,
                    };
                    const dataId = {
                        kind: "normal",
                        ...nodeId,
                        containerType: CodecContainerType.Map,
                    };
                    const record = {
                        id: nodeId,
                        parent: undefined,
                        position: state.positions[node.fractionalIndexIndex].slice(),
                        deleted: node.parentIndexPlusTwo === 1n,
                        writer: {
                            peer: state.peers[Number(node.lastSetPeerIndex)],
                            lamport: node.lastSetCounter + node.lastSetLamportSub,
                        },
                        lastMoveId: {
                            peer: state.peers[Number(node.lastSetPeerIndex)],
                            counter: node.lastSetCounter,
                        },
                        data: this.#getOrCreateContainer(dataId, container),
                    };
                    records.push(record);
                }
                for (let index = 0; index < state.nodes.length; index += 1) {
                    const parent = state.nodes[index].parentIndexPlusTwo;
                    if (parent >= 2n)
                        records[index].parent = records[Number(parent - 2n)].id;
                }
                for (const record of records)
                    container._setRecord(record);
            }
            else if (container instanceof LoroCounter &&
                state.kind === CodecContainerType.Counter) {
                float64BitsScratch.setBigUint64(0, state.bits, true);
                container._value = float64BitsScratch.getFloat64(0, true);
            }
            else if (container instanceof LoroMovableList &&
                state.kind === CodecContainerType.MovableList) {
                const ids = state.listItemIds.slice(0, state.values.length);
                container._insertVisible(0, state.values.map((value) => this.#decodeSnapshotValue(value, container)), ids.map((item) => ({
                    peer: state.peers[Number(item.peerIndex)],
                    counter: item.counter,
                })), ids.map((item) => item.counter + item.lamportSub));
                container._valueHistoryComplete = false;
                container._moveHistoryComplete = false;
            }
            else if (container instanceof LoroList &&
                state.kind === CodecContainerType.List) {
                container._insertVisible(0, state.values.map((value) => this.#decodeSnapshotValue(value, container)), state.ids.map((item) => ({
                    peer: state.peers[Number(item.peerIndex)],
                    counter: item.counter,
                })), state.ids.map((item) => item.counter + item.lamportSub));
            }
        }
    }
    #decodeSnapshotValue(value, parent) {
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
                return Object.fromEntries(value.value.map(([key, item]) => [key, this.#decodeSnapshotValue(item)]));
            case "container":
                return this.#getOrCreateContainer(value.value, parent);
        }
    }
    #materializeMapValue(parent, key, rawValue) {
        const parentId = parent._codecId;
        if (parentId === undefined)
            return rawValue;
        const childType = parseMergeableMarker(parentId, key, rawValue);
        if (childType === undefined)
            return rawValue;
        return this.#getOrCreateContainer(newMergeableContainerId(parentId, key, childType), parent);
    }
    #captureEventValues(records) {
        const values = new Map();
        for (const { change } of records) {
            for (const operation of change.operations) {
                const id = this.#containerKey(operation.container);
                if (values.has(id))
                    continue;
                const container = this.#containers.get(id);
                values.set(id, container === undefined ? undefined : containerEventValue(container));
            }
        }
        return values;
    }
    #recordConcurrentTreeSubjects(recording, added, beforeVersion) {
        const incomingVersion = new VersionVector();
        for (const { change } of added) {
            const length = changeLength(change);
            if (length === 0)
                continue;
            const frontier = {
                peer: change.id.peer,
                counter: change.id.counter + length - 1,
            };
            for (const [peer, counter] of this.#causalVersionAt([frontier])) {
                if (counter > (incomingVersion.get(peer) ?? 0)) {
                    incomingVersion.set(peer, counter);
                }
            }
        }
        // Rust calculates a concurrent import diff by retreating to the common
        // ancestor and replaying both branches. Tree operations from the local-only
        // branch can therefore produce visible reindex moves even though those
        // operations are not present in the imported update. Capture those subjects
        // so the incremental TS merge reports the same net transition.
        for (const { change } of this.#recordsInVersionRange(incomingVersion, beforeVersion)) {
            for (const operation of change.operations) {
                const content = operation.content;
                if (content.type !== "tree-create" &&
                    content.type !== "tree-move" &&
                    content.type !== "tree-delete") {
                    continue;
                }
                const container = this.#containers.get(this.#containerKey(operation.container));
                if (!(container instanceof LoroTree))
                    continue;
                const state = this.#treeEventState(recording, container);
                const nodeId = formatTreeId(content.subject);
                if (state.originals.has(nodeId))
                    continue;
                const record = container._nodes.get(nodeId);
                state.originals.set(nodeId, record === undefined || container._isRecordDeleted(record)
                    ? undefined
                    : this.#treeEventNode(container, record));
            }
        }
    }
    #captureContainerEventValues(ids) {
        const values = new Map();
        for (const id of ids) {
            const container = this.#containers.get(id);
            values.set(id, container === undefined ? undefined : containerEventValue(container));
        }
        return values;
    }
    #captureMapKeys(ids) {
        const keys = new Map();
        for (const id of ids) {
            const container = this.#containers.get(id);
            if (container instanceof LoroMap)
                keys.set(id, new Set(container._entries.keys()));
        }
        return keys;
    }
    #emit(by, origin, from, to, changed, beforeValues = new Map(), preparedDiffs = new Map()) {
        if (changed.size === 0 || !this.#hasEventSubscribers())
            return;
        const expandedChanged = new Set(changed);
        const revived = new Set();
        const calculatedDiffs = new Map(preparedDiffs);
        for (const id of changed) {
            const container = this.#containers.get(id);
            if (!(container instanceof LoroTree) || this._isContainerDeleted(container)) {
                continue;
            }
            const diff = calculatedDiffs.get(id) ?? containerDiff(container, beforeValues.get(id));
            calculatedDiffs.set(id, diff);
            if (diff.type !== "tree")
                continue;
            for (const item of diff.diff) {
                if (item.action !== "create")
                    continue;
                const record = container._nodes.get(item.target);
                if (record === undefined || container._isRecordDeleted(record))
                    continue;
                expandedChanged.add(record.data.id);
                revived.add(record.data.id);
            }
        }
        const events = [];
        for (const id of expandedChanged) {
            const container = this.#containers.get(id);
            if (container === undefined || this._isContainerDeleted(container))
                continue;
            const diff = calculatedDiffs.get(id) ??
                containerDiff(container, revived.has(id) ? undefined : beforeValues.get(id));
            if (isEmptyContainerDiff(diff))
                continue;
            events.push({ target: id, diff, path: containerPath(container) });
        }
        if (events.length === 0)
            return;
        const fromFormatted = from.map(formatOpId);
        const toFormatted = to.map(formatOpId);
        // Two explicit literals: the key set differs by `origin`, so each branch
        // keeps one stable object shape and avoids CopyDataProperties.
        const base = (origin === undefined
            ? { by, events, from: fromFormatted, to: toFormatted }
            : { by, origin, events, from: fromFormatted, to: toFormatted });
        for (const listener of this.#subscribers)
            listener(base);
        const relevantByTarget = new Map();
        for (const event of events) {
            let current = this.#containers.get(event.target);
            while (current !== undefined) {
                const target = current.id;
                if (this.#containerSubscribers.has(target)) {
                    const relevant = relevantByTarget.get(target);
                    if (relevant === undefined)
                        relevantByTarget.set(target, [event]);
                    else
                        relevant.push(event);
                }
                current = current.parent();
            }
        }
        for (const [target, relevant] of relevantByTarget) {
            const listeners = this.#containerSubscribers.get(target);
            if (listeners === undefined)
                continue;
            const batch = origin === undefined
                ? {
                    by,
                    events: relevant,
                    from: fromFormatted,
                    to: toFormatted,
                    currentTarget: target,
                }
                : {
                    by,
                    origin,
                    events: relevant,
                    from: fromFormatted,
                    to: toFormatted,
                    currentTarget: target,
                };
            for (const listener of listeners)
                listener(batch);
        }
    }
    #hasEventSubscribers() {
        return this.#subscribers.size > 0 || this.#containerSubscribers.size > 0;
    }
}
export class ChangeModifier {
    #options;
    constructor(options) {
        this.#options = options;
    }
    free() { }
    setMessage(message) {
        this.#options.message = message;
        return this;
    }
    setTimestamp(timestamp) {
        if (!Number.isFinite(timestamp))
            throw new TypeError("timestamp must be finite");
        this.#options.timestamp = timestamp;
        return this;
    }
}
export class Loro extends LoroDoc {
}
export function callPendingEvents() { }
export function LORO_VERSION() {
    return packageMetadata.version;
}
export function encodeFrontiers(frontiers) {
    return encodePostcardFrontiers(frontiers.map(parseOpId));
}
export function decodeFrontiers(bytes) {
    return decodePostcardFrontiers(bytes).map(formatOpId);
}
export function setDebug() { }
export function decodeImportBlobMeta(blob, checkChecksum = true) {
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
        const startFrontiers = new Map();
        for (const { change } of records) {
            for (const dependency of change.dependencies) {
                const start = startVersion.get(dependency.peer);
                if ((start !== undefined && start > dependency.counter) ||
                    (start === undefined && endVersion.get(dependency.peer) === undefined)) {
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
    const records = [];
    let startVersion = new VersionVector();
    let endVersion = new VersionVector();
    let startFrontiers = [];
    let endFrontiers = [];
    for (const entry of decodeSstable(snapshot.oplog)) {
        if (bytesEqual(entry.key, VERSION_KEY)) {
            endVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
        }
        else if (bytesEqual(entry.key, FRONTIERS_KEY)) {
            endFrontiers = decodePostcardFrontiers(entry.value);
        }
        else if (bytesEqual(entry.key, START_VERSION_KEY)) {
            startVersion = versionVectorFromCodec(decodePostcardVersionVector(entry.value));
        }
        else if (bytesEqual(entry.key, START_FRONTIERS_KEY)) {
            startFrontiers = decodePostcardFrontiers(entry.value);
        }
        else if (entry.key.length === 12) {
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
    const timestampAt = (frontiers) => {
        const targetByPeer = new Map(frontiers.map((id) => [id.peer, id]));
        let timestamp = 0;
        for (const { change } of records) {
            const frontier = targetByPeer.get(change.id.peer);
            if (frontier === undefined)
                continue;
            const end = change.id.counter + changeLength(change);
            if (frontier.counter >= change.id.counter && frontier.counter < end) {
                timestamp = Math.max(timestamp, Number(change.timestamp));
            }
        }
        return timestamp;
    };
    let latestRecord;
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
        endTimestamp: timestampAt(endFrontiers) || Number(latestRecord?.change.timestamp ?? 0n),
        mode: snapshot.shallowRootState.length > 0 ? "shallow-snapshot" : "snapshot",
        changeNum: records.length,
    };
}
function decodeHistoryRecordBlock(bytes) {
    const block = decodeChangeBlock(bytes);
    return block.changes.map((change) => ({
        change,
        keys: block.keys,
        keyIndices: undefined,
    }));
}
function versionVectorFromCodec(ids) {
    return new VersionVector(new Map(ids.map((id) => [peerIdToString(id.peer), id.counter])));
}
function diffContainerId(value) {
    if (isContainer(value))
        return value.id;
    if (typeof value !== "string" || !value.startsWith("🦜:"))
        return undefined;
    const id = value.slice("🦜:".length);
    return isContainerId(id) ? id : undefined;
}
function ensureMergeableChild(parent, key, type) {
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
function diffKindMismatch(container, diffType) {
    return new TypeError(`cannot apply a ${diffType} diff to a ${container.kind()} container`);
}
function createContainer(type) {
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
function registerKey(pending, key) {
    const existing = pending.keyIndices.get(key);
    if (existing !== undefined)
        return existing;
    const index = pending.keys.length;
    pending.keys.push(key);
    pending.keyIndices.set(key, index);
    return index;
}
function changeKey(id) {
    return `${id.peer}:${id.counter}`;
}
function idKey(id) {
    return `${id.peer}:${id.counter}`;
}
function frontierSetsEqual(left, right) {
    if (left.length !== right.length)
        return false;
    const rightIds = new Set(right.map((id) => idKey(parseOpId(id))));
    return left.every((id) => rightIds.has(idKey(parseOpId(id))));
}
const changeLengthCache = new WeakMap();
function changeLength(change) {
    const cached = changeLengthCache.get(change);
    if (cached !== undefined)
        return cached;
    const length = change.operations.reduce((sum, operation) => sum + operation.length, 0);
    changeLengthCache.set(change, length);
    return length;
}
function validateStateSnapshotTextIds(store) {
    const textSnapshotIds = new Map();
    if (store.kind !== "sstable")
        return textSnapshotIds;
    for (const { wrapper } of store.containers) {
        if (wrapper.state.kind === CodecContainerType.Text) {
            textSnapshotIds.set(wrapper.state, validateTextSnapshotIdRanges(wrapper.state.peers, wrapper.state.spans));
        }
    }
    return textSnapshotIds;
}
function validateTextSnapshotIdRanges(peers, spans) {
    const statsByPeer = new Map();
    for (const span of spans) {
        if (span.length <= 0)
            continue;
        const peerIndex = Number(span.peerIndex);
        const end = span.counter + span.length;
        if (!Number.isSafeInteger(peerIndex) ||
            peerIndex < 0 ||
            peerIndex >= peers.length ||
            !Number.isSafeInteger(span.counter) ||
            span.counter < 0 ||
            !Number.isSafeInteger(end) ||
            end <= span.counter ||
            end > 2147483647) {
            throw new Error("text snapshot contains an invalid ID range");
        }
        const peer = peers[peerIndex];
        let stats = statsByPeer.get(peer);
        if (stats === undefined) {
            stats = { peer, count: 0, end: 0, visible: 0, offset: 0 };
            statsByPeer.set(peer, stats);
        }
        stats.count += 1;
        stats.visible += span.length;
        if (!Number.isSafeInteger(stats.visible)) {
            throw new Error("text snapshot contains too many IDs for one peer");
        }
        if (end > stats.end)
            stats.end = end;
    }
    for (const stats of statsByPeer.values()) {
        stats.ranges = new BigUint64Array(stats.count);
    }
    for (const span of spans) {
        if (span.length <= 0)
            continue;
        const peerIndex = Number(span.peerIndex);
        const end = span.counter + span.length;
        const stats = statsByPeer.get(peers[peerIndex]);
        stats.ranges[stats.offset] = (BigInt(span.counter) << 32n) | BigInt(end);
        stats.offset += 1;
    }
    for (const stats of statsByPeer.values()) {
        const peerRanges = stats.ranges;
        peerRanges.sort();
        let previousEnd = -1;
        for (const packed of peerRanges) {
            const start = Number(packed >> 32n);
            const end = Number(packed & 0xffffffffn);
            if (start < previousEnd) {
                throw new Error("text snapshot contains duplicate sequence IDs");
            }
            previousEnd = end;
        }
    }
    return {
        reservations: [...statsByPeer.values()].map(({ peer, end, visible }) => ({
            peer,
            end,
            visible,
        })),
    };
}
function changedContainerIds(records) {
    return new Set(records.flatMap(({ change }) => change.operations.map((operation) => formatContainerId(operation.container))));
}
function longestIncreasingSubsequenceIndices(values) {
    if (values.length === 0)
        return new Set();
    const tails = [];
    const previous = new Int32Array(values.length);
    previous.fill(-1);
    for (let index = 0; index < values.length; index += 1) {
        let low = 0;
        let high = tails.length;
        while (low < high) {
            const middle = (low + high) >>> 1;
            if (values[tails[middle]] < values[index])
                low = middle + 1;
            else
                high = middle;
        }
        if (low > 0)
            previous[index] = tails[low - 1];
        tails[low] = index;
    }
    const indices = new Set();
    let index = tails.at(-1);
    while (index >= 0) {
        indices.add(index);
        index = previous[index];
    }
    return indices;
}
function movableMoveTransitionMode(retreat, forward, movePeers) {
    const changedMoveContainers = new Set();
    for (const { change } of [...retreat, ...forward]) {
        for (const operation of change.operations) {
            if (operation.content.type === "movable-list-move") {
                changedMoveContainers.add(formatContainerId(operation.container));
            }
        }
    }
    if (changedMoveContainers.size === 0)
        return "anchors";
    if (retreat.length > 0 && forward.length > 0)
        return "replay";
    for (const containerId of changedMoveContainers) {
        if ((movePeers.get(containerId)?.size ?? 0) > 1)
            return "replay";
    }
    return "anchors";
}
function hasMaterializedSequenceInsertions(records, containers) {
    for (const { change } of records) {
        for (const operation of change.operations) {
            const content = operation.content;
            if (content.type !== "text-insert" &&
                content.type !== "list-insert" &&
                content.type !== "movable-list-insert") {
                continue;
            }
            const container = containers.get(formatContainerId(operation.container));
            if ((container instanceof LoroText || container instanceof LoroList) &&
                container._sequence.findById({
                    peer: change.id.peer,
                    counter: operation.counter,
                }) !== undefined) {
                return true;
            }
        }
    }
    return false;
}
function canMergeChanges(left, right, interval) {
    return (right.id.peer === left.id.peer &&
        right.id.counter === left.id.counter + changeLength(left) &&
        right.dependencies.length === 1 &&
        right.dependencies[0].peer === left.id.peer &&
        right.timestamp - left.timestamp <= interval &&
        right.message === left.message);
}
function appendHistoryRecord(left, right, leftLength) {
    let keyIndices = left.keyIndices;
    let keys;
    if (keyIndices === undefined) {
        keys = [...left.keys];
        keyIndices = new Map();
        for (const [index, key] of keys.entries()) {
            if (!keyIndices.has(key))
                keyIndices.set(key, index);
        }
        left.keys = keys;
        left.keyIndices = keyIndices;
    }
    else {
        keys = left.keys;
    }
    // A change with no keys of its own needs no remapping, so its operations
    // array can be reused as-is; nothing downstream mutates it.
    let appendedOperations;
    if (right.keys.length === 0) {
        appendedOperations = right.change.operations;
    }
    else {
        const remappedKeyIndices = right.keys.map((key) => {
            const existing = keyIndices.get(key);
            if (existing !== undefined)
                return existing;
            const index = keys.length;
            keys.push(key);
            keyIndices.set(key, index);
            return index;
        });
        appendedOperations = right.change.operations.map((operation) => remapOperationKeys(operation, remappedKeyIndices));
    }
    const mergedOperations = left.change.operations;
    for (const operation of appendedOperations)
        mergedOperations.push(operation);
    const rightLength = changeLength(right.change);
    changeLengthCache.set(left.change, leftLength + rightLength);
    const appended = {
        change: {
            ...right.change,
            timestamp: left.change.timestamp,
            message: left.change.message,
            operations: appendedOperations,
        },
        keys,
        // Deliberately not the merged map: it is shared with `left` and keeps
        // growing, and this slice record never merges further.
        keyIndices: undefined,
    };
    changeLengthCache.set(appended.change, rightLength);
    return appended;
}
function cloneHistoryRecord(record) {
    return {
        change: { ...record.change, operations: [...record.change.operations] },
        keys: record.keys,
        keyIndices: undefined,
    };
}
function appendToTrailingListInsert(operations, operation) {
    const content = operation.content;
    if (content.type !== "list-insert" && content.type !== "movable-list-insert") {
        return false;
    }
    const previousIndex = operations.length - 1;
    const previous = operations[previousIndex];
    if (previous === undefined ||
        previous.content.type !== content.type ||
        !containerIdsEqual(previous.container, operation.container) ||
        previous.counter + previous.length !== operation.counter ||
        previous.content.position + previous.length !== content.position) {
        return false;
    }
    const values = previous.content.values;
    for (const value of content.values)
        values.push(value);
    operations[previousIndex] = {
        ...previous,
        length: previous.length + operation.length,
        content: { ...previous.content, values },
    };
    return true;
}
function remapOperationKeys(operation, remap) {
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
function remapChangeValueKeys(value, remap) {
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
function remapLoroValueKeys(value, remap) {
    if (value.type === "list") {
        return {
            ...value,
            value: value.value.map((item) => remapLoroValueKeys(item, remap)),
        };
    }
    if (value.type === "map") {
        return {
            ...value,
            value: value.value.map(([keyIndex, item]) => [
                remapKeyIndex(keyIndex, remap),
                remapLoroValueKeys(item, remap),
            ]),
        };
    }
    return value;
}
function remapKeyIndex(keyIndex, remap) {
    if (keyIndex < 0n || keyIndex > BigInt(Number.MAX_SAFE_INTEGER)) {
        throw new Error("change value map key index is out of range");
    }
    const mapped = remap[Number(keyIndex)];
    if (mapped === undefined) {
        throw new Error("change value map key index is out of range");
    }
    return BigInt(mapped);
}
const I64_MAX = 0x7fffffffffffffffn;
const I64_MIN = -0x8000000000000000n;
function numberToI64(value) {
    if (Number.isNaN(value))
        return 0n;
    if (value >= Number(I64_MAX))
        return I64_MAX;
    if (value <= Number(I64_MIN))
        return I64_MIN;
    return BigInt(Math.trunc(value));
}
function maxBigInt(left, right) {
    return left >= right ? left : right;
}
function sliceChange(change, from, to) {
    const totalLength = changeLength(change);
    if (!Number.isSafeInteger(from) ||
        !Number.isSafeInteger(to) ||
        from < 0 ||
        from >= to ||
        to > totalLength) {
        throw new RangeError("change slice is out of range");
    }
    const operations = [];
    let offset = 0;
    for (const operation of change.operations) {
        const operationStart = offset;
        const operationEnd = offset + operation.length;
        offset = operationEnd;
        if (operationEnd <= from)
            continue;
        if (operationStart >= to)
            break;
        const sliceFrom = Math.max(0, from - operationStart);
        const sliceTo = Math.min(operation.length, to - operationStart);
        if (sliceFrom === 0 && sliceTo === operation.length) {
            operations.push(operation);
        }
        else {
            operations.push(sliceOperation(operation, sliceFrom, sliceTo));
        }
    }
    return {
        ...change,
        id: { peer: change.id.peer, counter: change.id.counter + from },
        lamport: change.lamport + from,
        dependencies: from === 0
            ? change.dependencies
            : [{ peer: change.id.peer, counter: change.id.counter + from - 1 }],
        operations,
    };
}
// Slice a string by Unicode scalar offsets without materializing per-scalar
// strings. Out-of-range offsets clamp like Array.from(value).slice would.
function sliceUnicodeScalars(value, from, to) {
    let offset = 0;
    let scalar = 0;
    while (scalar < from && offset < value.length) {
        const first = value.charCodeAt(offset);
        offset +=
            first >= 0xd800 &&
                first <= 0xdbff &&
                offset + 1 < value.length &&
                value.charCodeAt(offset + 1) >= 0xdc00 &&
                value.charCodeAt(offset + 1) <= 0xdfff
                ? 2
                : 1;
        scalar += 1;
    }
    const start = offset;
    while (scalar < to && offset < value.length) {
        const first = value.charCodeAt(offset);
        offset +=
            first >= 0xd800 &&
                first <= 0xdbff &&
                offset + 1 < value.length &&
                value.charCodeAt(offset + 1) >= 0xdc00 &&
                value.charCodeAt(offset + 1) <= 0xdfff
                ? 2
                : 1;
        scalar += 1;
    }
    return value.slice(start, offset);
}
function sliceOperation(operation, from, to) {
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
                    value: sliceUnicodeScalars(content.value, from, to),
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
function versionIncludes(version, required) {
    return required
        ._codecEntriesUnsorted()
        .every(({ peer, counter }) => (version.get(peer) ?? 0) >= counter);
}
function historyVersionForRecords(records, base) {
    const version = base?.clone() ?? new VersionVector();
    for (const { change } of records) {
        const end = change.id.counter + changeLength(change);
        if (end > (version.get(change.id.peer) ?? 0)) {
            version.set(change.id.peer, end);
        }
    }
    return version;
}
function versionDistance(start, end) {
    let distance = 0;
    for (const { peer, counter } of end._codecEntriesUnsorted()) {
        distance += Math.max(0, counter - (start.get(peer) ?? 0));
    }
    return distance;
}
function mergeStateSnapshotStores(root, overlay) {
    if (root.kind !== "sstable")
        return overlay;
    if (overlay.kind !== "sstable") {
        return { kind: "sstable", frontiers: undefined, containers: root.containers };
    }
    const containers = new Map(root.containers.map((entry) => [formatContainerId(entry.id), entry]));
    for (const entry of overlay.containers) {
        containers.set(formatContainerId(entry.id), entry);
    }
    return {
        kind: "sstable",
        frontiers: undefined,
        containers: [...containers.values()],
    };
}
function compareIds(left, right) {
    return left.peer < right.peer
        ? -1
        : left.peer > right.peer
            ? 1
            : left.counter - right.counter;
}
function compareHistoryRecords(left, right) {
    return (left.change.lamport - right.change.lamport ||
        compareIds(left.change.id, right.change.id));
}
function compareHistoryOperations(leftChange, leftOperation, rightChange, rightOperation) {
    return compareWriter(operationWriter(leftChange, leftOperation), operationWriter(rightChange, rightOperation));
}
function lowerBoundHistory(records, counter) {
    let low = 0;
    let high = records.length;
    while (low < high) {
        const middle = (low + high) >>> 1;
        if (records[middle].change.id.counter < counter)
            low = middle + 1;
        else
            high = middle;
    }
    return low;
}
function lowerBoundOperation(operations, counter) {
    let low = 0;
    let high = operations.length;
    while (low < high) {
        const middle = (low + high) >>> 1;
        if (operations[middle].counter < counter)
            low = middle + 1;
        else
            high = middle;
    }
    return low;
}
function lowerBoundIndexedOperation(operations, counter) {
    let low = 0;
    let high = operations.length;
    while (low < high) {
        const middle = (low + high) >>> 1;
        if (operations[middle].operation.counter < counter)
            low = middle + 1;
        else
            high = middle;
    }
    return low;
}
function lowerBoundWriter(operations, writer) {
    let low = 0;
    let high = operations.length;
    while (low < high) {
        const middle = (low + high) >>> 1;
        if (compareWriter(operations[middle].writer, writer) < 0)
            low = middle + 1;
        else
            high = middle;
    }
    return low;
}
function latestIncludedOperation(history, version) {
    if (history === undefined)
        return undefined;
    let latest;
    for (const [peer, operations] of history.byPeer) {
        const index = lowerBoundIndexedOperation(operations, version.get(peer) ?? 0) - 1;
        const operation = operations[index];
        if (operation !== undefined &&
            (latest === undefined || compareWriter(latest.writer, operation.writer) < 0)) {
            latest = operation;
        }
    }
    return latest;
}
function latestIncludedTreePlacement(history, version, before) {
    if (history?.placementsByPeer === undefined)
        return undefined;
    let latest;
    for (const [peer, operations] of history.placementsByPeer) {
        const index = Math.min(lowerBoundIndexedOperation(operations, version.get(peer) ?? 0), lowerBoundWriter(operations, before)) - 1;
        const operation = operations[index];
        if (operation !== undefined &&
            (latest === undefined || compareWriter(latest.writer, operation.writer) < 0)) {
            latest = operation;
        }
    }
    return latest;
}
function latestIncludedSequenceValue(history, version) {
    if (history === undefined)
        return undefined;
    for (let index = history.length - 1; index >= 0; index -= 1) {
        const meta = history[index];
        if (meta.id.counter < (version.get(meta.id.peer) ?? 0))
            return meta;
    }
    return undefined;
}
function hasSequenceValueMeta(history, writer, peer, counter) {
    if (history === undefined)
        return false;
    let low = 0;
    let high = history.length;
    while (low < high) {
        const middle = (low + high) >>> 1;
        const meta = history[middle];
        if (compareWriter({ peer: meta.id.peer, lamport: meta.lamport }, writer) < 0) {
            low = middle + 1;
        }
        else {
            high = middle;
        }
    }
    const meta = history[low];
    return meta?.id.peer === peer && meta.id.counter === counter;
}
function findSequenceMoveMeta(history, writer) {
    const index = findSequenceMoveMetaIndex(history, writer);
    return index < 0 ? undefined : history[index];
}
function findSequenceMoveMetaIndex(history, writer) {
    if (history === undefined)
        return -1;
    let low = 0;
    let high = history.length;
    while (low < high) {
        const middle = (low + high) >>> 1;
        const meta = history[middle];
        if (compareWriter({ peer: meta.id.peer, lamport: meta.lamport }, writer) < 0) {
            low = middle + 1;
        }
        else {
            high = middle;
        }
    }
    return low < history.length ? low : -1;
}
function counterDelta(content) {
    if (content.value.type === "double")
        return content.value.value;
    if (content.value.type === "i64")
        return Number(content.value.value);
    if (content.value.type === "delta-int")
        return content.value.value;
    return undefined;
}
function publicChange(change) {
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
function historyRecordsToJsonSchema(records, startVersion, withPeerCompression, nullMessage = true) {
    const peers = collectJsonPeers(records, startVersion);
    const peerMap = withPeerCompression
        ? new Map(peers.map((peer, index) => [peer, BigInt(index)]))
        : undefined;
    return {
        schema_version: 1,
        start_version: Object.fromEntries(startVersion
            .codecEntries()
            .map(({ peer, counter }) => [(peerMap?.get(peer) ?? peer).toString(), counter])),
        peers: withPeerCompression ? peers.map(peerIdToString) : null,
        changes: records.map((record) => historyRecordToJsonChange(record, peerMap, nullMessage)),
    };
}
function historyRecordToJsonChange(record, peerMap, nullMessage) {
    const { change } = record;
    return {
        id: formatJsonOpId(change.id, peerMap),
        timestamp: Number(change.timestamp),
        deps: change.dependencies.map((id) => formatJsonOpId(id, peerMap)),
        lamport: change.lamport,
        msg: change.message ?? (nullMessage ? null : undefined),
        ops: change.operations.map((operation) => decodedOperationToJson(operation, record.keys, change.id.peer, peerMap)),
    };
}
function collectJsonPeers(records, startVersion) {
    const peers = new Set(startVersion._codecEntriesUnsorted().map(({ peer }) => peer));
    const addId = (id) => {
        if (id !== undefined)
            peers.add(id.peer);
    };
    for (const { change } of records) {
        addId(change.id);
        for (const dependency of change.dependencies)
            addId(dependency);
        for (const operation of change.operations) {
            if (operation.container.kind === "normal")
                peers.add(operation.container.peer);
            const content = operation.content;
            if (content.type === "text-delete" ||
                content.type === "list-delete" ||
                content.type === "movable-list-delete") {
                addId(content.startId);
            }
            else if (content.type === "tree-create" || content.type === "tree-move") {
                addId(content.subject);
                addId(content.parent);
            }
            else if (content.type === "tree-delete") {
                addId(content.subject);
            }
            else if (content.type === "movable-list-move" ||
                content.type === "movable-list-set") {
                peers.add(content.elementId.peer);
            }
        }
    }
    return [...peers].sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
}
function decodedOperationToJson(operation, keys, changePeer, peerMap) {
    const operationId = { peer: changePeer, counter: operation.counter };
    const content = operation.content;
    let json;
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
                value: content.values.map((value, index) => changeLoroValueToJson(value, keys, { peer: changePeer, counter: operation.counter + index }, peerMap)),
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
                elem_id: formatJsonOpId({ peer: content.elementId.peer, counter: content.elementId.lamport }, peerMap),
            };
            break;
        case "movable-list-set":
            json = {
                type: "set",
                elem_id: formatJsonOpId({ peer: content.elementId.peer, counter: content.elementId.lamport }, peerMap),
                value: changeLoroValueToJson(content.value, keys, operationId, peerMap),
            };
            break;
        case "tree-create":
        case "tree-move":
            json = {
                type: content.type === "tree-create" ? "create" : "move",
                target: formatJsonTreeId(content.subject, peerMap),
                parent: content.parent === undefined ? null : formatJsonTreeId(content.parent, peerMap),
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
            }
            else if (value.type === "delta-int") {
                json = { type: "counter", value: value.value, prop: content.property };
            }
            else {
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
function changeLoroValueToJson(value, keys, operationId, peerMap) {
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
            return value.value.map((item, index) => changeLoroValueToJson(item, keys, { peer: operationId.peer, counter: operationId.counter + index }, peerMap));
        case "map":
            return Object.fromEntries(value.value.map(([keyIndex, item]) => {
                const key = keys[Number(keyIndex)];
                if (key === undefined)
                    throw new RangeError("JSON value key is out of range");
                return [key, changeLoroValueToJson(item, keys, operationId, peerMap)];
            }));
        case "container-type": {
            const child = {
                kind: "normal",
                ...operationId,
                containerType: containerTypeFromRawByte(value.value),
            };
            return `🦜:${formatJsonContainerId(child, peerMap)}`;
        }
    }
}
function changeValueToJsonUnknown(value) {
    if (typeof value !== "object" || value === null)
        return value;
    if ("value" in value) {
        const inner = value.value;
        return typeof inner === "bigint" ? Number(inner) : inner;
    }
    if ("data" in value)
        return value.data.slice();
    return value;
}
function formatJsonOpId(id, peerMap) {
    return `${id.counter}@${(peerMap?.get(id.peer) ?? id.peer).toString()}`;
}
function formatJsonTreeId(id, peerMap) {
    return formatJsonOpId(id, peerMap);
}
function formatJsonContainerId(id, peerMap) {
    return formatContainerId(id.kind === "normal" ? { ...id, peer: peerMap?.get(id.peer) ?? id.peer } : id);
}
function jsonSchemaToHistoryRecords(schema) {
    if (!Array.isArray(schema.changes))
        throw new TypeError("JSON changes must be an array");
    const peers = Array.isArray(schema.peers) ? schema.peers.map(parsePeerId) : undefined;
    const resolvePeer = (peer) => {
        if (peers === undefined)
            return peer;
        const resolved = peers[Number(peer)];
        if (resolved === undefined)
            throw new RangeError(`JSON peer index ${peer} is missing`);
        return resolved;
    };
    const parseJsonId = (value) => {
        const parsed = parseTreeId(value);
        return { peer: resolvePeer(parsed.peer), counter: parsed.counter };
    };
    const parseJsonContainer = (value) => {
        const parsed = parseContainerId(value);
        return parsed.kind === "normal"
            ? { ...parsed, peer: resolvePeer(parsed.peer) }
            : parsed;
    };
    return schema.changes.map((change) => {
        const id = parseJsonId(change.id);
        const keys = [];
        const keyIndices = new Map();
        const registerJsonKey = (key) => {
            const existing = keyIndices.get(key);
            if (existing !== undefined)
                return existing;
            const index = keys.length;
            keys.push(key);
            keyIndices.set(key, index);
            return index;
        };
        const operations = change.ops.map((operation) => jsonOperationToDecoded(operation, id.peer, resolvePeer, parseJsonContainer, registerJsonKey));
        let expectedCounter = id.counter;
        for (const operation of operations) {
            if (operation.counter !== expectedCounter) {
                throw new RangeError(`JSON operation counter ${operation.counter} does not follow ${expectedCounter}`);
            }
            expectedCounter += operation.length;
        }
        return {
            change: {
                id,
                timestamp: BigInt(Math.trunc(change.timestamp)),
                dependencies: change.deps.map(parseJsonId),
                lamport: change.lamport,
                message: change.msg ?? undefined,
                operations,
            },
            keys,
            keyIndices: undefined,
        };
    });
}
function jsonOperationToDecoded(operation, changePeer, resolvePeer, parseJsonContainer, registerKey) {
    const container = parseJsonContainer(operation.container);
    const containerType = codecTypeToPublic(container.containerType);
    const content = operation.content;
    const operationId = { peer: changePeer, counter: operation.counter };
    const parseId = (value) => {
        if (typeof value !== "string")
            throw new TypeError("JSON operation ID must be a string");
        const parsed = parseTreeId(value);
        return { peer: resolvePeer(parsed.peer), counter: parsed.counter };
    };
    const parseParent = (value) => value === null || value === undefined ? undefined : parseId(value);
    const encodeValue = (value, id = operationId) => jsonValueToChangeLoroValue(value, id, resolvePeer, registerKey);
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
            const values = content.value.map((value, index) => encodeValue(value, { peer: changePeer, counter: operation.counter + index }));
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
        const position = typeof content.fractional_index === "string"
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
        const value = typeof content === "number"
            ? content
            : typeof content.value === "number"
                ? content.value
                : Number.NaN;
        if (!Number.isFinite(value))
            throw new TypeError("counter JSON value must be finite");
        return {
            container,
            counter: operation.counter,
            length: 1,
            content: {
                type: "future",
                property: typeof content === "object" && content !== null && "prop" in content
                    ? requireJsonInteger(content.prop, "counter property")
                    : 0,
                value: { type: "double", value },
            },
        };
    }
    throw new TypeError("JSON updates do not support this container type");
}
function jsonValueToChangeLoroValue(value, operationId, resolvePeer, registerKey) {
    if (value === undefined || value === null)
        return { type: "null" };
    if (typeof value === "boolean")
        return { type: "bool", value };
    if (typeof value === "number") {
        if (!Number.isFinite(value))
            throw new TypeError("JSON update numbers must be finite");
        return Number.isSafeInteger(value)
            ? { type: "i64", value: BigInt(value) }
            : { type: "double", value };
    }
    if (typeof value === "bigint")
        return { type: "i64", value };
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
    if (value instanceof Uint8Array)
        return { type: "binary", value: value.slice() };
    if (Array.isArray(value)) {
        return {
            type: "list",
            value: value.map((item, index) => jsonValueToChangeLoroValue(item, { peer: operationId.peer, counter: operationId.counter + index }, resolvePeer, registerKey)),
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
function requireJsonInteger(value, label) {
    if (!Number.isSafeInteger(value))
        throw new TypeError(`${label} must be an integer`);
    return value;
}
function unicodeScalarLength(value) {
    let length = 0;
    for (let index = 0; index < value.length; index += 1) {
        const first = value.charCodeAt(index);
        if (first >= 0xd800 &&
            first <= 0xdbff &&
            index + 1 < value.length &&
            value.charCodeAt(index + 1) >= 0xdc00 &&
            value.charCodeAt(index + 1) <= 0xdfff) {
            index += 1;
        }
        length += 1;
    }
    return length;
}
function compareWriter(left, right) {
    return (left.lamport - right.lamport ||
        (left.peer < right.peer ? -1 : left.peer > right.peer ? 1 : 0));
}
function operationWriter(change, operation) {
    return {
        peer: change.id.peer,
        lamport: change.lamport + operation.counter - change.id.counter,
    };
}
function captureBlueprint(container) {
    if (container instanceof LoroText)
        return { kind: "Text", value: container.toDelta() };
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
function restoreBlueprint(container, blueprint) {
    if (container instanceof LoroMap) {
        for (const [key, value] of blueprint.value) {
            if (isContainer(value))
                container.setContainer(key, value);
            else
                container.set(key, value);
        }
    }
    else if (container instanceof LoroText) {
        container.applyDelta(blueprint.value);
    }
    else if (container instanceof LoroCounter) {
        if (blueprint.value !== 0)
            container.increment(blueprint.value);
    }
    else if (container instanceof LoroList) {
        for (const value of blueprint.value) {
            if (isContainer(value))
                container.pushContainer(value);
            else
                container.push(value);
        }
    }
}
function containerDepth(container) {
    let depth = 1;
    let parent = container.parent();
    while (parent !== undefined) {
        depth += 1;
        parent = parent.parent();
    }
    return depth;
}
function assertTextStyleExpand(value) {
    if (value !== "before" && value !== "after" && value !== "none" && value !== "both") {
        throw new TypeError(`invalid text style expand mode: ${value}`);
    }
}
function textStyleInfoByte(expand, deleting) {
    const effective = deleting
        ? expand === "none"
            ? "both"
            : expand === "both"
                ? "none"
                : expand
        : expand;
    return (0x80 |
        (effective === "before" || effective === "both" ? 0x02 : 0) |
        (effective === "after" || effective === "both" ? 0x04 : 0));
}
function generatePeerId() {
    const cryptoObject = globalThis.crypto;
    if (cryptoObject !== undefined) {
        const bytes = new Uint8Array(8);
        cryptoObject.getRandomValues(bytes);
        let value = 0n;
        for (const byte of bytes)
            value = (value << 8n) | BigInt(byte);
        if (value !== 0n)
            return value;
    }
    return fallbackPeer++;
}
function importStatus(records, pendingRecords) {
    // Group by the bigint peer first so the string form is built once per peer
    // rather than once per change.
    const spansByPeer = new Map();
    for (const { change } of records) {
        const start = change.id.counter;
        const end = start + changeLength(change);
        const current = spansByPeer.get(change.id.peer);
        if (current === undefined) {
            spansByPeer.set(change.id.peer, { start, end });
        }
        else {
            current.start = Math.min(start, current.start);
            current.end = Math.max(end, current.end);
        }
    }
    const success = new Map();
    for (const [peer, span] of spansByPeer) {
        success.set(peerIdToString(peer), span);
    }
    const pending = historySpans(pendingRecords);
    return { success, pending: pending.size === 0 ? null : pending };
}
function importStatusBetweenVersions(before, retainedStart, end) {
    const success = new Map();
    for (const { peer, counter } of end._codecEntriesUnsorted()) {
        const start = Math.max(before.get(peer) ?? 0, retainedStart.get(peer) ?? 0);
        if (counter > start) {
            success.set(peerIdToString(peer), { start, end: counter });
        }
    }
    return { success, pending: null };
}
function historySpans(records) {
    const spansByPeer = new Map();
    for (const { change } of records) {
        const start = change.id.counter;
        const end = start + changeLength(change);
        const current = spansByPeer.get(change.id.peer);
        if (current === undefined) {
            spansByPeer.set(change.id.peer, { start, end });
        }
        else {
            current.start = Math.min(start, current.start);
            current.end = Math.max(end, current.end);
        }
    }
    const spans = new Map();
    for (const [peer, span] of spansByPeer) {
        spans.set(peerIdToString(peer), span);
    }
    return spans;
}
function isEmptyJson(value) {
    return (value === "" ||
        (Array.isArray(value) && value.length === 0) ||
        (typeof value === "object" && value !== null && Object.keys(value).length === 0) ||
        value === 0);
}
function containerEventValue(container) {
    if (container instanceof LoroMap)
        return new Map(container.entries());
    if (container instanceof LoroText) {
        return {
            text: container.toString(),
            delta: container.toDelta(),
        };
    }
    if (container instanceof LoroTree) {
        return container.getNodes().map((node) => ({
            id: node.id,
            parent: node.parent()?.id,
            index: node.index(),
            fractionalIndex: node.fractionalIndex() ?? "",
        }));
    }
    if (container instanceof LoroCounter)
        return container.value;
    return container.toArray();
}
function containerDiff(container, beforeValue, mapOperationKeys = { from: undefined, to: undefined }) {
    if (container instanceof LoroMap) {
        const before = beforeValue instanceof Map ? beforeValue : new Map();
        const after = new Map(container.entries());
        const updated = {};
        const keys = new Set([
            ...before.keys(),
            ...after.keys(),
            ...(mapOperationKeys.from ?? []),
            ...(mapOperationKeys.to ?? []),
        ]);
        for (const key of keys) {
            const previous = before.get(key);
            const next = after.get(key);
            const tombstonePresenceChanged = !before.has(key) &&
                !after.has(key) &&
                mapOperationKeys.from?.has(key) !== mapOperationKeys.to?.has(key);
            if (tombstonePresenceChanged ||
                !eventValuesEqual(previous, next) ||
                before.has(key) !== after.has(key)) {
                updated[key] = next;
            }
        }
        return { type: "map", updated };
    }
    if (container instanceof LoroText) {
        const before = isTextEventValue(beforeValue)
            ? beforeValue
            : ({ text: "", delta: [] });
        const after = containerEventValue(container);
        if (eventValuesEqual(before.delta, after.delta))
            return { type: "text", diff: [] };
        const styled = [...before.delta, ...after.delta].some((item) => "attributes" in item && item.attributes !== undefined);
        if (styled) {
            return {
                type: "text",
                diff: [
                    ...(before.text.length === 0 ? [] : [{ delete: before.text.length }]),
                    ...after.delta,
                ],
            };
        }
        return { type: "text", diff: stringDelta(before.text, after.text) };
    }
    if (container instanceof LoroTree) {
        return {
            type: "tree",
            diff: treeDelta(Array.isArray(beforeValue) ? beforeValue : [], containerEventValue(container)),
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
        diff: listDelta(Array.isArray(beforeValue) ? beforeValue : [], container.toArray()),
    };
}
function diffForJson(diff) {
    if (diff.type === "map") {
        return {
            type: "map",
            updated: Object.fromEntries(Object.entries(diff.updated).map(([key, value]) => [key, jsonDiffValue(value)])),
        };
    }
    if (diff.type === "list") {
        return {
            type: "list",
            diff: diff.diff.map((operation) => "insert" in operation
                ? { ...operation, insert: operation.insert.map(jsonDiffValue) }
                : operation),
        };
    }
    return diff;
}
function jsonDiffValue(value) {
    if (isContainer(value))
        return `🦜:${value.id}`;
    if (value instanceof Uint8Array || value === null)
        return value;
    if (Array.isArray(value))
        return value.map(jsonDiffValue);
    if (typeof value === "object" && value !== null) {
        return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, jsonDiffValue(child)]));
    }
    return value;
}
// True when a UTF-16 boundary at `at` would split a surrogate pair.
function splitsSurrogatePair(value, at) {
    if (at <= 0 || at >= value.length)
        return false;
    const before = value.charCodeAt(at - 1);
    const after = value.charCodeAt(at);
    return before >= 0xd800 && before <= 0xdbff && after >= 0xdc00 && after <= 0xdfff;
}
function stringDelta(before, after) {
    // Scan the common prefix/suffix directly in UTF-16 code units and only
    // allocate the inserted substring. Boundaries are backed off to code-point
    // edges so the delta never splits a surrogate pair.
    let prefix = 0;
    const prefixLimit = Math.min(before.length, after.length);
    while (prefix < prefixLimit && before.charCodeAt(prefix) === after.charCodeAt(prefix)) {
        prefix += 1;
    }
    if (splitsSurrogatePair(before, prefix) || splitsSurrogatePair(after, prefix)) {
        prefix -= 1;
    }
    let suffix = 0;
    const suffixLimit = Math.min(before.length - prefix, after.length - prefix);
    while (suffix < suffixLimit &&
        before.charCodeAt(before.length - suffix - 1) ===
            after.charCodeAt(after.length - suffix - 1)) {
        suffix += 1;
    }
    if (splitsSurrogatePair(before, before.length - suffix) ||
        splitsSurrogatePair(after, after.length - suffix)) {
        suffix -= 1;
    }
    const diff = [];
    if (prefix > 0)
        diff.push({ retain: prefix });
    const deleted = before.length - prefix - suffix;
    if (deleted > 0)
        diff.push({ delete: deleted });
    const inserted = after.slice(prefix, after.length - suffix);
    if (inserted.length > 0)
        diff.push({ insert: inserted });
    return diff;
}
function listDelta(before, after) {
    let prefix = 0;
    while (prefix < before.length &&
        prefix < after.length &&
        eventValuesEqual(before[prefix], after[prefix])) {
        prefix += 1;
    }
    let suffix = 0;
    while (suffix < before.length - prefix &&
        suffix < after.length - prefix &&
        eventValuesEqual(before[before.length - suffix - 1], after[after.length - suffix - 1])) {
        suffix += 1;
    }
    const diff = [];
    if (prefix > 0)
        diff.push({ retain: prefix });
    const deleted = before.length - prefix - suffix;
    if (deleted > 0)
        diff.push({ delete: deleted });
    const inserted = after.slice(prefix, after.length - suffix);
    if (inserted.length > 0)
        diff.push({ insert: [...inserted] });
    return diff;
}
function treeDelta(before, after) {
    const beforeById = new Map(before.map((node) => [node.id, node]));
    const afterById = new Map(after.map((node) => [node.id, node]));
    const diff = [];
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
        }
        else if (previous.parent !== node.parent ||
            previous.index !== node.index ||
            previous.fractionalIndex !== node.fractionalIndex) {
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
function eventValuesEqual(left, right) {
    if (left === right)
        return true;
    // undefined compares equal to null, matching how the runtime encodes both
    // as the null Loro value. Stored values never contain undefined, so this
    // only normalizes user-provided input.
    if (left === undefined)
        left = null;
    if (right === undefined)
        right = null;
    if (left === right)
        return true;
    if (left instanceof Uint8Array && right instanceof Uint8Array) {
        return bytesEqual(left, right);
    }
    if (left instanceof Uint8Array || right instanceof Uint8Array)
        return false;
    if (isContainer(left) && isContainer(right))
        return left.id === right.id;
    if (isContainer(left) || isContainer(right))
        return false;
    if (Array.isArray(left) && Array.isArray(right)) {
        if (left.length !== right.length)
            return false;
        for (let index = 0; index < left.length; index += 1) {
            if (!eventValuesEqual(left[index], right[index]))
                return false;
        }
        return true;
    }
    if (Array.isArray(left) || Array.isArray(right))
        return false;
    if (left instanceof Map && right instanceof Map) {
        if (left.size !== right.size)
            return false;
        for (const [key, value] of left) {
            if (!right.has(key) || !eventValuesEqual(value, right.get(key)))
                return false;
        }
        return true;
    }
    if (left instanceof Map || right instanceof Map)
        return false;
    if (typeof left === "object" &&
        left !== null &&
        typeof right === "object" &&
        right !== null) {
        const leftRecord = left;
        const rightRecord = right;
        let leftCount = 0;
        for (const key in leftRecord) {
            leftCount += 1;
            if (!(key in rightRecord) || !eventValuesEqual(leftRecord[key], rightRecord[key])) {
                return false;
            }
        }
        let rightCount = 0;
        for (const _key in rightRecord)
            rightCount += 1;
        return leftCount === rightCount;
    }
    return false;
}
function subtractSequenceIdRuns(runs, removedRuns) {
    if (removedRuns.length === 0)
        return [...runs];
    const removalsByPeer = new Map();
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
            const previous = write > 0 ? removals[write - 1] : undefined;
            if (previous !== undefined && removal.start <= previous.end) {
                previous.end = Math.max(previous.end, removal.end);
            }
            else {
                removals[write++] = removal;
            }
        }
        removals.length = write;
    }
    const retained = [];
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
            if (removals[middle].end <= run.start.counter)
                low = middle + 1;
            else
                high = middle;
        }
        let cursor = run.start.counter;
        for (let index = low; index < removals.length; index += 1) {
            const removal = removals[index];
            if (removal.start >= end)
                break;
            if (cursor < removal.start) {
                retained.push({
                    start: { peer: run.start.peer, counter: cursor },
                    length: Math.min(removal.start, end) - cursor,
                });
            }
            cursor = Math.max(cursor, removal.end);
            if (cursor >= end)
                break;
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
function isTextEventValue(value) {
    return (typeof value === "object" &&
        value !== null &&
        typeof value.text === "string" &&
        Array.isArray(value.delta));
}
function isEmptyContainerDiff(diff) {
    if (diff.type === "map")
        return Object.keys(diff.updated).length === 0;
    if (diff.type === "counter")
        return diff.increment === 0;
    return diff.diff.length === 0;
}
function recoverParentBinding(child, parent) {
    if (parent instanceof LoroMap) {
        for (const [key, record] of parent._entries) {
            if (record.value !== child)
                continue;
            const binding = { kind: "map", key };
            child._setParentBinding(parent, binding);
            return binding;
        }
    }
    else if (parent instanceof LoroList) {
        for (const element of parent._sequence.all()) {
            if (element.value !== child)
                continue;
            const binding = { kind: "sequence", element };
            child._setParentBinding(parent, binding);
            return binding;
        }
    }
    else if (parent instanceof LoroTree) {
        for (const record of parent._nodes.values()) {
            if (record.data !== child)
                continue;
            const binding = { kind: "tree", record };
            child._setParentBinding(parent, binding);
            return binding;
        }
    }
    return undefined;
}
function containerPath(container) {
    const path = [];
    let current = container;
    while (current !== undefined) {
        const id = current._codecId;
        if (id?.kind === "root" && !isMergeableContainerId(id)) {
            path.unshift(id.name);
            break;
        }
        const parent = current.parent();
        const binding = current._parentLink?.binding ??
            (parent === undefined ? undefined : recoverParentBinding(current, parent));
        if (parent instanceof LoroMap) {
            if (binding?.kind === "map") {
                path.unshift(binding.key);
            }
        }
        else if (parent instanceof LoroList) {
            const index = binding?.kind === "sequence"
                ? parent._sequence.visibleIndexOf(binding.element)
                : undefined;
            if (index !== undefined && index >= 0)
                path.unshift(index);
        }
        else if (parent instanceof LoroTree) {
            if (binding?.kind === "tree")
                path.unshift(formatTreeId(binding.record.id));
            else if (id?.kind === "normal")
                path.unshift(formatTreeId(id));
        }
        current = parent;
    }
    return path;
}
function parseOptionalPathIndex(value) {
    if (!/^(0|[1-9]\d*)$/u.test(value))
        return undefined;
    const index = Number(value);
    return Number.isSafeInteger(index) ? index : undefined;
}
function parsePathIndex(value) {
    return parseOptionalPathIndex(value) ?? -1;
}
function treeNodeAtPath(tree, _parent, part) {
    if (part.includes("@"))
        return tree.getNodeByID(part);
    const index = parseOptionalPathIndex(part);
    return index === undefined ? undefined : tree._nodeAt(undefined, index);
}
