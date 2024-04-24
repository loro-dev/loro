import { describe, expect, it } from "vitest";
import { Delta, Loro, TextDiff } from "../src";
import {
  Cursor,
  LoroList,
  LoroMovableList,
  OpId,
  PeerID,
  setDebug,
} from "loro-wasm";

describe("movable list", () => {
  it("should work like list", () => {
    const doc = new Loro();
    const list = doc.getMovableList("list");
    expect(list.length).toBe(0);
    list.push("a");
    expect(list.length).toBe(1);
    expect(list.get(0)).toBe("a");
    let v = list.pop();
    expect(list.length).toBe(0);
    expect(v).toBe("a");
  });

  it("can be synced", () => {
    const doc = new Loro();
    const list = doc.getMovableList("list");
    list.push("a");
    list.push("b");
    list.push("c");
    expect(list.toArray()).toEqual(["a", "b", "c"]);
    list.set(2, "d");
    list.move(0, 1);
    const doc2 = new Loro();
    const list2 = doc2.getMovableList("list");
    expect(list2.length).toBe(0);
    doc2.import(doc.exportFrom());
    expect(list2.length).toBe(3);
    expect(list2.get(0)).toBe("b");
    expect(list2.get(1)).toBe("a");
    expect(list2.get(2)).toBe("d");
  });

  it("should support move", () => {
    const doc = new Loro();
    const list = doc.getMovableList("list");
    list.push("a");
    list.push("b");
    list.push("c");
    expect(list.toArray()).toEqual(["a", "b", "c"]);
    list.move(0, 1);
    expect(list.toArray()).toEqual(["b", "a", "c"]);
  });

  it("should support set", () => {
    const doc = new Loro();
    const list = doc.getMovableList("list");
    list.push("a");
    list.push("b");
    list.push("c");
    expect(list.toArray()).toEqual(["a", "b", "c"]);
    list.set(1, "d");
    expect(list.toArray()).toEqual(["a", "d", "c"]);
  });

  it.todo("should support get cursor", () => {
    const doc = new Loro();
    doc.setPeerId(1);
    const list = doc.getMovableList("list");
    list.push("a");
    list.push("b");
    list.push("c");
    expect(list.toArray()).toEqual(["a", "b", "c"]);
    const cursor = list.getCursor(1)!;
    const ans = doc.getCursorPos(cursor);
    expect(ans.offset).toBe(1);
    expect(ans.update).toBeFalsy();

    // cursor position should not be affected by set and move
    list.set(1, "d");
    list.move(1, 2);
    const ans2 = doc.getCursorPos(cursor);
    expect(ans2.offset).toBe(1);
    expect(ans2.update).toBeTruthy();
    const pos = ans2.update?.pos();
    expect(pos).toStrictEqual({ peer: "1", counter: 4 });
  });

  it("inserts sub-container", () => {
    const doc = new Loro();
    const list = doc.getMovableList("list");
    list.push("a");
    list.push("b");
    list.push("c");
    const subList = list.insertContainer(1, new LoroList());
    subList.push("d");
    subList.push("e");
    subList.push("f");
    expect(list.toJson()).toEqual(["a", ["d", "e", "f"], "b", "c"]);
    list.move(1, 0);
    expect(list.toJson()).toEqual([["d", "e", "f"], "a", "b", "c"]);
    list.move(0, 3);
    expect(list.toJson()).toEqual(["a", "b", "c", ["d", "e", "f"]]);
  });

  it("can be inserted into a list as an attached container", () => {
    const doc = new Loro();
    const list = doc.getMovableList("list");
    list.push("a");
    list.push("b");
    list.push("c");
    const blist = doc.getList("blist");
    const newList: LoroMovableList = blist.insertContainer(0, list);
    expect(blist.toJson()).toEqual([["a", "b", "c"]]);
    newList.move(0, 1);
    expect(blist.toJson()).toEqual([["b", "a", "c"]]);
    list.move(0, 2);
    // change on list should not affect blist
    expect(blist.toJson()).toEqual([["b", "a", "c"]]);
  });
});
