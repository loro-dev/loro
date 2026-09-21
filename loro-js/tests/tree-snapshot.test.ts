import { readFileSync } from "node:fs";

import { describe, expect, test } from "vitest";

import {
  ContainerType,
  compareBytes,
  decodeFastSnapshot,
  decodeStateSnapshotStore,
  type TreeStateSnapshot,
} from "../src/codec/index";
import { LoroDoc } from "../src/index";

const fixture = (name: string): Uint8Array =>
  new Uint8Array(readFileSync(new URL(`./fixtures/rust/${name}`, import.meta.url)));

function treeStates(stateBytes: Uint8Array): TreeStateSnapshot[] {
  const store = decodeStateSnapshotStore(stateBytes);
  if (store.kind !== "sstable") return [];
  return store.containers
    .map(({ wrapper }) => wrapper.state)
    .filter((state): state is TreeStateSnapshot => state.kind === ContainerType.Tree);
}

/**
 * Returns the nodes that break Rust's `TreeState::decode_snapshot_fast`
 * expectations: each parent's children in strictly increasing
 * (fractional index, lamport, peer) order, and deleted nodes at the default index.
 */
function rustOrderViolations(state: TreeStateSnapshot): number[] {
  const violations: number[] = [];
  const lastChild = new Map<
    bigint,
    { position: Uint8Array; lamport: number; peer: bigint }
  >();
  state.nodes.forEach((node, index) => {
    const key = {
      position: state.positions[node.fractionalIndexIndex]!,
      lamport: node.lastSetCounter + node.lastSetLamportSub,
      peer: state.peers[Number(node.lastSetPeerIndex)]!,
    };
    const previous = lastChild.get(node.parentIndexPlusTwo);
    const order =
      previous === undefined
        ? -1
        : compareBytes(previous.position, key.position) ||
          previous.lamport - key.lamport ||
          (previous.peer < key.peer ? -1 : previous.peer > key.peer ? 1 : 0);
    const deletedAtDefault =
      node.parentIndexPlusTwo !== 1n ||
      (key.position.length === 1 && key.position[0] === 0x80);
    if (order >= 0 || !deletedAtDefault) violations.push(index);
    lastChild.set(node.parentIndexPlusTwo, key);
  });
  return violations;
}

function lastMoveIds(doc: LoroDoc): Map<string, { peer: string; counter: number }> {
  return new Map(
    doc
      .getTree("tree")
      .getNodes({ withDeleted: true })
      .map((node) => [node.id, node.getLastMoveId()] as const),
  );
}

function buildMovedTree(): {
  doc: LoroDoc;
  shallowRoot: ReturnType<LoroDoc["frontiers"]>;
} {
  const left = new LoroDoc();
  left.setPeerId(5);
  const tree = left.getTree("tree");
  const root = tree.createNode();
  root.data.set("title", "root");
  const first = root.createNode();
  root.createNode();
  const third = root.createNode();
  // loro-dev/loro#1088: the moved node sorts before its older siblings.
  root.createNode().move(root, 0);
  const nested = first.createNode();
  nested.createNode();
  left.commit();
  const base = left.oplogVersion();

  const right = new LoroDoc();
  right.setPeerId(6);
  right.import(left.export({ mode: "update" }));
  right.getTree("tree").move(third.id, root.id, 0);
  right.getTree("tree").delete(first.id);
  right.getTree("tree").createNode(root.id, 1);
  right.commit();
  tree.move(third.id, nested.id, 0);
  tree.createNode(undefined, 0);
  left.commit();
  left.import(right.export({ mode: "update", from: base }));
  const shallowRoot = left.frontiers();
  tree.move(third.id, root.id, 2);
  left.commit();
  return { doc: left, shallowRoot };
}

describe("tree state snapshots", () => {
  test("re-encode Rust tree state field-for-field", () => {
    const rust = fixture("snapshot.blob");
    const doc = new LoroDoc();
    doc.import(rust);
    const expected = treeStates(decodeFastSnapshot(rust).state);
    expect(expected.length).toBeGreaterThan(0);
    expect(
      treeStates(decodeFastSnapshot(doc.export({ mode: "snapshot" })).state),
    ).toEqual(expected);
  });

  test("write moved and deleted nodes in the order Rust decodes", () => {
    const { doc, shallowRoot } = buildMovedTree();
    const snapshot = doc.export({ mode: "snapshot" });
    const shallow = doc.export({ mode: "shallow-snapshot", frontiers: shallowRoot });

    const decodedSnapshot = decodeFastSnapshot(snapshot);
    const decodedShallow = decodeFastSnapshot(shallow);
    const states = [
      ...treeStates(decodedSnapshot.state),
      ...treeStates(decodedShallow.state),
      ...treeStates(decodedShallow.shallowRootState),
    ];
    expect(states).toHaveLength(3);
    for (const state of states) expect(rustOrderViolations(state)).toEqual([]);

    const [state] = treeStates(decodedSnapshot.state);
    expect(state!.nodes.some((node) => node.parentIndexPlusTwo === 1n)).toBe(true);
    const moveIds = lastMoveIds(doc);
    for (const node of state!.nodes) {
      const id = `${node.counter}@${state!.peers[Number(node.peerIndex)]}`;
      expect({
        peer: state!.peers[Number(node.lastSetPeerIndex)]!.toString(),
        counter: node.lastSetCounter,
      }).toEqual(moveIds.get(id));
    }

    for (const bytes of [snapshot, shallow]) {
      const imported = new LoroDoc();
      imported.import(bytes);
      expect(imported.toJSON()).toEqual(doc.toJSON());
      expect(lastMoveIds(imported)).toEqual(moveIds);
    }
  });

  test("keep the committed Rust interop fixture in sync", () => {
    const { doc, shallowRoot } = buildMovedTree();
    const fromFixture = new LoroDoc();
    fromFixture.import(fixture("tree-move-updates.ts.blob"));
    expect(fromFixture.toJSON()).toEqual(doc.toJSON());
    expect(
      treeStates(decodeFastSnapshot(fixture("tree-move-snapshot.ts.blob")).state),
    ).toEqual(treeStates(decodeFastSnapshot(doc.export({ mode: "snapshot" })).state));
    expect(
      treeStates(
        decodeFastSnapshot(fixture("tree-move-shallow.ts.blob")).shallowRootState,
      ),
    ).toEqual(
      treeStates(
        decodeFastSnapshot(
          doc.export({ mode: "shallow-snapshot", frontiers: shallowRoot }),
        ).shallowRootState,
      ),
    );
  });
});
