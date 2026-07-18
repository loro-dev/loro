import { OrderedIndex } from "./ordered-index";
import type { SequenceId, SequenceIdRun } from "./sequence-index";

interface DeletionMapping {
  readonly targetPeer: bigint;
  readonly targetStart: number;
  readonly deletePeer: bigint;
  readonly deleteStart: number;
  readonly length: number;
  readonly direction: 1 | -1;
}

interface TargetSegment {
  start: number;
  end: number;
  readonly mappings: DeletionMapping[];
}

interface OperationRun extends DeletionMapping {
  readonly serial: number;
}

export class SequenceDeletionIndex {
  readonly #targetsByPeer = new Map<bigint, OrderedIndex<TargetSegment>>();
  readonly #operationsByPeer = new Map<bigint, OrderedIndex<OperationRun>>();
  #serial = 0;

  add(
    targetStart: SequenceId,
    length: number,
    deleteStart: SequenceId,
    direction: 1 | -1,
  ): void {
    if (length <= 0) return;
    const mapping: DeletionMapping = {
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
    const existing: TargetSegment[] = [];
    targets.forEachFrom(
      { start: targetStart.counter, end: targetStart.counter, mappings: [] },
      (segment) => {
        if (segment.start >= targetEnd) return false;
        existing.push(segment);
      },
    );
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

    this.#operations(deleteStart.peer).add({ ...mapping, serial: this.#serial++ });
  }

  deletionIdsAt(target: SequenceId): SequenceId[] {
    const segment = this.#targetSegmentAt(target);
    if (segment === undefined) return [];
    return segment.mappings.map((mapping) => ({
      peer: mapping.deletePeer,
      counter: deleteCounterAt(mapping, target.counter),
    }));
  }

  someDeletion(target: SequenceId, predicate: (id: SequenceId) => boolean): boolean {
    const segment = this.#targetSegmentAt(target);
    return (
      segment?.mappings.some((mapping) =>
        predicate({
          peer: mapping.deletePeer,
          counter: deleteCounterAt(mapping, target.counter),
        }),
      ) ?? false
    );
  }

  everyDeletion(target: SequenceId, predicate: (id: SequenceId) => boolean): boolean {
    const segment = this.#targetSegmentAt(target);
    return (
      segment?.mappings.every((mapping) =>
        predicate({
          peer: mapping.deletePeer,
          counter: deleteCounterAt(mapping, target.counter),
        }),
      ) ?? true
    );
  }

  targetRunsDeletedBy(peer: bigint, start: number, end: number): SequenceIdRun[] {
    if (start >= end) return [];
    const operations = this.#operationsByPeer.get(peer);
    if (operations === undefined) return [];
    const selected: SequenceIdRun[] = [];
    const first = Math.max(
      0,
      operations._lowerBoundBy((operation) => operation.deleteStart - start) - 1,
    );
    for (let index = first; index < operations.size; index += 1) {
      const operation = operations.at(index)!;
      if (operation.deleteStart >= end) break;
      const overlapStart = Math.max(start, operation.deleteStart);
      const overlapEnd = Math.min(end, operation.deleteStart + operation.length);
      if (overlapStart >= overlapEnd) continue;
      const targetCounter =
        operation.direction === 1
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

  hasDeletionAt(
    runs: readonly SequenceIdRun[],
    version: { get(peer: bigint): number | undefined },
  ): boolean {
    for (const run of runs) {
      const targets = this.#targetsByPeer.get(run.start.peer);
      if (targets === undefined) continue;
      const start = run.start.counter;
      const end = start + run.length;
      let index = Math.max(
        0,
        targets._lowerBoundBy((segment) => segment.start - (start + 1)) - 1,
      );
      for (; index < targets.size; index += 1) {
        const segment = targets.at(index)!;
        if (segment.start >= end) break;
        const overlapStart = Math.max(start, segment.start);
        const overlapEnd = Math.min(end, segment.end);
        if (overlapStart >= overlapEnd) continue;
        for (const mapping of segment.mappings) {
          const minimumCounter =
            mapping.direction === 1
              ? deleteCounterAt(mapping, overlapStart)
              : deleteCounterAt(mapping, overlapEnd - 1);
          if (minimumCounter < (version.get(mapping.deletePeer) ?? 0)) return true;
        }
      }
    }
    return false;
  }

  reset(): void {
    this.#targetsByPeer.clear();
    this.#operationsByPeer.clear();
    this.#serial = 0;
  }

  #targets(peer: bigint): OrderedIndex<TargetSegment> {
    let targets = this.#targetsByPeer.get(peer);
    if (targets === undefined) {
      targets = new OrderedIndex((left, right) => left.start - right.start);
      this.#targetsByPeer.set(peer, targets);
    }
    return targets;
  }

  #operations(peer: bigint): OrderedIndex<OperationRun> {
    let operations = this.#operationsByPeer.get(peer);
    if (operations === undefined) {
      operations = new OrderedIndex(
        (left, right) =>
          left.deleteStart - right.deleteStart || left.serial - right.serial,
      );
      this.#operationsByPeer.set(peer, operations);
    }
    return operations;
  }

  #targetSegmentAt(target: SequenceId): TargetSegment | undefined {
    const targets = this.#targetsByPeer.get(target.peer);
    if (targets === undefined) return undefined;
    const index =
      targets._lowerBoundBy((segment) => segment.start - (target.counter + 1)) - 1;
    const segment = targets.at(index);
    return segment !== undefined && target.counter < segment.end ? segment : undefined;
  }

  #ensureBoundary(targets: OrderedIndex<TargetSegment>, counter: number): void {
    const index = targets._lowerBoundBy((segment) => segment.start - counter);
    if (targets.at(index)?.start === counter) return;
    const previous = targets.at(index - 1);
    if (previous === undefined || counter >= previous.end) return;
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

function addMapping(mappings: DeletionMapping[], mapping: DeletionMapping): void {
  if (
    mappings.some(
      (current) =>
        current.deletePeer === mapping.deletePeer &&
        current.deleteStart === mapping.deleteStart &&
        current.targetPeer === mapping.targetPeer &&
        current.targetStart === mapping.targetStart,
    )
  ) {
    return;
  }
  mappings.push(mapping);
}

function deleteCounterAt(mapping: DeletionMapping, targetCounter: number): number {
  const offset = targetCounter - mapping.targetStart;
  return (
    mapping.deleteStart + (mapping.direction === 1 ? offset : mapping.length - 1 - offset)
  );
}

function normalizeRuns(runs: readonly SequenceIdRun[]): SequenceIdRun[] {
  const sorted = [...runs].sort((left, right) =>
    left.start.peer < right.start.peer
      ? -1
      : left.start.peer > right.start.peer
        ? 1
        : left.start.counter - right.start.counter,
  );
  const merged: { start: SequenceId; length: number }[] = [];
  for (const run of sorted) {
    const previous = merged.at(-1);
    if (
      previous !== undefined &&
      previous.start.peer === run.start.peer &&
      run.start.counter <= previous.start.counter + previous.length
    ) {
      previous.length =
        Math.max(
          previous.start.counter + previous.length,
          run.start.counter + run.length,
        ) - previous.start.counter;
    } else {
      merged.push({ start: { ...run.start }, length: run.length });
    }
  }
  return merged;
}
