import type { DecodedOperationContent } from "../codec/change-block-codec";
import type { Id } from "../codec/types";
import type { LastWriter, TreeNodeRecord } from "./containers";
import { formatTreeId } from "./ids";
import { OrderedIndex } from "./ordered-index";

export type TreeMove = Extract<
  DecodedOperationContent,
  { type: "tree-create" | "tree-move" | "tree-delete" }
>;
type Node = Omit<TreeNodeRecord, "data">;
type Version = { get(peer: bigint): number | undefined };

interface Entry {
  readonly content: TreeMove;
  readonly id: Id;
  readonly writer: LastWriter;
  active: boolean;
  previous: Node | undefined;
}

/**
 * Rust's tree semantics: apply moves in (lamport, peer) order, retaining
 * cycle-closing moves in history without changing the node's placement.
 * An older import or checkout rewinds and replays the affected suffix, since
 * its moves' cycle checks depend on every earlier move, including other nodes.
 */
export class TreeMoves {
  readonly nodes = new Map<string, Node>();
  readonly #entries = new Map<string, Entry>();
  readonly #order = new OrderedIndex<Entry>(compareEntries);
  readonly #peers = new Map<bigint, OrderedIndex<Entry>>();
  readonly #version = new Map<bigint, number>();
  #lastActive: Entry | undefined;
  #query: { version: Version; nodes: Map<string, Node | undefined> } | undefined;

  constructor(seed: Iterable<Node>) {
    for (const node of seed) this.nodes.set(formatTreeId(node.id), { ...node });
  }

  register(content: TreeMove, id: Id, writer: LastWriter): Entry {
    const key = formatTreeId(id);
    const known = this.#entries.get(key);
    if (known !== undefined) return known;
    const entry: Entry = { content, id, writer, active: false, previous: undefined };
    this.#entries.set(key, entry);
    this.#order.add(entry);
    let peer = this.#peers.get(id.peer);
    if (peer === undefined) {
      peer = new OrderedIndex((a, b) => a.id.counter - b.id.counter);
      this.#peers.set(id.peer, peer);
    }
    peer.add(entry);
    this.#query = undefined;
    return entry;
  }

  apply(content: TreeMove, id: Id, writer: LastWriter): Set<string> {
    const entry = this.register(content, id, writer);
    if (entry.active) return new Set();
    this.#version.set(id.peer, Math.max(this.#version.get(id.peer) ?? 0, id.counter + 1));
    this.#query = undefined;
    if (this.#lastActive === undefined || compareEntries(this.#lastActive, entry) < 0) {
      this.#applyEntry(entry);
      this.#lastActive = entry;
      return new Set([formatTreeId(content.subject)]);
    }
    return this.#replay(entry, this.#version);
  }

  beforeMove(id: Id): Node | undefined {
    return this.#entries.get(formatTreeId(id))?.previous;
  }

  transition(version: Version): Set<string> {
    this.#query = undefined;
    const first = this.#firstDifference(version);
    const changed =
      first === undefined ? new Set<string>() : this.#replay(first, version);
    for (const peer of this.#peers.keys())
      this.#version.set(peer, version.get(peer) ?? 0);
    return changed;
  }

  /**
   * Resolve one historical version once per diff/event. Keep only the changed
   * placements as an overlay; copying a 100k-node tree for a single move would
   * make diffs linear in the total tree size.
   */
  placementAt(id: Id, version: Version): Node | undefined {
    if (this.#query?.version !== version) {
      const nodes = new Map<string, Node | undefined>();
      const first = this.#firstDifference(version);
      if (first !== undefined) {
        const changed = this.#replay(first, version);
        for (const key of changed) nodes.set(key, this.nodes.get(key));
        this.#replay(first, this.#version);
      }
      this.#query = { version, nodes };
    }
    const key = formatTreeId(id);
    return this.#query.nodes.has(key) ? this.#query.nodes.get(key) : this.nodes.get(key);
  }

  #firstDifference(version: Version): Entry | undefined {
    let first: Entry | undefined;
    for (const [peerId, peer] of this.#peers) {
      const current = this.#version.get(peerId) ?? 0;
      const target = version.get(peerId) ?? 0;
      if (current === target) continue;
      const start = peer._lowerBoundBy(
        (entry) => entry.id.counter - Math.min(current, target),
      );
      const end = peer._lowerBoundBy(
        (entry) => entry.id.counter - Math.max(current, target),
      );
      for (const entry of peer.valuesRange(start, end)) {
        if (first === undefined || compareEntries(entry, first) < 0) first = entry;
      }
    }
    return first;
  }

  #replay(first: Entry, version: Version): Set<string> {
    const suffix = this.#order.valuesRange(
      this.#order.lowerBound(first),
      this.#order.size,
    );
    const changed = new Set<string>();
    for (let i = suffix.length - 1; i >= 0; i -= 1) {
      const entry = suffix[i]!;
      if (!entry.active) continue;
      const key = formatTreeId(entry.content.subject);
      changed.add(key);
      if (entry.previous === undefined) this.nodes.delete(key);
      else this.nodes.set(key, entry.previous);
    }
    for (const entry of suffix) {
      entry.active = entry.id.counter < (version.get(entry.id.peer) ?? 0);
      if (!entry.active) continue;
      changed.add(formatTreeId(entry.content.subject));
      this.#applyEntry(entry);
    }
    this.#lastActive = undefined;
    for (let i = this.#order.size - 1; i >= 0; i -= 1) {
      const entry = this.#order.at(i)!;
      if (entry.active) {
        this.#lastActive = entry;
        break;
      }
    }
    return changed;
  }

  #applyEntry(entry: Entry): void {
    entry.active = true;
    const { content, id, writer } = entry;
    const key = formatTreeId(content.subject);
    const previous = this.nodes.get(key);
    entry.previous = previous;
    if (content.type === "tree-delete") {
      if (previous !== undefined) {
        this.nodes.set(key, { ...previous, deleted: true, writer, lastMoveId: id });
      }
    } else if (!this.#wouldCycle(content.subject, content.parent)) {
      this.nodes.set(key, {
        id: content.subject,
        parent: content.parent,
        position: content.position,
        deleted: false,
        writer,
        lastMoveId: id,
      });
    }
  }

  #wouldCycle(subject: Id, parent: Id | undefined): boolean {
    const key = formatTreeId(subject);
    const existing = this.nodes.get(key);
    if (parent !== undefined && formatTreeId(parent) === key) return true;
    // A new node has no descendants; a same-parent reorder changes no links.
    if (
      existing === undefined ||
      (!existing.deleted &&
        (existing.parent === undefined
          ? parent === undefined
          : parent !== undefined &&
            formatTreeId(existing.parent) === formatTreeId(parent)))
    )
      return false;
    for (let steps = 0; parent !== undefined; steps += 1) {
      if (formatTreeId(parent) === key) return true;
      // Fail fast if an internal invariant is broken; never hide the tree.
      if (steps > this.nodes.size) throw new Error("cycle in resolved tree parents");
      const node = this.nodes.get(formatTreeId(parent));
      parent = node === undefined || node.deleted ? undefined : node.parent;
    }
    return false;
  }
}

function compareEntries(a: Entry, b: Entry): number {
  return (
    a.writer.lamport - b.writer.lamport ||
    (a.writer.peer < b.writer.peer ? -1 : a.writer.peer > b.writer.peer ? 1 : 0)
  );
}
