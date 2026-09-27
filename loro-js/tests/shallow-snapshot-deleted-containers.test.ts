import { readFileSync } from "node:fs";

import { describe, expect, test } from "vitest";

import { decodeFastSnapshot, decodeStateSnapshotStore } from "../src/codec/index";
import {
  LoroDoc,
  LoroMap,
  LoroText,
  type ContainerID,
  type Frontiers,
  type TreeID,
} from "../src/index";
import { formatContainerId } from "../src/runtime/ids";

/**
 * Port of crates/loro/tests/shallow_snapshot_deleted_containers.rs
 * (loro-dev/loro#1119 and #1123). Containers deleted before a shallow root
 * are dropped from both exported state sections unless a retained op can
 * revive them. Tree node metadata is the exception, because a retained move
 * revives a deleted node with its old metadata.
 */
const RETAINED_FILLER = [0, 300];

const fixture = (name: string): Uint8Array =>
  new Uint8Array(readFileSync(new URL(`./fixtures/rust/${name}`, import.meta.url)));

const importBytes = (bytes: Uint8Array): LoroDoc => {
  const doc = new LoroDoc();
  doc.import(bytes);
  return doc;
};

const shallowRoundTrip = (doc: LoroDoc, root: Frontiers): LoroDoc =>
  importBytes(doc.export({ mode: "shallow-snapshot", frontiers: root }));

/** Container IDs stored in the latest-state and shallow-root sections. */
function storedContainers(bytes: Uint8Array): Set<ContainerID> {
  const snapshot = decodeFastSnapshot(bytes);
  const ids = new Set<ContainerID>();
  for (const section of [snapshot.state, snapshot.shallowRootState]) {
    const store = decodeStateSnapshotStore(section);
    if (store.kind !== "sstable") continue;
    for (const { id } of store.containers) ids.add(formatContainerId(id));
  }
  return ids;
}

function filler(doc: LoroDoc, count: number): void {
  if (count === 0) return;
  const other = doc.getMap("other");
  for (let index = 0; index < count; index += 1) {
    other.set(`k${index}`, index);
    doc.commit();
  }
}

function fillMeta(doc: LoroDoc, node: TreeID, title: string): void {
  const meta = doc.getTree("tree").getNodeByID(node)!.data;
  meta.set("title", title);
  meta.setContainer("body", new LoroText()).insert(0, `${title} body`);
  meta.setContainer("props", new LoroMap()).set("owner", title);
}

/** Imports a peer-2 move of `target` (created by peer 1) under the tree root. */
function importRemoteMoveToRoot(doc: LoroDoc, target: TreeID): void {
  const [head] = doc.frontiers();
  const change = doc.getChangeAt(head!);
  const [targetCounter, targetPeer] = target.split("@");
  expect(targetPeer).toBe("1");
  doc.importJsonUpdates({
    schema_version: 1,
    start_version: {},
    peers: ["1", "2"],
    changes: [
      {
        id: "0@1",
        timestamp: 0,
        deps: [`${head!.counter}@0`],
        lamport: change.lamport + head!.counter - change.counter + 1,
        msg: null,
        ops: [
          {
            container: "cid:root-tree:Tree",
            content: {
              type: "move",
              target: `${targetCounter}@0`,
              parent: null,
              fractional_index: "80",
            },
            counter: 0,
          },
        ],
      },
    ],
  } as never);
  expect(doc.getTree("tree").isNodeDeleted(target)).toBe(false);
}

function docWithRowsDeletedBeforeCut(rows: number, fillerOps: number) {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const map = doc.getMap("rows");
  const ids: ContainerID[] = [];
  for (let index = 0; index < rows; index += 1) {
    const row = map.setContainer(`r${index}`, new LoroMap());
    row.set("title", `row ${index}`);
    ids.push(row.id);
  }
  doc.commit();
  for (let index = 0; index < rows; index += 1) map.delete(`r${index}`);
  doc.commit();
  const cut = doc.frontiers();
  filler(doc, fillerOps);
  return { doc, cut, ids };
}

/** `p -> c`; `p` is deleted before the cut and `c` is moved to the root after it. */
function docWithChildRevivedFromDeletedParent(fillerOps: number) {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const tree = doc.getTree("tree");
  const p = tree.createNode().id;
  const c = tree.createNode(p).id;
  fillMeta(doc, p, "parent");
  fillMeta(doc, c, "child");
  doc.commit();
  tree.delete(p);
  doc.commit();
  const cut = doc.frontiers();
  tree.move(c);
  doc.commit();
  filler(doc, fillerOps);
  return { doc, cut, c };
}

describe("shallow snapshots drop containers deleted before the root", () => {
  for (const fillerOps of RETAINED_FILLER) {
    test(`drops deleted map children (${fillerOps} retained ops)`, () => {
      const { doc, cut, ids } = docWithRowsDeletedBeforeCut(50, fillerOps);
      const bytes = doc.export({ mode: "shallow-snapshot", frontiers: cut });
      const stored = storedContainers(bytes);
      const loaded = importBytes(bytes);
      expect(loaded.toJSON()).toEqual(doc.toJSON());
      for (const id of ids) {
        expect(stored.has(id)).toBe(false);
        expect(loaded.hasContainer(id)).toBe(false);
      }
    });
  }

  test("keeps containers created after the root", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.getMap("seed").set("a", 1);
    doc.commit();
    const cut = doc.frontiers();
    const rows = doc.getMap("rows");
    const ids: ContainerID[] = [];
    for (let index = 0; index < 300; index += 1) {
      const row = rows.setContainer(`r${index}`, new LoroMap());
      row.set("title", `row ${index}`);
      ids.push(row.id);
      doc.commit();
    }
    const loaded = shallowRoundTrip(doc, cut);
    expect(loaded.toJSON()).toEqual(doc.toJSON());
    for (const id of ids) expect(loaded.hasContainer(id)).toBe(true);
  });

  for (const fillerOps of RETAINED_FILLER) {
    test(`keeps the metadata of a node revived from a deleted parent (${fillerOps})`, () => {
      const { doc, cut, c } = docWithChildRevivedFromDeletedParent(fillerOps);
      const loaded = shallowRoundTrip(doc, cut);
      expect(loaded.toJSON()).toEqual(doc.toJSON());
      const meta = loaded.getTree("tree").getNodeByID(c)!.data;
      expect(meta.toJSON()).toEqual(doc.getTree("tree").getNodeByID(c)!.data.toJSON());

      meta.set("after", 1);
      loaded.commit();
      const peer = doc.fork();
      peer.import(loaded.export({ mode: "update" }));
      expect(peer.toJSON()).toEqual(loaded.toJSON());
    });

    test(`re-exports keep the metadata of a revived node (${fillerOps})`, () => {
      const { doc, cut } = docWithChildRevivedFromDeletedParent(fillerOps);
      const shallow = shallowRoundTrip(doc, cut);
      expect(shallowRoundTrip(shallow, shallow.shallowSinceFrontiers()).toJSON()).toEqual(
        doc.toJSON(),
      );
      expect(importBytes(shallow.export({ mode: "snapshot" })).toJSON()).toEqual(
        doc.toJSON(),
      );
    });

    test(`keeps a directly deleted node revived by a remote move (${fillerOps})`, () => {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const tree = doc.getTree("tree");
      const node = tree.createNode().id;
      fillMeta(doc, node, "node");
      doc.commit();
      tree.delete(node);
      doc.commit();
      const cut = doc.frontiers();
      importRemoteMoveToRoot(doc, node);
      filler(doc, fillerOps);
      expect(shallowRoundTrip(doc, cut).toJSON()).toEqual(doc.toJSON());
    });

    test(`checkout matches full history across tree revivals (${fillerOps})`, () => {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const tree = doc.getTree("tree");
      const p = tree.createNode().id;
      const c = tree.createNode(p).id;
      const g = tree.createNode(c).id;
      const d = tree.createNode().id;
      fillMeta(doc, p, "parent");
      fillMeta(doc, c, "child");
      fillMeta(doc, g, "grandchild");
      fillMeta(doc, d, "direct");
      doc.commit();
      tree.delete(p);
      doc.commit();
      tree.delete(d);
      doc.commit();
      const cut = doc.frontiers();

      const versions: Frontiers[] = [cut];
      importRemoteMoveToRoot(doc, d);
      versions.push(doc.frontiers());
      const step = (edit: () => void): void => {
        edit();
        doc.commit();
        versions.push(doc.frontiers());
      };
      step(() => tree.getNodeByID(d)!.data.set("title", "direct v2"));
      step(() => tree.move(c));
      step(() => {
        const meta = tree.getNodeByID(c)!.data;
        meta.set("title", "child v2");
        (meta.get("body") as LoroText).insert(0, "edited ");
      });
      step(() => tree.delete(c));
      step(() => tree.move(g));
      step(() => tree.getNodeByID(g)!.data.set("title", "g v2"));
      step(() => tree.delete(d));
      step(() => tree.delete(g));
      filler(doc, fillerOps);
      versions.push(doc.frontiers());

      const loaded = shallowRoundTrip(doc, cut);
      expect(loaded.toJSON()).toEqual(doc.toJSON());
      const full = importBytes(doc.export({ mode: "snapshot" }));
      for (const order of [versions, [...versions].reverse()]) {
        for (const version of order) {
          full.checkout(version);
          loaded.checkout(version);
          expect({ version, value: loaded.toJSON() }).toEqual({
            version,
            value: full.toJSON(),
          });
        }
      }
    });
  }
});

describe("older shallow snapshots", () => {
  const deadMap: ContainerID = "cid:0@1:Map";

  test("imports a Rust blob whose root state lacks a revived node's metadata", () => {
    // Exported before loro-dev/loro#1119: `p -> c`, meta(c).title, delete(p),
    // cut, then c moved to the root. The metadata itself is not in the blob.
    const doc = importBytes(fixture("legacy-shallow-revived-tree-node.blob"));
    const tree = doc.getTree("tree");
    expect(tree.roots().map((node) => node.id)).toEqual(["1@1"]);
    tree.getNodeByID("1@1")!.data.set("title", "again");
    doc.commit();
    expect(importBytes(doc.export({ mode: "snapshot" })).toJSON()).toEqual(doc.toJSON());
  });

  for (const name of [
    "legacy-shallow-dead-map.blob",
    "legacy-state-only-dead-map.blob",
    "legacy-shallow-tree-revival-dead-map.blob",
    "legacy-shallow-tree-revival-dead-map.ts.blob",
  ]) {
    test(`re-exporting ${name} at its own root drops the dead map`, () => {
      const bytes = fixture(name);
      expect(storedContainers(bytes).has(deadMap)).toBe(true);
      const legacy = importBytes(bytes);
      const expected = legacy.toJSON();
      for (const again of [
        legacy.export({
          mode: "shallow-snapshot",
          frontiers: legacy.shallowSinceFrontiers(),
        }),
        legacy.export({ mode: "snapshot" }),
      ]) {
        expect(storedContainers(again).has(deadMap)).toBe(false);
        const reloaded = importBytes(again);
        expect(reloaded.hasContainer(deadMap)).toBe(false);
        expect(reloaded.toJSON()).toEqual(expected);
      }
    });
  }

  test("a re-exported revival blob matches full history at every retained version", () => {
    const legacy = importBytes(fixture("legacy-shallow-tree-revival-dead-map.ts.blob"));
    const again = shallowRoundTrip(legacy, legacy.shallowSinceFrontiers());
    const versions: Frontiers[] = [];
    for (const [peer, changes] of legacy.getAllChanges()) {
      for (const change of changes) {
        versions.push([{ peer, counter: change.counter + change.length - 1 }]);
      }
    }
    for (const version of versions) {
      legacy.checkout(version);
      again.checkout(version);
      expect({ version, value: again.toJSON() }).toEqual({
        version,
        value: legacy.toJSON(),
      });
    }
  });
});
