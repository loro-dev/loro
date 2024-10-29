import { describe, expect, it } from "vitest";
import {
  decodeImportBlobMeta,
  LoroDoc,
  LoroMap,
  OpId,
  VersionVector,
} from "../src";

describe("Frontiers", () => {
  it("two clients", () => {
    const doc = new LoroDoc();
    doc.setPeerId(0);
    const text = doc.getText("text");
    text.insert(0, "0");
    doc.commit();

    const v0 = doc.frontiers();
    const docB = new LoroDoc();
    docB.setPeerId(1);
    docB.import(doc.exportFrom());
    expect(docB.cmpWithFrontiers(v0)).toBe(0);
    text.insert(1, "0");
    doc.commit();
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(-1);
    const textB = docB.getText("text");
    textB.insert(0, "0");
    docB.commit();
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(-1);
    docB.import(doc.exportFrom());
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(1);
    doc.import(docB.exportFrom());
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(0);
  });

  it("cmp frontiers", () => {
    const doc1 = new LoroDoc();
    doc1.setPeerId(1);
    const doc2 = new LoroDoc();
    doc2.setPeerId(2n);

    doc1.getText("text").insert(0, "01234");
    doc2.import(doc1.exportFrom());
    doc2.getText("text").insert(0, "56789");
    doc1.import(doc2.exportFrom());
    doc1.getText("text").insert(0, "01234");
    doc1.commit();

    expect(() => {
      doc1.cmpFrontiers(
        [{ peer: "1", counter: 1 }],
        [
          {
            peer: "2",
            counter: 10,
          },
        ],
      );
    }).toThrow();
    expect(doc1.cmpFrontiers([], [{ peer: "1", counter: 1 }])).toBe(-1);
    expect(doc1.cmpFrontiers([], [])).toBe(0);
    expect(
      doc1.cmpFrontiers(
        [{ peer: "1", counter: 4 }],
        [
          {
            peer: "2",
            counter: 3,
          },
        ],
      ),
    ).toBe(-1);
    expect(
      doc1.cmpFrontiers(
        [{ peer: "1", counter: 5 }],
        [
          {
            peer: "2",
            counter: 3,
          },
        ],
      ),
    ).toBe(1);
  });
});

it("peer id repr should be consistent", () => {
  const doc = new LoroDoc();
  const id = doc.peerIdStr;
  doc.getText("text").insert(0, "hello");
  doc.commit();
  const f = doc.frontiers();
  expect(f[0].peer).toBe(id);
  const child = new LoroMap();
  const map = doc.getList("list").insertContainer(0, child);
  const mapId = map.id;
  const peerIdInContainerId = mapId.split(":")[1].split("@")[1];
  expect(peerIdInContainerId).toBe(id);
  doc.commit();
  expect(doc.version().get(id)).toBe(6);
  expect(doc.version().toJSON().get(id)).toBe(6);
  const m = doc.getMap(mapId);
  m.set("0", 1);
  expect(map.get("0")).toBe(1);
});

describe("Version", () => {
  const a = new LoroDoc();
  a.setPeerId(0n);
  const b = new LoroDoc();
  b.setPeerId(1n);
  a.getText("text").insert(0, "ha");
  b.getText("text").insert(0, "yo");
  a.import(b.exportFrom());
  a.getText("text").insert(0, "k");
  a.commit();

  it("version vector to frontiers", () => {
    {
      const vv = new Map();
      vv.set("0", 3);
      vv.set("1", 2);
      expect(a.version().toJSON()).toStrictEqual(vv);
      expect(a.version().toJSON()).toStrictEqual(vv);
      expect(a.vvToFrontiers(new VersionVector(vv))).toStrictEqual(
        a.frontiers(),
      );
      const v = a.version();
      const temp = a.vvToFrontiers(v);
      expect(temp).toStrictEqual(a.frontiers());
      expect(a.frontiers()).toStrictEqual([
        { peer: "0", counter: 2 },
      ] as OpId[]);
    }
  });

  it("get changes", () => {
    const changes = a.getAllChanges();
    expect(typeof changes.get("0")?.[0].peer == "string").toBeTruthy();
    expect(changes.size).toBe(2);
    expect(changes.get("0")?.length).toBe(2);
    expect(changes.get("0")?.[0].length).toBe(2);
    expect(changes.get("0")?.[1].lamport).toBe(2);
    expect(changes.get("0")?.[1].deps).toStrictEqual([
      { peer: "0", counter: 1 },
      { peer: "1", counter: 1 },
    ]);
    expect(changes.get("1")?.length).toBe(1);
  });

  it("get ops inside changes", () => {
    const change = a.getOpsInChange({ peer: "0", counter: 2 });
    expect(change.length).toBe(1);
  });
});

it("get import blob metadata", () => {
  const doc0 = new LoroDoc();
  doc0.setPeerId(0n);
  const text = doc0.getText("text");
  text.insert(0, "0");
  doc0.commit();
  {
    const bytes = doc0.exportFrom();
    const meta = decodeImportBlobMeta(bytes);
    expect(meta.changeNum).toBe(1);
    expect(meta.partialStartVersionVector.get("0")).toBeFalsy();
    expect(meta.partialEndVersionVector.get("0")).toBe(1);
    expect(meta.startTimestamp).toBe(0);
    expect(meta.endTimestamp).toBe(0);
    expect(meta.isSnapshot).toBeFalsy();
    expect(meta.startFrontiers.length).toBe(0);
  }

  const doc1 = new LoroDoc();
  doc1.setPeerId(1);
  doc1.getText("text").insert(0, "123");
  doc1.import(doc0.exportFrom());
  {
    const bytes = doc1.exportFrom();
    const meta = decodeImportBlobMeta(bytes);
    expect(meta.changeNum).toBe(2);
    expect(meta.partialStartVersionVector.get("0")).toBeFalsy();
    expect(meta.partialEndVersionVector.get("0")).toBe(1);
    expect(meta.partialEndVersionVector.get("1")).toBe(3);
    expect(meta.startTimestamp).toBe(0);
    expect(meta.endTimestamp).toBe(0);
    expect(meta.isSnapshot).toBeFalsy();
    expect(meta.startFrontiers.length).toBe(0);
  }
  {
    const bytes = doc1.exportSnapshot();
    const meta = decodeImportBlobMeta(bytes);
    expect(meta.changeNum).toBe(2);
    expect(meta.partialStartVersionVector.get("0")).toBeFalsy();
    expect(meta.partialEndVersionVector.get("0")).toBe(1);
    expect(meta.partialEndVersionVector.get("1")).toBe(3);
    expect(meta.startTimestamp).toBe(0);
    expect(meta.endTimestamp).toBe(0);
    expect(meta.isSnapshot).toBeTruthy();
    expect(meta.startFrontiers.length).toBe(0);
  }
  {
    const bytes = doc1.exportFrom(doc0.oplogVersion());
    const meta = decodeImportBlobMeta(bytes);
    expect(meta.changeNum).toBe(1);
    expect(meta.partialStartVersionVector.get("0")).toBeUndefined();
    expect(meta.partialStartVersionVector.get("1")).toBeFalsy();
    expect(meta.partialEndVersionVector.get("0")).toBeUndefined();
    expect(meta.partialEndVersionVector.get("1")).toBe(3);
    expect(meta.startTimestamp).toBe(0);
    expect(meta.endTimestamp).toBe(0);
    expect(meta.isSnapshot).toBeFalsy();
    expect(meta.startFrontiers).toStrictEqual([{ peer: "0", counter: 0 }]);
  }
});
