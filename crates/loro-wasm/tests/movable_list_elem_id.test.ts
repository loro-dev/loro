import { describe, expect, it } from "vitest";
import { type ElemId, LoroDoc, LoroMovableList } from "../bundler/index";

function idsOf(list: LoroMovableList): ElemId[] {
  return list.getElemIds().map((id) => ({ peer: id.peer, lamport: id.lamport }));
}

describe("movable list element ids", () => {
  it("keeps the insert id across move and set, and returns undefined past the end", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getMovableList("mq");
    expect(list.getElemIds()).toEqual([]);
    expect(list.getElemIdAt(0)).toBeUndefined();

    list.push("a");
    list.push("b");
    list.push("c");
    doc.commit();
    const created = idsOf(list);
    expect(created).toEqual([
      { peer: "1", lamport: 0 },
      { peer: "1", lamport: 1 },
      { peer: "1", lamport: 2 },
    ]);
    expect(list.getElemIdAt(0)).toEqual(created[0]);
    expect(list.getIndexOfElemId(created[0]!)).toBe(0);

    list.move(0, 2);
    list.set(2, "A");
    doc.commit();
    expect(list.toJSON()).toEqual(["b", "c", "A"]);
    expect(idsOf(list)).toEqual([created[1], created[2], created[0]]);
    expect(list.getIndexOfElemId(created[0]!)).toBe(2);
    expect(list.getElemIdAt(3)).toBeUndefined();
    expect(list.getIndexOfElemId({ peer: "1", lamport: 9 })).toBeUndefined();

    const deleted = list.getElemIdAt(0)!;
    list.delete(0, 1);
    doc.commit();
    expect(list.getIndexOfElemId(deleted)).toBeUndefined();
    expect(idsOf(list).some((id) => id.lamport === deleted.lamport)).toBe(false);
  });

  it("gives a detached container no element ids", () => {
    const list = new LoroMovableList();
    list.push("a");
    list.push("b");
    expect(list.toJSON()).toEqual(["a", "b"]);
    expect(list.getElemIds()).toEqual([]);
    expect(list.getElemIdAt(0)).toBeUndefined();
    expect(list.getIndexOfElemId({ peer: "1", lamport: 0 })).toBeUndefined();
  });

  it("checkout and a shallow snapshot report the ids visible at that version", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getMovableList("mq");
    list.push("a");
    list.push("b");
    doc.commit();
    const atInsert = doc.frontiers();
    const inserted = idsOf(list);

    list.move(0, 1);
    doc.commit();
    const moved = idsOf(list);
    expect(moved).toEqual([inserted[1], inserted[0]]);

    doc.checkout(atInsert);
    expect(doc.isDetached()).toBe(true);
    expect(idsOf(list)).toEqual(inserted);
    expect(list.toJSON()).toEqual(["a", "b"]);
    doc.checkoutToLatest();
    expect(idsOf(list)).toEqual(moved);

    const shallow = new LoroDoc();
    shallow.import(
      doc.export({ mode: "shallow-snapshot", frontiers: atInsert }),
    );
    expect(shallow.isShallow()).toBe(true);
    const shallowList = shallow.getMovableList("mq");
    expect(idsOf(shallowList)).toEqual(moved);
    expect(shallowList.getIndexOfElemId(moved[1]!)).toBe(1);

    shallow.checkout(atInsert);
    expect(shallow.isDetached()).toBe(true);
    expect(idsOf(shallowList)).toEqual(inserted);
    expect(shallowList.toJSON()).toEqual(["a", "b"]);
    shallow.checkoutToLatest();
    expect(idsOf(shallowList)).toEqual(moved);

    const trimmed = new LoroDoc();
    trimmed.import(
      doc.export({
        mode: "shallow-snapshot",
        frontiers: doc.oplogFrontiers(),
      }),
    );
    expect(trimmed.isShallow()).toBe(true);
    expect(idsOf(trimmed.getMovableList("mq"))).toEqual(moved);
    expect(() => trimmed.checkout(atInsert)).toThrow(/shallow/);
  });

  it("rejects a malformed element id without trapping", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getMovableList("mq");
    list.push("a");
    expect(() =>
      list.getIndexOfElemId({ peer: "1", lamport: 1.5 }),
    ).toThrow(/lamport/);
  });
});
