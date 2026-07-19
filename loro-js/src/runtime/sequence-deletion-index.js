import { OrderedIndex } from "./ordered-index";
// Reused deletion id passed to predicates so visited mappings do not each
// allocate a short-lived id object. Predicates must not retain the id
// argument or query deletions reentrantly.
const scratchDeletionId = { peer: 0n, counter: 0 };
// Reused OrderedIndex.forEachFrom probe in `add`; only `.start` is read.
const targetSegmentProbe = { start: 0, end: 0, mappings: [] };
export class SequenceDeletionIndex {
    #targetsByPeer = new Map();
    #operationsByPeer = new Map();
    #serial = 0;
    add(targetStart, length, deleteStart, direction) {
        if (length <= 0)
            return;
        const mapping = {
            targetPeer: targetStart.peer,
            targetStart: targetStart.counter,
            deletePeer: deleteStart.peer,
            deleteStart: deleteStart.counter,
            length,
            direction,
        };
        const targets = this.#targets(targetStart.peer);
        const targetEnd = targetStart.counter + length;
        this.#ensureBoundary(targets, targetStart.counter);
        this.#ensureBoundary(targets, targetEnd);
        const existing = [];
        // OrderedIndex.forEachFrom only reads `.start` through the comparator
        // and never retains the probe, so a shared scratch object is safe.
        targetSegmentProbe.start = targetStart.counter;
        targets.forEachFrom(targetSegmentProbe, (segment) => {
            if (segment.start >= targetEnd)
                return false;
            existing.push(segment);
        });
        let cursor = targetStart.counter;
        for (const segment of existing) {
            if (cursor < segment.start) {
                targets.add({ start: cursor, end: segment.start, mappings: [mapping] });
            }
            addMapping(segment.mappings, mapping);
            cursor = segment.end;
        }
        if (cursor < targetEnd) {
            targets.add({ start: cursor, end: targetEnd, mappings: [mapping] });
        }
        this.#operations(deleteStart.peer).add({
            targetPeer: mapping.targetPeer,
            targetStart: mapping.targetStart,
            deletePeer: mapping.deletePeer,
            deleteStart: mapping.deleteStart,
            length: mapping.length,
            direction: mapping.direction,
            serial: this.#serial++,
        });
    }
    deletionIdsAt(target) {
        const segment = this.#targetSegmentAt(target);
        if (segment === undefined)
            return [];
        return segment.mappings.map((mapping) => ({
            peer: mapping.deletePeer,
            counter: deleteCounterAt(mapping, target.counter),
        }));
    }
    someDeletion(target, predicate) {
        const mappings = this.#targetSegmentAt(target)?.mappings;
        if (mappings === undefined)
            return false;
        for (let index = 0; index < mappings.length; index += 1) {
            const mapping = mappings[index];
            scratchDeletionId.peer = mapping.deletePeer;
            scratchDeletionId.counter = deleteCounterAt(mapping, target.counter);
            if (predicate(scratchDeletionId))
                return true;
        }
        return false;
    }
    everyDeletion(target, predicate) {
        const mappings = this.#targetSegmentAt(target)?.mappings;
        if (mappings === undefined)
            return true;
        for (let index = 0; index < mappings.length; index += 1) {
            const mapping = mappings[index];
            scratchDeletionId.peer = mapping.deletePeer;
            scratchDeletionId.counter = deleteCounterAt(mapping, target.counter);
            if (!predicate(scratchDeletionId))
                return false;
        }
        return true;
    }
    targetRunsDeletedBy(peer, start, end) {
        if (start >= end)
            return [];
        const operations = this.#operationsByPeer.get(peer);
        if (operations === undefined)
            return [];
        const selected = [];
        const first = Math.max(0, operations._lowerBoundBy((operation) => operation.deleteStart - start) - 1);
        for (let index = first; index < operations.size; index += 1) {
            const operation = operations.at(index);
            if (operation.deleteStart >= end)
                break;
            const overlapStart = Math.max(start, operation.deleteStart);
            const overlapEnd = Math.min(end, operation.deleteStart + operation.length);
            if (overlapStart >= overlapEnd)
                continue;
            const targetCounter = operation.direction === 1
                ? operation.targetStart + overlapStart - operation.deleteStart
                : operation.targetStart +
                    operation.length -
                    (overlapEnd - operation.deleteStart);
            selected.push({
                start: { peer: operation.targetPeer, counter: targetCounter },
                length: overlapEnd - overlapStart,
            });
        }
        return normalizeRuns(selected);
    }
    hasDeletionAt(runs, version) {
        for (const run of runs) {
            const targets = this.#targetsByPeer.get(run.start.peer);
            if (targets === undefined)
                continue;
            const start = run.start.counter;
            const end = start + run.length;
            let index = Math.max(0, targets._lowerBoundBy((segment) => segment.start - (start + 1)) - 1);
            for (; index < targets.size; index += 1) {
                const segment = targets.at(index);
                if (segment.start >= end)
                    break;
                const overlapStart = Math.max(start, segment.start);
                const overlapEnd = Math.min(end, segment.end);
                if (overlapStart >= overlapEnd)
                    continue;
                for (const mapping of segment.mappings) {
                    const minimumCounter = mapping.direction === 1
                        ? deleteCounterAt(mapping, overlapStart)
                        : deleteCounterAt(mapping, overlapEnd - 1);
                    if (minimumCounter < (version.get(mapping.deletePeer) ?? 0))
                        return true;
                }
            }
        }
        return false;
    }
    reset() {
        this.#targetsByPeer.clear();
        this.#operationsByPeer.clear();
        this.#serial = 0;
    }
    #targets(peer) {
        let targets = this.#targetsByPeer.get(peer);
        if (targets === undefined) {
            targets = new OrderedIndex((left, right) => left.start - right.start);
            this.#targetsByPeer.set(peer, targets);
        }
        return targets;
    }
    #operations(peer) {
        let operations = this.#operationsByPeer.get(peer);
        if (operations === undefined) {
            operations = new OrderedIndex((left, right) => left.deleteStart - right.deleteStart || left.serial - right.serial);
            this.#operationsByPeer.set(peer, operations);
        }
        return operations;
    }
    #targetSegmentAt(target) {
        const targets = this.#targetsByPeer.get(target.peer);
        if (targets === undefined)
            return undefined;
        const index = targets._lowerBoundBy((segment) => segment.start - (target.counter + 1)) - 1;
        const segment = targets.at(index);
        return segment !== undefined && target.counter < segment.end ? segment : undefined;
    }
    #ensureBoundary(targets, counter) {
        const index = targets._lowerBoundBy((segment) => segment.start - counter);
        if (targets.at(index)?.start === counter)
            return;
        const previous = targets.at(index - 1);
        if (previous === undefined || counter >= previous.end)
            return;
        targets.delete(previous);
        const right = {
            start: counter,
            end: previous.end,
            mappings: [...previous.mappings],
        };
        previous.end = counter;
        targets.add(previous);
        targets.add(right);
    }
}
function addMapping(mappings, mapping) {
    if (mappings.some((current) => current.deletePeer === mapping.deletePeer &&
        current.deleteStart === mapping.deleteStart &&
        current.targetPeer === mapping.targetPeer &&
        current.targetStart === mapping.targetStart)) {
        return;
    }
    mappings.push(mapping);
}
function deleteCounterAt(mapping, targetCounter) {
    const offset = targetCounter - mapping.targetStart;
    return (mapping.deleteStart + (mapping.direction === 1 ? offset : mapping.length - 1 - offset));
}
function normalizeRuns(runs) {
    const sorted = [...runs].sort((left, right) => left.start.peer < right.start.peer
        ? -1
        : left.start.peer > right.start.peer
            ? 1
            : left.start.counter - right.start.counter);
    const merged = [];
    for (const run of sorted) {
        const previous = merged.at(-1);
        if (previous !== undefined &&
            previous.start.peer === run.start.peer &&
            run.start.counter <= previous.start.counter + previous.length) {
            previous.length =
                Math.max(previous.start.counter + previous.length, run.start.counter + run.length) - previous.start.counter;
        }
        else {
            merged.push({ start: { ...run.start }, length: run.length });
        }
    }
    return merged;
}
