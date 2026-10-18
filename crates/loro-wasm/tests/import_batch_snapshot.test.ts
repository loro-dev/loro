import { expect, it } from "vitest";
import { LoroDoc, LoroList, LoroMap, LoroText } from "../bundler/index";

it("snapshot batches match split imports with overlapping and concurrent updates", () => {
  const source = new LoroDoc();
  source.setPeerId(1);
  const node = source.getList("blocks").pushContainer(new LoroMap());
  node.set("nodeName", "paragraph");
  node.setContainer("attributes", new LoroMap());
  const children = node.setContainer("children", new LoroList());
  const text = children.pushContainer(new LoroText());
  text.insert(0, "rich text");
  text.mark({ start: 0, end: 4 }, "bold", true);
  source.commit();
  const snapshot = source.export({ mode: "snapshot" });
  const concurrent = new LoroDoc();
  concurrent.setPeerId(2);
  concurrent.import(snapshot);
  concurrent.getMap("metadata").set("concurrent", true);
  concurrent.commit();
  const updates: Uint8Array[] = [];
  for (let i = 0; i < 20; i++) {
    const from = source.oplogVersion();
    text.insert(0, `edit${i} `);
    source.commit();
    updates.push(source.export({ mode: "update", from }));
  }
  for (const n of [1, 20]) {
    const suffix = updates.slice(0, n).reverse();
    suffix.push(updates[0], concurrent.export({ mode: "update" }));
    const split = new LoroDoc();
    split.import(snapshot);
    split.importBatch(suffix);
    const batch = new LoroDoc();
    batch.importBatch([...suffix, snapshot]);
    expect(batch.toJSON()).toEqual(split.toJSON());
    expect((batch.getContainerById(text.id) as LoroText).toDelta()).toEqual(
      (split.getContainerById(text.id) as LoroText).toDelta(),
    );
    expect(batch.frontiers()).toEqual(split.frontiers());
    expect(batch.oplogVersion().toJSON()).toEqual(
      split.oplogVersion().toJSON(),
    );
    expect(batch.isDetached()).toBe(false);
    const roundtrip = new LoroDoc();
    roundtrip.import(batch.export({ mode: "snapshot" }));
    expect(roundtrip.toJSON()).toEqual(split.toJSON());
  }
});

it("a rejected update rolls back the bootstrap snapshot without emitting events", () => {
  const source = new LoroDoc();
  source.getText("text").insert(0, "snapshot");
  const snapshot = source.export({ mode: "snapshot" });
  const forged = new LoroDoc();
  forged.getList("list").push("out of bounds");
  forged.commit();
  const json = forged.exportJsonUpdates();
  (json.changes[0].ops[0].content as { pos: number }).pos = 1000;
  const carrier = new LoroDoc();
  carrier.detach();
  carrier.importJsonUpdates(json);
  const bad = carrier.export({ mode: "update" });
  const target = new LoroDoc();
  const handle = target.getText("existing");
  const before = target.toJSON();
  let events = 0;
  target.subscribe(() => events++);
  expect(() => target.importBatch([snapshot, bad])).toThrow();
  expect(target.isDetached()).toBe(false);
  expect(target.frontiers()).toEqual([]);
  expect(target.oplogVersion().toJSON()).toEqual(new Map());
  expect(target.toJSON()).toEqual(before);
  expect(events).toBe(0);
  handle.insert(0, "usable");
  target.commit();
  expect(handle.toString()).toBe("usable");
  target.import(snapshot);
  expect(target.getText("text").toString()).toBe("snapshot");
});
