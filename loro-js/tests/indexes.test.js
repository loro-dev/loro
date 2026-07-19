import { describe, expect, test } from "vitest";
import { OrderedIndex } from "../src/runtime/ordered-index";
import { SequenceDeletionIndex } from "../src/runtime/sequence-deletion-index";
import { SequenceIndex } from "../src/runtime/sequence-index";
import { TextStyleIndex } from "../src/runtime/text-style-index";
describe("runtime indexes", () => {
    test("maintains sequence ranks, metrics, and id lookups", () => {
        const sequence = new SequenceIndex((element) => ({
            utf16: element.value.length,
            utf8: new TextEncoder().encode(element.value).length,
        }));
        const physical = [];
        let counter = 0;
        let random = 305419896;
        const nextRandom = () => {
            random ^= random << 13;
            random ^= random >>> 17;
            random ^= random << 5;
            return random >>> 0;
        };
        const visible = () => physical.filter((element) => !element.deleted);
        for (let step = 0; step < 2000; step += 1) {
            const current = visible();
            const operation = current.length === 0 ? 0 : nextRandom() % 3;
            if (operation === 0) {
                const position = nextRandom() % (current.length + 1);
                const value = ["a", "文", "🙂"][nextRandom() % 3];
                const element = {
                    id: { peer: 1n, counter: counter++ },
                    deleted: false,
                    deletedBy: [],
                    value,
                };
                const physicalPosition = position === current.length
                    ? physical.length
                    : physical.indexOf(current[position]);
                physical.splice(physicalPosition, 0, element);
                sequence.insertAtVisible(position, [element]);
            }
            else if (operation === 1) {
                const element = current[nextRandom() % current.length];
                sequence.setDeleted(element, true);
            }
            else {
                const from = nextRandom() % current.length;
                const to = nextRandom() % current.length;
                if (from !== to) {
                    const element = current[from];
                    const physicalPosition = physical.indexOf(element);
                    physical.splice(physicalPosition, 1);
                    const afterRemoval = visible();
                    const destination = to >= afterRemoval.length
                        ? physical.length
                        : physical.indexOf(afterRemoval[to]);
                    physical.splice(destination, 0, element);
                    sequence.moveVisible(from, to);
                }
            }
            if (step % 25 !== 0)
                continue;
            const expectedVisible = visible();
            expect(sequence.all()).toEqual(physical);
            expect(sequence.visible()).toEqual(expectedVisible);
            expect(sequence.allLength).toBe(physical.length);
            expect(sequence.visibleLength).toBe(expectedVisible.length);
            expect(sequence.visibleUtf16Length).toBe(expectedVisible.reduce((length, element) => length + element.value.length, 0));
            expect(sequence.visibleUtf8Length).toBe(new TextEncoder().encode(expectedVisible.map(({ value }) => value).join(""))
                .length);
            for (const [index, element] of physical.entries()) {
                expect(sequence.atPhysical(index)).toBe(element);
                expect(sequence.physicalIndexOf(element)).toBe(index);
                expect(sequence.findById(element.id)).toBe(element);
            }
            for (const [index, element] of expectedVisible.entries()) {
                expect(sequence.atVisible(index)).toBe(element);
                expect(sequence.visibleIndexOf(element)).toBe(index);
            }
            const start = expectedVisible.length >>> 2;
            const end = expectedVisible.length - start;
            expect(sequence.visibleRange(start, end)).toEqual(expectedVisible.slice(start, end));
            expect(sequence.visibleIdRuns(start, end)).toEqual(compressIdRuns(expectedVisible.slice(start, end)));
        }
    });
    test("stops visible range predicates after the first match", () => {
        const sequence = new SequenceIndex();
        sequence.insertAtVisible(0, Array.from({ length: 96 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: counter % 7 === 0,
            deletedBy: [],
            value: String(counter),
        })));
        let visits = 0;
        expect(sequence.someVisibleRange(10, 80, (element) => {
            visits += 1;
            return element.value === "18";
        })).toBe(true);
        expect(visits).toBeLessThan(10);
        visits = 0;
        expect(sequence.someVisibleRange(10, 20, () => {
            visits += 1;
            return false;
        })).toBe(false);
        expect(visits).toBe(10);
    });
    test("visits raw visible storage ranges and locates insertion neighbor ids", () => {
        const sequence = new SequenceIndex();
        const elements = Array.from({ length: 6 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: String(counter),
        }));
        sequence.insertAtVisible(0, elements);
        sequence.setDeleted(elements[1], true);
        sequence.setDeleted(elements[4], true);
        const ranges = [];
        sequence.forEachVisibleStorageRange((_storage, start, end) => {
            ranges.push({ start, end });
        });
        expect(ranges).toEqual([
            { start: 0, end: 1 },
            { start: 2, end: 4 },
            { start: 5, end: 6 },
        ]);
        const context = {
            leftPeer: undefined,
            leftCounter: 0,
            startIndex: 0,
            rightPeer: undefined,
            rightCounter: 0,
        };
        const locate = (position) => ({
            ...sequence.visibleInsertionIdContext(position, context),
        });
        expect(locate(0)).toEqual({
            leftPeer: undefined,
            leftCounter: 0,
            startIndex: 0,
            rightPeer: 1n,
            rightCounter: 0,
        });
        expect(locate(1)).toEqual({
            leftPeer: 1n,
            leftCounter: 0,
            startIndex: 1,
            rightPeer: 1n,
            rightCounter: 1,
        });
        expect(locate(2)).toEqual({
            leftPeer: 1n,
            leftCounter: 2,
            startIndex: 3,
            rightPeer: 1n,
            rightCounter: 3,
        });
        expect(locate(4)).toEqual({
            leftPeer: 1n,
            leftCounter: 5,
            startIndex: 6,
            rightPeer: undefined,
            rightCounter: 0,
        });
        expect(() => sequence.loadValidatedSpans([])).toThrow(/validated sequence spans require an empty index/u);
    });
    test("maps id runs to visible metric ranges without crossing inserts", () => {
        const sequence = new SequenceIndex((element) => ({
            utf16: element.value.length,
            utf8: new TextEncoder().encode(element.value).length,
        }));
        const original = ["a", "🙂", "b", "c", "d"].map((value, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value,
        }));
        sequence.insertAtVisible(0, original);
        sequence.insertAtVisible(2, [
            {
                id: { peer: 2n, counter: 0 },
                deleted: false,
                deletedBy: [],
                value: "X",
            },
        ]);
        sequence.setDeleted(original[3], true);
        expect(sequence.visibleMetricRangesForIdRuns([{ start: { peer: 1n, counter: 0 }, length: 5 }], "utf16")).toEqual([
            { start: 0, end: 3 },
            { start: 4, end: 6 },
        ]);
    });
    test("maintains ordered ranks through insertion and deletion", () => {
        const index = new OrderedIndex((left, right) => left.key - right.key);
        const values = Array.from({ length: 1000 }, (_, key) => ({ key }));
        for (let offset = 0; offset < 10; offset += 1) {
            for (let key = offset; key < values.length; key += 10)
                index.add(values[key]);
        }
        expect(index.values()).toEqual(values);
        for (const [position, value] of values.entries()) {
            expect(index.at(position)).toBe(value);
            expect(index.indexOf(value)).toBe(position);
        }
        for (const value of values.filter(({ key }) => key % 3 === 0))
            index.delete(value);
        const remaining = values.filter(({ key }) => key % 3 !== 0);
        expect(index.values()).toEqual(remaining);
        for (const [position, value] of remaining.entries()) {
            expect(index.indexOf(value)).toBe(position);
        }
    });
    test("indexes text styles by id runs and causal version", () => {
        const styles = new TextStyleIndex();
        const outer = {
            startId: { peer: 7n, counter: 0 },
            lamport: 1,
            info: 0,
            value: true,
        };
        const inner = {
            startId: { peer: 8n, counter: 0 },
            lamport: 2,
            info: 0,
            value: null,
        };
        styles.add([{ start: { peer: 1n, counter: 0 }, length: 100 }], "bold", outer);
        styles.add([{ start: { peer: 1n, counter: 25 }, length: 50 }], "bold", inner);
        expect(styles.metasAt({ peer: 1n, counter: 10 }).get("bold")).toBe(outer);
        expect(styles.metasAt({ peer: 1n, counter: 50 }).get("bold")).toBe(inner);
        expect(styles.metasAt({ peer: 1n, counter: 50 }, new Map([[7n, 1]])).get("bold")).toBe(outer);
        expect(styles.rangeHasKey([{ start: { peer: 1n, counter: 30 }, length: 20 }], "bold")).toBe(false);
        expect(styles.runsContainMeta([{ start: { peer: 1n, counter: 0 }, length: 100 }], "bold", outer.startId)).toBe(true);
        expect(styles.runsContainMeta([{ start: { peer: 1n, counter: 0 }, length: 100 }], "bold", inner.startId)).toBe(false);
        expect(styles.transitions([{ start: { peer: 1n, counter: 0 }, length: 100 }], "bold", new Map([[7n, 1]]), undefined)).toEqual([
            {
                run: { start: { peer: 1n, counter: 25 }, length: 50 },
                before: outer,
                after: inner,
            },
        ]);
    });
    test("indexes forward and reverse deletion runs in both directions", () => {
        const deletions = new SequenceDeletionIndex();
        deletions.add({ peer: 1n, counter: 10 }, 5, { peer: 2n, counter: 100 }, 1);
        deletions.add({ peer: 1n, counter: 12 }, 4, { peer: 3n, counter: 200 }, -1);
        expect(deletions.deletionIdsAt({ peer: 1n, counter: 13 })).toEqual([
            { peer: 2n, counter: 103 },
            { peer: 3n, counter: 202 },
        ]);
        expect(deletions.someDeletion({ peer: 1n, counter: 15 }, (id) => id.counter < 201)).toBe(true);
        expect(deletions.everyDeletion({ peer: 1n, counter: 13 }, (id) => id.counter >= 100)).toBe(true);
        expect(deletions.targetRunsDeletedBy(2n, 101, 104)).toEqual([
            { start: { peer: 1n, counter: 11 }, length: 3 },
        ]);
        expect(deletions.targetRunsDeletedBy(3n, 201, 204)).toEqual([
            { start: { peer: 1n, counter: 12 }, length: 3 },
        ]);
    });
    test("indexes visibility at an earlier causal version", () => {
        const sequence = new SequenceIndex();
        const elements = [
            { id: { peer: 1n, counter: 0 }, deleted: false, deletedBy: [], value: "a" },
            { id: { peer: 2n, counter: 0 }, deleted: false, deletedBy: [], value: "b" },
            { id: { peer: 1n, counter: 1 }, deleted: false, deletedBy: [], value: "c" },
        ];
        sequence.insertAtPhysical(0, elements);
        sequence.setDeleted(elements[0], true);
        sequence.addDeletion(elements[0], { peer: 2n, counter: 1 });
        const beforeConcurrentPeer = sequence.causalView(new Map([
            [1n, 2],
            [2n, 0],
        ]));
        expect(beforeConcurrentPeer.length).toBe(2);
        expect(beforeConcurrentPeer.range(0, 2).map(({ value }) => value)).toEqual([
            "a",
            "c",
        ]);
        expect(beforeConcurrentPeer.at(1)?.value).toBe("c");
        const mutableVersion = new Map([
            [1n, 2],
            [2n, 0],
        ]);
        const immutableView = sequence.causalView(mutableVersion);
        mutableVersion.set(2n, 2);
        expect(immutableView.range(0, immutableView.length).map(({ value }) => value)).toEqual(["a", "c"]);
        expect(sequence.causalView(new Map([
            [1n, 2],
            [2n, 0],
        ]))).toBe(beforeConcurrentPeer);
        const withoutPeerOneTail = sequence.causalView(new Map([
            [1n, 1],
            [2n, 2],
        ]));
        expect(withoutPeerOneTail.range(0, withoutPeerOneTail.length).map(({ value }) => value)).toEqual(["b"]);
        expect(sequence.causalView(new Map([
            [1n, 2],
            [2n, 0],
        ]))).toBe(beforeConcurrentPeer);
        const latest = sequence.causalView(new Map([
            [1n, 2],
            [2n, 2],
        ]));
        expect(latest.range(0, latest.length).map(({ value }) => value)).toEqual(["b", "c"]);
        sequence.moveBefore(elements[2], elements[0]);
        const afterMove = sequence.causalView(new Map([
            [1n, 2],
            [2n, 0],
        ]));
        expect(afterMove).not.toBe(beforeConcurrentPeer);
        expect(afterMove.range(0, afterMove.length).map(({ value }) => value)).toEqual([
            "c",
            "a",
        ]);
    });
    test("packs batch inserts into bounded sequence spans", () => {
        const sequence = new SequenceIndex();
        const elements = Array.from({ length: 10000 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: "x",
        }));
        sequence.insertAtPhysical(0, elements);
        expect(sequence.spanCount).toBeLessThanOrEqual(Math.ceil(elements.length / 32));
        for (let index = 0; index < elements.length; index += 97) {
            sequence.setDeleted(elements[index], true);
        }
        expect(sequence.spanCount).toBeLessThanOrEqual(Math.ceil(elements.length / 32));
        expect(sequence.visibleLength).toBe(elements.filter((element) => !element.deleted).length);
    });
    test("skips deleted subtrees when finding visible neighbors", () => {
        const sequence = new SequenceIndex();
        const elements = Array.from({ length: 10000 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: "x",
        }));
        sequence.insertAtPhysical(0, elements);
        sequence.deleteIdSpan({ peer: 1n, counter: 1 }, elements.length - 2, {
            peer: 2n,
            counter: 0,
        });
        expect(sequence.nextVisible(elements[0])).toBe(elements.at(-1));
        expect(sequence.previousVisible(elements.at(-1))).toBe(elements[0]);
        expect(sequence.nextVisible(elements[elements.length >>> 1])).toBe(elements.at(-1));
        expect(sequence.previousVisible(elements[elements.length >>> 1])).toBe(elements[0]);
    });
    test("materializes lazy span deletions before restoring or restructuring", () => {
        const sequence = new SequenceIndex();
        const elements = Array.from({ length: 10000 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: String(counter),
        }));
        sequence.insertAtPhysical(0, elements);
        sequence.deleteIdSpan({ peer: 1n, counter: 0 }, elements.length, {
            peer: 2n,
            counter: 0,
        });
        sequence.setDeleted(elements[5000], false);
        expect(sequence.visible()).toEqual([elements[5000]]);
        expect(sequence.visibleIndexOf(elements[5000])).toBe(0);
        expect(sequence.atPhysical(4999)?.deleted).toBe(true);
        const inserted = {
            id: { peer: 3n, counter: 0 },
            deleted: false,
            deletedBy: [],
            value: "inserted",
        };
        sequence.insertAtPhysical(5000, [inserted]);
        expect(sequence.visible()).toEqual([inserted, elements[5000]]);
        expect(sequence.all().filter((element) => !element.deleted)).toEqual([
            inserted,
            elements[5000],
        ]);
    });
    test("keeps rank and range queries correct after lazy range restoration", () => {
        const sequence = new SequenceIndex();
        const elements = Array.from({ length: 10000 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: String(counter),
        }));
        sequence.insertAtPhysical(0, elements);
        const all = [{ start: { peer: 1n, counter: 0 }, length: elements.length }];
        sequence.setIdRunsDeleted(all);
        sequence.setIdRunsVisible([{ start: { peer: 1n, counter: 100 }, length: 9800 }]);
        expect(sequence.visibleLength).toBe(9800);
        expect(sequence.atVisible(0)).toBe(elements[100]);
        expect(sequence.atVisible(5000)).toBe(elements[5100]);
        expect(sequence.atVisible(9799)).toBe(elements[9899]);
        expect(sequence.visibleIndexOf(elements[5100])).toBe(5000);
        expect(sequence.metricOffsetAtVisibleIndex(5000, "utf16")).toBe(5000);
        expect(sequence.visibleIndexAtMetricOffset(5000, "utf16")).toBe(5000);
        expect(sequence.visibleIdRuns(123, 9123)).toEqual([
            { start: { peer: 1n, counter: 223 }, length: 9000 },
        ]);
        expect(sequence.visibleRange(4999, 5002)).toEqual([
            elements[5099],
            elements[5100],
            elements[5101],
        ]);
        expect(sequence.previousVisible(elements[100])).toBeUndefined();
        expect(sequence.nextVisible(elements[9899])).toBeUndefined();
    });
    test("deletes id spans while preserving causal delete order", () => {
        const sequence = new SequenceIndex();
        const elements = Array.from({ length: 96 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: String(counter),
        }));
        sequence.insertAtPhysical(0, elements);
        sequence.deleteIdSpan({ peer: 1n, counter: 20 }, 50, {
            peer: 2n,
            counter: 100,
        });
        expect(sequence.visibleLength).toBe(46);
        expect(sequence.visible().map(({ id }) => id.counter)).toEqual([
            ...Array.from({ length: 20 }, (_, counter) => counter),
            ...Array.from({ length: 26 }, (_, counter) => counter + 70),
        ]);
        expect(sequence.elementsDeletedBy(2n, 100, 150).map(({ id }) => id.counter)).toEqual(Array.from({ length: 50 }, (_, counter) => counter + 20));
        const halfway = sequence.causalView(new Map([
            [1n, 96],
            [2n, 125],
        ]));
        expect(halfway.length).toBe(71);
        expect(halfway.range(19, 22).map(({ id }) => id.counter)).toEqual([19, 45, 46]);
        const reversed = new SequenceIndex();
        const reversedElements = Array.from({ length: 8 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: String(counter),
        }));
        reversed.insertAtPhysical(0, reversedElements);
        reversed.deleteIdSpan({ peer: 1n, counter: 2 }, -4, {
            peer: 2n,
            counter: 10,
        });
        const partialReverse = reversed.causalView(new Map([
            [1n, 8],
            [2n, 12],
        ]));
        expect(partialReverse.range(0, partialReverse.length).map(({ id }) => id.counter)).toEqual([0, 1, 2, 3, 6, 7]);
    });
    test("checks scalar and range deletions only on the restored ID runs", () => {
        const sequence = new SequenceIndex();
        const elements = Array.from({ length: 96 }, (_, counter) => ({
            id: { peer: counter === 95 ? 4n : 1n, counter: counter === 95 ? 0 : counter },
            deleted: false,
            deletedBy: [],
            value: String(counter),
        }));
        sequence.insertAtPhysical(0, elements);
        sequence.deleteIdSpan({ peer: 1n, counter: 0 }, 95, {
            peer: 2n,
            counter: 0,
        });
        sequence.deleteElement(elements[95], { peer: 3n, counter: 0 });
        const restored = [{ start: { peer: 1n, counter: 0 }, length: 95 }];
        expect(sequence.canShowIdRunsAt(restored, new Map([
            [1n, 95],
            [2n, 0],
            [3n, 1],
            [4n, 1],
        ]))).toBe(true);
        sequence.addDeletion(elements[0], { peer: 3n, counter: 1 });
        expect(sequence.canShowIdRunsAt(restored, new Map([
            [1n, 95],
            [2n, 0],
            [3n, 2],
            [4n, 1],
        ]))).toBe(false);
    });
    test("coalesces adjacent single-element inserts into bounded spans", () => {
        const sequence = new SequenceIndex();
        for (let counter = 0; counter < 10000; counter += 1) {
            sequence.insertAtPhysical(sequence.allLength, [
                {
                    id: { peer: 1n, counter },
                    deleted: false,
                    deletedBy: [],
                    value: String(counter),
                },
            ]);
        }
        expect(sequence.allLength).toBe(10000);
        expect(sequence.spanCount).toBe(Math.ceil(10000 / 32));
        for (const counter of [0, 31, 32, 4999, 9999]) {
            const element = sequence.findById({ peer: 1n, counter });
            expect(element?.value).toBe(String(counter));
            expect(sequence.physicalIndexOf(element)).toBe(counter);
        }
    });
    test("ranges across an excluded element", () => {
        const sequence = new SequenceIndex();
        const base = Array.from({ length: 6 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: String.fromCharCode(97 + counter),
        }));
        sequence.insertAtPhysical(0, base);
        sequence.recordOperationId({ peer: 1n, counter: 6 });
        sequence.recordOperationId({ peer: 1n, counter: 7 });
        sequence.recordOperationId({ peer: 1n, counter: 8 });
        sequence.recordOperationId({ peer: 1n, counter: 9 });
        sequence.insertAtPhysical(3, [
            {
                id: { peer: 1n, counter: 10 },
                deleted: false,
                deletedBy: [],
                value: "X",
            },
        ]);
        sequence.recordOperationId({ peer: 1n, counter: 11 });
        sequence.recordOperationId({ peer: 1n, counter: 12 });
        const view = sequence.causalView(new Map([[1n, 8]]));
        expect(view.length).toBe(6);
        expect(view.range(2, 4).map(({ value }) => value)).toEqual(["c", "d"]);
        expect(view.idRuns(0, view.length)).toEqual([
            { start: { peer: 1n, counter: 0 }, length: 6 },
        ]);
    });
    test("seeks sparse operation counters without walking numeric gaps", () => {
        const sequence = new SequenceIndex();
        const first = {
            id: { peer: 1n, counter: 0 },
            deleted: false,
            deletedBy: [],
            value: "a",
        };
        const distant = {
            id: { peer: 1n, counter: 1000000000 },
            deleted: false,
            deletedBy: [],
            value: "b",
        };
        sequence.insertAtPhysical(0, [first, distant]);
        sequence.setDeleted(first, true);
        sequence.addDeletion(first, { peer: 2n, counter: 1000000000 });
        const early = sequence.causalView(new Map([
            [1n, 1],
            [2n, 0],
        ]));
        expect(early.range(0, early.length).map(({ value }) => value)).toEqual(["a"]);
        expect(sequence.elementsDeletedBy(2n, 999999999, 1000000001)).toEqual([first]);
    });
    test("promotes a deletion counter to a set only for one-to-many deletes", () => {
        const sequence = new SequenceIndex();
        const elements = Array.from({ length: 3 }, (_, counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: String(counter),
        }));
        sequence.insertAtPhysical(0, elements);
        for (const element of elements.slice(0, 2)) {
            sequence.setDeleted(element, true);
            sequence.addDeletion(element, { peer: 2n, counter: 0 });
        }
        sequence.addDeletion(elements[0], { peer: 3n, counter: 0 });
        expect(sequence.elementsDeletedBy(2n, 0, 1)).toEqual(elements.slice(0, 2));
        const beforeDelete = sequence.causalView(new Map([
            [1n, 3],
            [2n, 0],
            [3n, 0],
        ]));
        expect(beforeDelete.range(0, beforeDelete.length)).toEqual(elements);
        const afterSecondDelete = sequence.causalView(new Map([
            [1n, 3],
            [2n, 0],
            [3n, 1],
        ]));
        expect(afterSecondDelete.range(0, afterSecondDelete.length)).toEqual(elements.slice(1));
    });
    test("merges out-of-order counter runs", () => {
        const sequence = new SequenceIndex();
        const counters = [10, 0, 2, 1, 1000000000, 11];
        const elements = counters.map((counter) => ({
            id: { peer: 1n, counter },
            deleted: false,
            deletedBy: [],
            value: String(counter),
        }));
        sequence.insertAtPhysical(0, elements);
        const early = sequence.causalView(new Map([[1n, 2]]));
        expect(early.range(0, early.length).map(({ id }) => id.counter)).toEqual([0, 1]);
        for (let index = 0; index < elements.length; index += 1) {
            sequence.setDeleted(elements[index], true);
            sequence.addDeletion(elements[index], {
                peer: 2n,
                counter: counters[index],
            });
        }
        expect(sequence.elementsDeletedBy(2n, 1, 11).map(({ id }) => id.counter)).toEqual([
            10, 2, 1,
        ]);
    });
});
function compressIdRuns(elements) {
    const runs = [];
    for (const { id } of elements) {
        const previous = runs.at(-1);
        if (previous !== undefined &&
            previous.start.peer === id.peer &&
            previous.start.counter + previous.length === id.counter) {
            previous.length += 1;
        }
        else {
            runs.push({ start: { ...id }, length: 1 });
        }
    }
    return runs;
}
