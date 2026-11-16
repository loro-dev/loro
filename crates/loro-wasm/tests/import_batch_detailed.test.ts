import { expect, it, vi } from "vitest";
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

it.each([false, true])("imports typed-array views without changing their backing buffer (large=%s)", (large) => {
  let seed = 123456789;
  const payload = large
    ? Array.from({ length: 400_000 }, () => {
        seed ^= seed << 13;
        seed ^= seed >>> 17;
        seed ^= seed << 5;
        return String.fromCharCode(33 + ((seed >>> 0) % 90));
      }).join("")
    : "hello";
  const bytes = update(123, (doc) => doc.getText("text").insert(0, payload));
  if (large) expect(bytes.length).toBeGreaterThan(256 * 1024);
  const backing = new Uint8Array(bytes.length + 16).fill(0x7f);
  backing.set(bytes, 8);
  const view = backing.subarray(8, 8 + bytes.length);
  const other = update(124, (doc) => doc.getMap("map").set("ok", true));
  const doc = new LoroDoc();
  doc.importBatch([view, other]);
  expect(doc.getText("text").toString()).toBe(payload);
  const detailed = new LoroDoc();
  expect(detailed.importBatchDetailed([view, other]).blobs.map((b) => b.type)).toEqual(["imported", "imported"]);
  expect(detailed.getText("text").toString()).toBe(payload);
  expect(backing.subarray(0, 8)).toEqual(new Uint8Array(8).fill(0x7f));
  expect(view).toEqual(bytes);
});

it.each([1, 2])("copies each small detailed-batch blob into WASM only once (count=%s)", (count) => {
  const blobs = [update(301, (doc) => doc.getMap("m").set("a", 1)), update(302, (doc) => doc.getMap("m").set("b", 2))];
  const batch = blobs.slice(0, count);
  const lengths = new Set(batch.map((b) => b.length));
  const original = Uint8Array.prototype.set;
  let copies = 0;
  const spy = vi.spyOn(Uint8Array.prototype, "set").mockImplementation(function (this: Uint8Array, source, offset) {
    if (source instanceof Uint8Array && lengths.has(source.length)) copies++;
    original.call(this, source, offset);
  });
  const doc = new LoroDoc();
  try {
    const result = doc.importBatchDetailed(batch);
    expect(result.blobs.map((b) => b.type)).toEqual(batch.map(() => "imported"));
    expect(copies).toBe(count);
  } finally { spy.mockRestore(); }
  expect(doc.getMap("m").toJSON()).toEqual(count === 1 ? { a: 1 } : { a: 1, b: 2 });
});

it("preserves per-blob preflight failures in single and small detailed batches", () => {
  const blob = update(401, (doc) => doc.getMap("m").set("a", 1));
  const doc = new LoroDoc();
  doc.setMemoryBudget({ maxImportBlobBytes: 1 });
  for (const batch of [[blob], [blob, blob]]) {
    const detail = doc.importBatchDetailed(batch);
    expect(detail.blobs.map((b) => b.type)).toEqual(batch.map(() => "failed"));
    for (const result of detail.blobs) {
      if (result.type === "failed") expect(result.error).toContain("kind=import_blob");
    }
    expect(detail.rollbackError).toBeNull();
  }
});
