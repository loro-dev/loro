import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { LoroDoc, LoroMap, TreeID, UndoManager } from "../bundler/index";

// loro-dev/loro#1158: in a document loaded from a snapshot (or `fork()`), the
// metadata of a tree node created under an already deleted parent had no known
// parent, so touching it trapped with `RuntimeError: unreachable`.
// See context/arena-parent-links.md.

/** Peer 2 creates `child` under `parent` while peer 1 deletes `parent`. */
function history(): { src: LoroDoc; child: TreeID } {
  const a = new LoroDoc();
  a.setPeerId(1n);
  const tree = a.getTree("tree");
  const parent = tree.createNode();
  a.commit();
  const b = new LoroDoc();
  b.setPeerId(2n);
  b.import(a.export({ mode: "update" }));
  const child = b.getTree("tree").createNode(parent.id).id;
  b.commit();
  tree.delete(parent.id);
  a.commit();
  a.import(b.export({ mode: "update" }));
  // Later history, so the child's creation is in an older change block.
  for (let i = 0; i < 5000; i++) {
    a.getMap("m").set(`k${i % 50}`, i);
    a.commit();
  }
  return { src: a, child };
}

function loaded(src: LoroDoc): [string, LoroDoc][] {
  const load = (bytes: Uint8Array) => {
    const doc = new LoroDoc();
    doc.import(bytes);
    doc.setPeerId(9n);
    return doc;
  };
  const fork = src.fork();
  fork.setPeerId(9n);
  return [
    ["updates", load(src.export({ mode: "update" }))],
    ["snapshot", load(src.export({ mode: "snapshot" }))],
    ["fork", fork],
  ];
}

const metaId = (node: TreeID) => `cid:${node}:Map`;

/** An update from another peer that revives `child` and sets `x` in its meta. */
function revivalAndMetaEdit(src: LoroDoc, child: TreeID): Uint8Array {
  const editor = new LoroDoc();
  editor.setPeerId(4n);
  editor.import(src.export({ mode: "update" }));
  const before = editor.oplogVersion();
  editor.getTree("tree").move(child, undefined);
  editor.getTree("tree").getNodeByID(child)!.data.set("x", 1);
  editor.commit();
  return editor.export({ mode: "update", from: before });
}

describe("metadata of a node created under a deleted parent", () => {
  const { src, child } = history();

  it("is deleted and rejects edits", () => {
    for (const [how, doc] of loaded(src)) {
      const meta = doc.getTree("tree").getNodeByID(child)!.data;
      expect(meta.isDeleted(), how).toBe(true);
      expect(() => {
        meta.set("k", 1);
        doc.commit();
      }, how).toThrow(/deleted/);
      expect(doc.getPathToContainer(metaId(child) as any), how).toBeUndefined();
    }
  });

  it("is found by id in a fresh document", () => {
    for (const [how, doc] of loaded(src)) {
      const meta = doc.getContainerById(metaId(child) as any) as LoroMap;
      expect(meta, how).toBeDefined();
      expect(meta.isDeleted(), how).toBe(true);
    }
  });

  it("can be edited after reviving the node", () => {
    for (const [how, doc] of loaded(src)) {
      const tree = doc.getTree("tree");
      const meta = tree.getNodeByID(child)!.data;
      expect(meta.isDeleted(), how).toBe(true);
      tree.move(child, undefined);
      doc.commit();
      expect(meta.isDeleted(), how).toBe(false);
      meta.set("k", 1);
      doc.commit();
      expect(meta.get("k"), how).toBe(1);
    }
  });

  it("emits events for an imported edit", () => {
    const update = revivalAndMetaEdit(src, child);

    let expected: string | undefined;
    for (const [how, doc] of loaded(src)) {
      const seen: string[] = [];
      doc.subscribe((e) => {
        for (const ev of e.events)
          seen.push(`${ev.target} ${JSON.stringify(ev.path)}`);
      });
      doc.import(update);
      const meta = doc.getTree("tree").getNodeByID(child)!.data;
      expect(meta.get("x"), how).toBe(1);
      const got = JSON.stringify(seen);
      if (expected === undefined) expected = got;
      else expect(got, how).toBe(expected);
    }
  });

  it("undoes a local edit after importing an edit of the meta", () => {
    const update = revivalAndMetaEdit(src, child);
    let expected: string | undefined;
    for (const [how, doc] of loaded(src)) {
      const undo = new UndoManager(doc, { mergeInterval: 0 });
      doc.getMap("m").set("local", 1);
      doc.commit();
      doc.import(update);
      expect(undo.undo(), how).toBe(true);
      expect(undo.redo(), how).toBe(true);
      expect(undo.undo(), how).toBe(true);
      const meta = doc.getTree("tree").getNodeByID(child)!.data;
      expect(meta.get("x"), how).toBe(1);
      expect(doc.getMap("m").get("local"), how).toBeUndefined();
      const got = JSON.stringify(doc.toJSON());
      if (expected === undefined) expected = got;
      else expect(got, how).toBe(expected);
    }
  });

  it("the review harness dump", () => {
    const read = (name: string) =>
      new Uint8Array(
        readFileSync(new URL(`../../loro/tests/${name}`, import.meta.url)),
      );
    const node = "7@40881" as TreeID;
    const fromSnapshot = new LoroDoc();
    fromSnapshot.import(read("unregistered_meta_parent.snapshot.bin"));
    const fromUpdates = new LoroDoc();
    fromUpdates.import(read("unregistered_meta_parent.updates.bin"));
    for (const [how, doc] of [
      ["updates", fromUpdates],
      ["fork", fromSnapshot.fork()],
      ["snapshot", fromSnapshot],
    ] as [string, LoroDoc][]) {
      const meta = doc.getTree("tree").getNodeByID(node)!.data;
      expect(meta.isDeleted(), how).toBe(true);
      expect(() => {
        meta.delete("c0");
        doc.commit();
      }, how).toThrow(/deleted/);
    }
  });
});
