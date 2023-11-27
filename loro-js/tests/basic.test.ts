import { describe, expect, it } from "vitest";
import {
  ContainerID,
  Loro,
  LoroList,
  LoroMap,
  setPanicHook,
  toEncodedVersion,
} from "../src";

setPanicHook();

it("basic example", () => {
  const doc = new Loro();
  const list: LoroList = doc.getList("list");
  list.insert(0, "A");
  list.insert(1, "B");
  list.insert(2, "C");

  const map: LoroMap = doc.getMap("map");
  // map can only has string key
  map.set("key", "value");
  expect(doc.toJson()).toStrictEqual({
    list: ["A", "B", "C"],
    map: { key: "value" },
  });

  // delete 2 elements at index 0
  list.delete(0, 2);
  expect(doc.toJson()).toStrictEqual({
    list: ["C"],
    map: { key: "value" },
  });

  // Insert a text container to the list
  const text = list.insertContainer(0, "Text");
  text.insert(0, "Hello");
  text.insert(0, "Hi! ");

  // delete 1 element at index 0
  expect(doc.toJson()).toStrictEqual({
    list: ["Hi! Hello", "C"],
    map: { key: "value" },
  });

  // Insert a list container to the map
  const list2 = map.setContainer("test", "List");
  list2.insert(0, 1);
  expect(doc.toJson()).toStrictEqual({
    list: ["Hi! Hello", "C"],
    map: { key: "value", test: [1] },
  });
});

it("basic sync example", () => {
  const docA = new Loro();
  const docB = new Loro();
  const listA: LoroList = docA.getList("list");
  listA.insert(0, "A");
  listA.insert(1, "B");
  listA.insert(2, "C");
  // B import the ops from A
  docB.import(docA.exportFrom());
  expect(docB.toJson()).toStrictEqual({
    list: ["A", "B", "C"],
  });

  const listB: LoroList = docB.getList("list");
  // delete 1 element at index 1
  listB.delete(1, 1);
  // A import the ops from B
  docA.import(docB.exportFrom(docA.version()));
  // list at A is now ["A", "C"], with the same state as B
  expect(docA.toJson()).toStrictEqual({
    list: ["A", "C"],
  });
  expect(docA.toJson()).toStrictEqual(docB.toJson());
});

it("basic events", () => {
  const doc = new Loro();
  doc.subscribe((event) => {});
  const list = doc.getList("list");
});

describe("list", () => {
  it("insert containers", () => {
    const doc = new Loro();
    const list = doc.getList("list");
    const map = list.insertContainer(0, "Map");
    map.set("key", "value");
    const v = list.get(0) as LoroMap;
    console.log(v);
    expect(v instanceof LoroMap).toBeTruthy();
    expect(v.getDeepValue()).toStrictEqual({ key: "value" });
  });

  it.todo("iterate");
});

describe("map", () => {
  it("get child container", () => {
    const doc = new Loro();
    const map = doc.getMap("map");
    const list = map.setContainer("key", "List");
    list.insert(0, 1);
    expect(map.get("key") instanceof LoroList).toBeTruthy();
    expect((map.get("key") as LoroList).getDeepValue()).toStrictEqual([1]);
  });

  it("set large int", () => {
    const doc = new Loro();
    const map = doc.getMap("map");
    map.set("key", 2147483699);
    expect(map.get("key")).toBe(2147483699);
  });
});

describe("import", () => {
  it("pending", () => {
    const a = new Loro();
    a.getText("text").insert(0, "a");
    const b = new Loro();
    b.import(a.exportFrom());
    b.getText("text").insert(1, "b");
    const c = new Loro();
    c.import(b.exportFrom());
    c.getText("text").insert(2, "c");

    // c export from b's version, which cannot be imported directly to a.
    // This operation is pending.
    a.import(c.exportFrom(b.version()));
    expect(a.getText("text").toString()).toBe("a");

    // a import the missing ops from b. It makes the pending operation from c valid.
    a.import(b.exportFrom(a.version()));
    expect(a.getText("text").toString()).toBe("abc");
  });

  it("import by frontiers", () => {
    const a = new Loro();
    a.getText("text").insert(0, "a");
    const b = new Loro();
    b.import(a.exportFrom());
    b.getText("text").insert(1, "b");
    b.getList("list").insert(0, [1, 2]);
    const updates = b.exportFrom(
      toEncodedVersion(b.frontiersToVV(a.frontiers())),
    );
    a.import(updates);
    expect(a.toJson()).toStrictEqual(b.toJson());
  });

  it("from snapshot", () => {
    const a = new Loro();
    a.getText("text").insert(0, "hello");
    const bytes = a.exportSnapshot();
    const b = Loro.fromSnapshot(bytes);
    b.getText("text").insert(0, "123");
    expect(b.toJson()).toStrictEqual({ text: "123hello" });
  });

  it("importBatch Error #181", () => {
    const docA = new Loro();
    const updateA = docA.exportSnapshot();
    const docB = new Loro();
    docB.importUpdateBatch([updateA]);
    docB.getText("text").insert(0, "hello");
    docB.commit();
    console.log(docB.exportFrom());
  });
});

describe("map", () => {
  it("keys", () => {
    const doc = new Loro();
    const map = doc.getMap("map");
    map.set("foo", "bar");
    map.set("baz", "bar");
    const entries = map.keys();
    expect(entries).toStrictEqual(["foo", "baz"]);
  });

  it("values", () => {
    const doc = new Loro();
    const map = doc.getMap("map");
    map.set("foo", "bar");
    map.set("baz", "bar");
    const entries = map.values();
    expect(entries).toStrictEqual(["bar", "bar"]);
  });

  it("entries", () => {
    const doc = new Loro();
    const map = doc.getMap("map");
    map.set("foo", "bar");
    map.set("baz", "bar");
    map.set("new", 11);
    map.delete("new");
    const entries = map.entries();
    expect(entries).toStrictEqual([
      ["foo", "bar"],
      ["baz", "bar"],
    ]);
  });
});

it("handlers should still be usable after doc is dropped", () => {
  const doc = new Loro();
  const text = doc.getText("text");
  const list = doc.getList("list");
  const map = doc.getMap("map");
  doc.free();
  text.insert(0, "123");
  expect(text.toString()).toBe("123");
  list.insert(0, 1);
  expect(list.getDeepValue()).toStrictEqual([1]);
  map.set("k", 8);
  expect(map.getDeepValue()).toStrictEqual({ k: 8 });
});
