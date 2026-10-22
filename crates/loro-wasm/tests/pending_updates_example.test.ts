import { LoroDoc } from "../bundler/index";
import { expect, it } from "vitest";

it("runs the documented pending-update checkpoint example against real WASM", () => {
  const source = new LoroDoc();
  source.setPeerId("1");
  source.getText("text").insert(0, "base|");
  const baseSnapshot = source.export({ mode: "snapshot" });
  const beforeA = source.oplogVersion();
  source.getText("text").insert(5, "A");
  const a = source.export({ mode: "update", from: beforeA });
  const beforeB = source.oplogVersion();
  source.getText("text").insert(6, "B");
  const b = source.export({ mode: "update", from: beforeB });

  const doc = new LoroDoc();
  doc.import(baseSnapshot);
  doc.import(b); // A is missing, so B stays pending.
  // Coordinate checkpoint collection and storage with document mutations.
  let savedCheckpoint = {
    snapshot: doc.export({ mode: "snapshot" }),
    pending: doc.exportPendingUpdates(),
  };

  const restored = new LoroDoc();
  restored.import(savedCheckpoint.snapshot);
  if (savedCheckpoint.pending !== undefined)
    restored.import(savedCheckpoint.pending);
  console.assert(restored.getText("text").toString() === "base|");
  expect(restored.getText("text").toString()).toBe("base|");
  expect(restored.exportPendingUpdates()).toBeInstanceOf(Uint8Array);
  restored.import(a);
  console.assert(restored.getText("text").toString() === "base|AB");
  expect(restored.getText("text").toString()).toBe("base|AB");

  savedCheckpoint = {
    snapshot: restored.export({ mode: "snapshot" }),
    pending: restored.exportPendingUpdates(),
  };
  console.assert(savedCheckpoint.pending === undefined);
  expect(savedCheckpoint.pending).toBeUndefined();
  // After both exports succeed, replace the checkpoint and remove its old pending sidecar.
});
