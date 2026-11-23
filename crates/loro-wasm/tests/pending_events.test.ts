import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { expect, it, vi } from "vitest";
import { LoroDoc } from "../bundler/index";

it("schedules one event check for a synchronous run of 200,001 commits", async () => {
  // Flush checks from preceding tests before counting this turn's checks.
  await Promise.resolve();
  const doc = new LoroDoc();
  const text = doc.getText("text");
  let delivered = 0;
  const unsubscribe = doc.subscribeLocalUpdates(() => delivered++);
  const resolve = vi.spyOn(Promise, "resolve");
  try {
    for (let index = 0; index < 200_001; index++) {
      text.insert(index, "a");
      doc.commit();
    }
    expect(delivered).toBe(200_001);
    expect(text.toString()).toBe("a".repeat(200_001));
    expect(resolve.mock.calls).toEqual([[null]]);
  } finally {
    resolve.mockRestore();
    unsubscribe();
    text.free();
    doc.free();
  }
}, 30_000);

it("reports an unflushed event behind an already scheduled check", () => {
  const result = spawnSync(process.execPath, ["-e", `
    const assert = require("node:assert/strict");
    const raw = require("./nodejs/loro_wasm.js");
    const rawCommit = raw.LoroDoc.prototype.commit;
    const { LoroDoc, callPendingEvents } = require("./nodejs/index.js");
    const doc = new LoroDoc();
    let delivered = 0;
    const unsubscribe = doc.subscribeLocalUpdates(() => delivered++);
    const text = doc.getText("text");
    text.insert(0, "a");
    doc.commit();
    assert.equal(delivered, 1);
    text.insert(1, "b");
    rawCommit.call(doc);
    assert.equal(delivered, 1);
    queueMicrotask(() => {
      callPendingEvents();
      assert.equal(delivered, 2);
      unsubscribe();
      text.free();
      doc.free();
    });
  `], { cwd: fileURLToPath(new URL("..", import.meta.url)), encoding: "utf8" });
  expect(result.status, result.stderr).toBe(0);
  expect(result.stderr.match(/\[LORO_INTERNAL_ERROR\] Event not called/g)).toHaveLength(1);
});

it("delivers a reentrant commit to a local-update subscription registered by an observer", () => {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  const updates: Uint8Array[] = [];
  let firstUpdate: Uint8Array | undefined;
  let unsubscribeLocal: (() => void) | undefined;
  const unsubscribe = doc.subscribe(() => {
    if (unsubscribeLocal) return;
    firstUpdate = doc.export({ mode: "update" });
    unsubscribeLocal = doc.subscribeLocalUpdates((update) => updates.push(update));
    text.insert(1, "b");
    doc.commit();
  });
  try {
    text.insert(0, "a");
    doc.commit();
    expect(updates).toHaveLength(1);
    const replica = new LoroDoc();
    try {
      expect(firstUpdate).toBeDefined();
      replica.import(firstUpdate!);
      replica.import(updates[0]);
      expect(replica.getText("text").toString()).toBe("ab");
    } finally {
      replica.free();
    }
  } finally {
    unsubscribeLocal?.();
    unsubscribe();
    text.free();
    doc.free();
  }
});
