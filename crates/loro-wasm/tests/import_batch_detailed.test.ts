import { expect, it } from "vitest";
import {
  ImportBatchDetail,
  ImportBlobOutcome,
  LoroDoc,
  PeerID,
} from "../bundler/index";

function update(peer: number, write: (doc: LoroDoc) => void): Uint8Array {
  const doc = new LoroDoc();
  doc.setPeerId(peer);
  write(doc);
  doc.commit();
  return doc.export({ mode: "update" });
}

function imported(peer: PeerID): ImportBlobOutcome {
  return {
    type: "imported",
    status: { success: new Map([[peer, { start: 0, end: 1 }]]), pending: null },
  };
}

/** An update whose list insert is past the end: only the state rejects it. */
function updateTheStateRejects(peer: number): Uint8Array {
  const forged = new LoroDoc();
  forged.setPeerId(peer);
  forged.getList("list").push("out of bounds");
  forged.commit();
  const json = forged.exportJsonUpdates();
  (json.changes[0].ops[0].content as { pos: number }).pos = 1000;
  const carrier = new LoroDoc();
  carrier.detach();
  carrier.importJsonUpdates(json);
  return carrier.export({ mode: "update" });
}

it("reports the blob that conflicts and imports the others", () => {
  const good = [
    update(8, (doc) => doc.getMap("m").set("8", true)),
    update(9, (doc) => doc.getMap("m").set("9", true)),
  ];
  const batch = [
    good[0],
    // Reuses 7@0 with other content.
    update(7, (doc) => doc.getText("t").insert(0, "B")),
    good[1],
  ];
  const doc = new LoroDoc();
  doc.import(update(7, (doc) => doc.getText("t").insert(0, "A")));
  let events = 0;
  doc.subscribe(() => events++);

  const detail: ImportBatchDetail = doc.importBatchDetailed(batch);
  expect(detail.blobs[0]).toEqual(imported("8"));
  expect(detail.blobs[1]).toEqual({
    type: "failed",
    error: expect.stringContaining("has been used"),
  });
  expect(detail.blobs[2]).toEqual(imported("9"));
  expect(detail.blobs.length).toBe(3);
  expect(detail.status).toEqual({
    success: new Map([
      ["8", { start: 0, end: 1 }],
      ["9", { start: 0, end: 1 }],
    ]),
    pending: null,
  });
  expect(detail.rollbackError).toBeNull();
  // The events of the batch are delivered before the call returns.
  expect(events).toBe(1);
  expect(doc.toJSON()).toEqual({ t: "A", m: { "8": true, "9": true } });

  // `importBatch` leaves the same document and only throws.
  const twin = new LoroDoc();
  twin.import(update(7, (doc) => doc.getText("t").insert(0, "A")));
  expect(() => twin.importBatch(batch)).toThrow("has been used");
  expect(twin.toJSON()).toEqual(doc.toJSON());
});

it("reports every blob as rolled back when the closing checkout fails", () => {
  const good = [
    update(8, (doc) => doc.getMap("m").set("8", true)),
    update(9, (doc) => doc.getMap("m").set("9", true)),
  ];
  const corrupt = update(6, (doc) => doc.getMap("m").set("6", true));
  // A checksum byte: the metadata still decodes, and the import of the update fails.
  corrupt[19] ^= 0xff;
  const batch = [good[0], updateTheStateRejects(5), corrupt, good[1]];
  const doc = new LoroDoc();
  doc.getText("t").insert(0, "local");
  doc.commit();
  const before = doc.toJSON();
  const version = doc.oplogVersion().toJSON();
  let events = 0;
  doc.subscribe(() => events++);

  const detail = doc.importBatchDetailed(batch);
  expect(detail.rollbackError).toEqual(expect.stringContaining("list diff"));
  expect(detail.blobs).toEqual([
    { type: "rolledBack" },
    { type: "rolledBack" },
    { type: "failed", error: expect.stringContaining("Checksum mismatch") },
    { type: "rolledBack" },
  ]);
  expect(detail.status).toEqual({ success: new Map(), pending: null });
  expect(doc.isDetached()).toBe(false);
  expect(doc.toJSON()).toEqual(before);
  expect(doc.oplogVersion().toJSON()).toEqual(version);
  expect(events).toBe(0);

  const retry = doc.importBatchDetailed(good);
  expect(retry.blobs).toEqual([imported("8"), imported("9")]);
  expect(retry.rollbackError).toBeNull();
  expect(doc.toJSON()).toEqual({ ...before, m: { "8": true, "9": true } });
});

it("fails an update with unreadable metadata alone, where importBatch throws", () => {
  const good = update(8, (doc) => doc.getMap("m").set("8", true));
  const batch = [new Uint8Array([1, 2, 3]), good];
  const doc = new LoroDoc();
  const detail = doc.importBatchDetailed(batch);
  expect(detail.blobs).toEqual([
    { type: "failed", error: expect.stringContaining("Decode error") },
    imported("8"),
  ]);
  expect(doc.toJSON()).toEqual({ m: { "8": true } });

  const twin = new LoroDoc();
  expect(() => twin.importBatch(batch)).toThrow();
  expect(twin.toJSON()).toEqual({});
  expect(new LoroDoc().importBatchDetailed([])).toEqual({
    blobs: [],
    status: { success: new Map(), pending: null },
    rollbackError: null,
  });
});
