import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
import { ContainerType, EncodeMode, bytesEqual, decodeChangeBlockKey, decodeChangeBlock, decodeChangeKeys, decodeChangeValueContent, decodeColumnarVecMaybeWrapped, decodeContainerId, decodeContainerArena, decodeDocument, decodeDeleteStartIds, decodeEncodedChangeBlock, decodeEncodedOperations, decodeFastSnapshot, decodeFastUpdates, decodeDeltaRleI32, decodeDeltaRleU32, decodeChangesHeader, decodeChangesMetadata, decodePositionArena, decodePostcardFrontiers, decodePostcardVersionVector, decodeRleU8, decodeRleU32, decodeSstable, decodeStateSnapshotStore, encodeEncodedChangeBlock, encodeChangeBlock, encodeFastSnapshot, encodeFastUpdates, encodeSstable, encodeStateSnapshotStore, } from "../src/codec/index";
const fixture = (name) => new Uint8Array(readFileSync(new URL(`./fixtures/rust/${name}`, import.meta.url)));
const textEncoder = new TextEncoder();
const VV_KEY = textEncoder.encode("vv");
const FRONTIERS_KEY = textEncoder.encode("fr");
describe("current Rust interoperability", () => {
    test("reads and losslessly reframes a Rust FastUpdates blob", () => {
        const bytes = fixture("updates.blob");
        expect(decodeDocument(bytes).mode).toBe(EncodeMode.FastUpdates);
        const blocks = decodeFastUpdates(bytes);
        expect(blocks.length).toBeGreaterThan(1);
        expect(encodeFastUpdates(blocks)).toEqual(bytes);
    });
    test("reads every Rust encoded change block and its operation columns", () => {
        const blocks = decodeFastUpdates(fixture("updates.blob"));
        let operationCount = 0;
        for (const bytes of blocks) {
            const block = decodeEncodedChangeBlock(bytes);
            expect(encodeEncodedChangeBlock(block)).toEqual(bytes);
            expect(block.changeCount).toBeGreaterThan(0);
            const columns = decodeColumnarVecMaybeWrapped(block.operations);
            expect(columns).toHaveLength(4);
            const containerIndices = decodeDeltaRleU32(columns[0]);
            const props = decodeDeltaRleI32(columns[1]);
            const valueTypes = decodeRleU8(columns[2]);
            const lengths = decodeRleU32(columns[3]);
            expect(props).toHaveLength(containerIndices.length);
            expect(valueTypes).toHaveLength(containerIndices.length);
            expect(lengths).toHaveLength(containerIndices.length);
            operationCount += lengths.length;
        }
        expect(operationCount).toBeGreaterThan(40);
    });
    test("decodes every semantic table in the Rust change blocks", () => {
        const blocks = decodeFastUpdates(fixture("updates.blob"));
        let changeCount = 0;
        let nestedContainerCount = 0;
        const valueTypes = new Set();
        for (const bytes of blocks) {
            const block = decodeEncodedChangeBlock(bytes);
            const header = decodeChangesHeader(block.header, {
                changeCount: block.changeCount,
                counterStart: block.counterStart,
                counterLength: block.counterLength,
                lamportStart: block.lamportStart,
                lamportLength: block.lamportLength,
            });
            const metadata = decodeChangesMetadata(block.changeMetadata, block.changeCount);
            const keys = decodeChangeKeys(block.keys);
            const containers = decodeContainerArena(block.containerIds, header.peers, keys);
            const positions = decodePositionArena(block.positions);
            const operations = decodeEncodedOperations(block.operations);
            const deletes = decodeDeleteStartIds(block.deleteStartIds);
            let remainingValues = block.values;
            for (const operation of operations) {
                const [value, remaining] = decodeChangeValueContent(operation.valueType, remainingValues);
                valueTypes.add(value.type);
                remainingValues = remaining;
            }
            expect(header.lengths).toHaveLength(block.changeCount);
            expect(header.dependencies).toHaveLength(block.changeCount);
            expect(metadata.timestamps).toHaveLength(block.changeCount);
            expect(metadata.commitMessages).toHaveLength(block.changeCount);
            expect(operations.reduce((sum, operation) => sum + operation.length, 0)).toBe(block.counterLength);
            expect(operations.every((operation) => operation.containerIndex < containers.length)).toBe(true);
            expect(deletes.every((entry) => entry.peerIndex < BigInt(header.peers.length))).toBe(true);
            expect(remainingValues).toHaveLength(0);
            changeCount += block.changeCount;
            nestedContainerCount += containers.filter((container) => container.kind === "normal").length;
            expect(positions.length).toBeGreaterThanOrEqual(0);
        }
        expect(changeCount).toBeGreaterThanOrEqual(5);
        expect(nestedContainerCount).toBeGreaterThan(0);
        expect([...valueTypes]).toEqual(expect.arrayContaining(["loro-value", "string", "raw-tree-move"]));
    });
    test("semantically decodes and re-encodes every Rust change block", () => {
        const operationTypes = new Set();
        let operationCount = 0;
        for (const bytes of decodeFastUpdates(fixture("updates.blob"))) {
            const decoded = decodeChangeBlock(bytes);
            for (const change of decoded.changes) {
                for (const operation of change.operations) {
                    operationTypes.add(operation.content.type);
                    operationCount += 1;
                }
            }
            const decodedAgain = decodeChangeBlock(encodeChangeBlock(decoded));
            expect(decodedAgain.changes).toEqual(decoded.changes);
        }
        expect(operationCount).toBeGreaterThan(40);
        expect([...operationTypes]).toEqual(expect.arrayContaining([
            "map-insert",
            "text-insert",
            "list-insert",
            "movable-list-insert",
            "tree-create",
        ]));
    });
    test("reads and losslessly reframes a Rust FastSnapshot blob", () => {
        const bytes = fixture("snapshot.blob");
        expect(decodeDocument(bytes).mode).toBe(EncodeMode.FastSnapshot);
        const snapshot = decodeFastSnapshot(bytes);
        expect(snapshot.oplog.length).toBeGreaterThan(0);
        expect(snapshot.state.length).toBeGreaterThan(0);
        expect(snapshot.shallowRootState).toHaveLength(0);
        expect(encodeFastSnapshot(snapshot)).toEqual(bytes);
    });
    test("decodes every Rust ChangeStore and state SSTable block", () => {
        const snapshot = decodeFastSnapshot(fixture("snapshot.blob"));
        const history = decodeSstable(snapshot.oplog);
        const state = decodeSstable(snapshot.state);
        const utf8 = new TextDecoder();
        const historyKeys = history.map((entry) => utf8.decode(entry.key));
        expect(historyKeys).toContain("vv");
        expect(historyKeys).toContain("fr");
        expect(history.length).toBeGreaterThan(2);
        expect(state.length).toBeGreaterThan(5);
    });
    test("decodes semantic keys and versions from a Rust snapshot", () => {
        const snapshot = decodeFastSnapshot(fixture("snapshot.blob"));
        const history = decodeSstable(snapshot.oplog);
        const versionEntry = history.find((entry) => bytesEqual(entry.key, VV_KEY));
        const frontiersEntry = history.find((entry) => bytesEqual(entry.key, FRONTIERS_KEY));
        expect(versionEntry).toBeDefined();
        expect(frontiersEntry).toBeDefined();
        const version = decodePostcardVersionVector(versionEntry.value);
        const frontiers = decodePostcardFrontiers(frontiersEntry.value);
        expect(version.length).toBeGreaterThan(0);
        expect(frontiers.length).toBeGreaterThan(0);
        const blockIds = history
            .filter((entry) => !bytesEqual(entry.key, VV_KEY) && !bytesEqual(entry.key, FRONTIERS_KEY))
            .map((entry) => decodeChangeBlockKey(entry.key));
        expect(blockIds.length).toBeGreaterThan(0);
        expect(blockIds.every((id) => id.counter >= 0)).toBe(true);
        const containerIds = decodeSstable(snapshot.state)
            .filter((entry) => !bytesEqual(entry.key, FRONTIERS_KEY))
            .map((entry) => decodeContainerId(entry.key));
        const rootNames = containerIds.flatMap((id) => (id.kind === "root" ? [id.name] : []));
        expect(rootNames).toEqual(expect.arrayContaining(["map", "list", "text", "mlist", "tree"]));
        expect(containerIds.some((id) => id.kind === "normal")).toBe(true);
    });
    test("semantically decodes and re-encodes every Rust container state", () => {
        const snapshot = decodeFastSnapshot(fixture("snapshot.blob"));
        const store = decodeStateSnapshotStore(snapshot.state);
        expect(store.kind).toBe("sstable");
        if (store.kind !== "sstable") {
            return;
        }
        const kinds = new Set(store.containers.map(({ wrapper }) => wrapper.state.kind));
        expect([...kinds]).toEqual(expect.arrayContaining([
            ContainerType.Map,
            ContainerType.List,
            ContainerType.Text,
            ContainerType.Tree,
            ContainerType.MovableList,
        ]));
        expect(store.containers.some(({ id }) => id.kind === "normal")).toBe(true);
        const encoded = encodeStateSnapshotStore(store, { compression: "none" });
        expect(decodeStateSnapshotStore(encoded)).toEqual(store);
    });
    test("rewrites Rust SSTables into independently decodable TypeScript SSTables", () => {
        const snapshot = decodeFastSnapshot(fixture("snapshot.blob"));
        for (const table of [snapshot.oplog, snapshot.state]) {
            const entries = decodeSstable(table);
            const rewritten = encodeSstable(entries, { compression: "none" });
            const decodedAgain = decodeSstable(rewritten);
            expect(decodedAgain).toHaveLength(entries.length);
            for (let index = 0; index < entries.length; index += 1) {
                expect(bytesEqual(decodedAgain[index].key, entries[index].key)).toBe(true);
                expect(bytesEqual(decodedAgain[index].value, entries[index].value)).toBe(true);
            }
        }
    });
});
