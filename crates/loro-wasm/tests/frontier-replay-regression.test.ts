import { expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

it("concurrent list rebuilds preserve edits, versions and checkout across unrelated history", () => {
  const base = new LoroDoc();
  for (let p = 1; p <= 80; p++) {
    base.setPeerId(p);
    base.getMap("history").set("last", p);
    base.commit();
  }
  const snapshot = base.export({ mode: "snapshot" });
  const initial = base.toJSON();
  const initialHeads = base.frontiers();
  const from = base.oplogVersion();
  const branches = [101, 102, 103].map(id => {
    const doc = new LoroDoc(); doc.import(snapshot); doc.setPeerId(id);
    doc.getMovableList("items").insert(0, id);
    doc.getText("text").insert(0, String(id));
    doc.getMap("properties").set(String(id), id); doc.commit();
    return doc;
  });
  const first = branches.map(doc => doc.export({ mode: "update", from }));
  const second = branches.map((doc, i) => {
    const version = doc.oplogVersion();
    doc.getMovableList("items").insert(0, 200 + i);
    doc.getMovableList("items").move(0, 1);
    doc.getText("text").delete(0, 1); doc.commit();
    return doc.export({ mode: "update", from: version });
  });
  const a = new LoroDoc(); a.import(snapshot);
  first.forEach(u => a.import(u)); second.forEach(u => a.import(u));
  const b = new LoroDoc(); b.import(snapshot);
  b.importBatch([...second].reverse()); // Pending dependencies arrive later.
  [...first].reverse().forEach(u => b.import(u));
  expect(b.toJSON()).toEqual(a.toJSON());
  expect(b.oplogVersion().compare(a.oplogVersion())).toBe(0);
  const latest = a.toJSON();
  a.checkout(initialHeads); expect(a.toJSON().history).toEqual(initial.history);
  a.attach(); expect(a.toJSON()).toEqual(latest);
  const cold = new LoroDoc(); cold.import(a.export({ mode: "snapshot" }));
  expect(cold.toJSON()).toEqual(latest);
  for (const doc of [base, ...branches, a, b, cold]) doc.free();
});
