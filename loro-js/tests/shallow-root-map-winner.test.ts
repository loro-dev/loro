import { describe, expect, test } from "vitest";

import { LoroDoc, LoroMap, UndoManager, type Frontiers } from "../src/index";

/**
 * Port of crates/loro/tests/shallow_checkout_equal_value.rs (loro-dev/loro#1124).
 *
 * The root commit holds several ops, so the op that wrote a key's root-time
 * value is trimmed from the shallow history. Retreating to the root must then
 * fall back to the shallow root state instead of dropping the key. MovableList
 * is left out: its position/last-set metadata is a separate known gap.
 */
const PADS = [0, 300];

function buildFixture(pad: number, revert: boolean) {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const map = doc.getMap("map");
  map.set("x", "root");
  map.set("untouched", "root");
  const child = map.setContainer("child", new LoroMap());
  child.set("x", 1);
  const sub = map.setContainer("sub", new LoroMap());
  sub.set("k", "v");
  const mergeable = map.ensureMergeableMap("merge");
  mergeable.set("x", "root");
  const list = doc.getList("list");
  list.push("a");
  const text = doc.getText("text");
  text.insert(0, "ab");
  const tree = doc.getTree("tree");
  const n1 = tree.createNode().id;
  const n2 = tree.createNode().id;
  tree.getNodeByID(n1)!.data.set("x", "root");
  const counter = doc.getCounter("counter");
  counter.increment(1);
  doc.commit();
  const root = doc.frontiers();
  const versions: Frontiers[] = [root];
  const commit = (): void => {
    doc.commit();
    versions.push(doc.frontiers());
  };

  map.set("x", "later");
  map.set("sub", "not a map");
  child.set("x", 2);
  mergeable.set("x", "later");
  list.delete(0, 1);
  list.push("a");
  text.delete(0, 1);
  text.insert(0, "a");
  tree.move(n1, n2);
  tree.getNodeByID(n1)!.data.set("x", "later");
  counter.increment(2);
  commit();

  const padMap = doc.getMap("pad");
  for (let index = 0; index < pad; index += 1) padMap.set("k", index);
  commit();

  if (revert) {
    doc.revertTo(root);
  } else {
    map.set("x", "root");
    child.set("x", 1);
    mergeable.set("x", "root");
    tree.move(n1);
    tree.getNodeByID(n1)!.data.set("x", "root");
    counter.decrement(2);
  }
  commit();
  map.set("tail", 1);
  commit();
  return { doc, root, versions };
}

const importBytes = (bytes: Uint8Array): LoroDoc => {
  const doc = new LoroDoc();
  doc.import(bytes);
  return doc;
};

const valueAt = (doc: LoroDoc, version: Frontiers): unknown => {
  doc.checkout(version);
  const value = doc.toJSON();
  doc.checkoutToLatest();
  return value;
};

function expectSameHistory(
  full: LoroDoc,
  shallow: LoroDoc,
  versions: readonly Frontiers[],
  context: string,
): void {
  // The context is part of the compared value so a failure names its step.
  expect({ context, value: shallow.toJSON() }).toEqual({ context, value: full.toJSON() });
  for (const version of versions) {
    expect({ context, version, value: valueAt(shallow, version) }).toEqual({
      context,
      version,
      value: valueAt(full, version),
    });
  }
  // Walk back and forth so later checkouts use incremental transitions.
  for (const version of [...versions].reverse()) {
    shallow.checkout(version);
    full.checkout(version);
    expect({ context, version, value: shallow.toJSON() }).toEqual({
      context,
      version,
      value: full.toJSON(),
    });
  }
  shallow.checkoutToLatest();
  full.checkoutToLatest();
  for (const from of versions) {
    for (const to of versions) {
      const applied = (doc: LoroDoc): unknown => {
        const diff = doc.diff(from, to);
        const fork = full.forkAt(from);
        fork.setDetachedEditing(true);
        fork.applyDiff(diff);
        return fork.toJSON();
      };
      expect({ context, from, to, value: applied(shallow) }).toEqual({
        context,
        from,
        to,
        value: applied(full),
      });
    }
  }
}

describe("shallow checkout of a value rewritten after the root", () => {
  test("keeps a root-time map value whose writer was trimmed", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const map = doc.getMap("map");
    map.set("a", 1);
    map.set("z", 0);
    doc.commit();
    const root = doc.frontiers();
    map.set("a", 2);
    doc.commit();
    const latest = doc.frontiers();

    const shallow = importBytes(
      doc.export({ mode: "shallow-snapshot", frontiers: root }),
    );
    for (let round = 0; round < 2; round += 1) {
      shallow.checkout(root);
      expect(shallow.toJSON()).toEqual({ map: { a: 1, z: 0 } });
      expect(shallow.getMap("map").getLastEditor("a")).toBe("1");
      shallow.checkout(latest);
      expect(shallow.toJSON()).toEqual({ map: { a: 2, z: 0 } });
    }
  });

  test("keeps a node deleted by a retained delete whose placement was trimmed", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const tree = doc.getTree("tree");
    const first = tree.createNode().id;
    const second = tree.createNode().id;
    doc.commit();
    const root = doc.frontiers();
    tree.delete(first);
    doc.commit();
    const deleted = doc.frontiers();

    const shallow = importBytes(
      doc.export({ mode: "shallow-snapshot", frontiers: root }),
    );
    for (let round = 0; round < 2; round += 1) {
      shallow.checkout(root);
      expect(
        shallow
          .getTree("tree")
          .roots()
          .map((node) => node.id),
      ).toEqual([first, second]);
      shallow.checkout(deleted);
      expect(
        shallow
          .getTree("tree")
          .roots()
          .map((node) => node.id),
      ).toEqual([second]);
      expect(shallow.getTree("tree").isNodeDeleted(first)).toBe(true);
    }
  });

  for (const pad of PADS) {
    for (const revert of [false, true]) {
      test(`matches full history (pad=${pad}, revert=${revert})`, () => {
        const { doc, root, versions } = buildFixture(pad, revert);
        const full = importBytes(doc.export({ mode: "snapshot" }));
        const shallow = importBytes(
          doc.export({ mode: "shallow-snapshot", frontiers: root }),
        );
        expect((valueAt(shallow, root) as { map: { x: string } }).map.x).toBe("root");
        expectSameHistory(full, shallow, versions, `pad=${pad} revert=${revert}`);
      });
    }

    test(`re-exports keep the root-time values (pad=${pad})`, () => {
      const { doc, root, versions } = buildFixture(pad, true);
      const full = importBytes(doc.export({ mode: "snapshot" }));
      let shallow = importBytes(
        doc.export({ mode: "shallow-snapshot", frontiers: root }),
      );
      for (let hop = 0; hop < 2; hop += 1) {
        shallow.checkout(root);
        shallow.checkoutToLatest();
        shallow = importBytes(
          shallow.export({ mode: "shallow-snapshot", frontiers: root }),
        );
        expectSameHistory(full, shallow, versions, `pad=${pad} hop=${hop}`);
      }
      const later = importBytes(
        shallow.export({ mode: "shallow-snapshot", frontiers: versions[1]! }),
      );
      expectSameHistory(full, later, versions.slice(1), `pad=${pad} later root`);
    });

    test(`undo matches full history (pad=${pad})`, () => {
      const { doc, root } = buildFixture(pad, true);
      const replicas = [
        importBytes(doc.export({ mode: "snapshot" })),
        importBytes(doc.export({ mode: "shallow-snapshot", frontiers: root })),
      ];
      const values = replicas.map((replica) => {
        replica.setPeerId(2);
        const undo = new UndoManager(replica, {});
        replica.getMap("map").set("x", "local");
        replica.commit();
        undo.undo();
        const afterUndo = replica.toJSON();
        undo.redo();
        return [afterUndo, replica.toJSON()];
      });
      expect(values[1]).toEqual(values[0]);
      expect((values[0]![0] as { map: { x: string } }).map.x).toBe("root");
    });

    test(`reports the root-time writer of an equal value (pad=${pad})`, () => {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const map = doc.getMap("map");
      map.set("x", "root");
      map.set("y", "root");
      doc.commit();
      const root = doc.frontiers();
      map.set("x", "later");
      for (let index = 0; index < pad; index += 1) doc.getMap("pad").set("k", index);
      doc.commit();
      doc.setPeerId(2);
      map.set("x", "root");
      doc.commit();

      const full = importBytes(doc.export({ mode: "snapshot" }));
      const shallow = importBytes(
        doc.export({ mode: "shallow-snapshot", frontiers: root }),
      );
      const expected = valueAt(full, root);
      for (const replica of [full, shallow]) {
        for (let round = 0; round < 2; round += 1) {
          replica.checkout(root);
          expect(replica.getMap("map").getLastEditor("x")).toBe("1");
          expect(replica.toJSON()).toEqual(expected);
          replica.checkoutToLatest();
          expect(replica.getMap("map").getLastEditor("x")).toBe("2");
        }
      }
    });
  }
});
