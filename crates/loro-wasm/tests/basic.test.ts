import { describe, expect, expectTypeOf, it } from "vitest";
import {
  Container,
  Diff,
  getType,
  isContainer,
  LoroDoc,
  LoroList,
  LoroMap,
  LoroText,
  LoroTree,
  VersionVector,
  MapDiff,
  TextDiff,
  Frontiers,
  encodeFrontiers,
  decodeFrontiers,
  OpId,
  ContainerID,
  LoroCounter,
  JsonDiff,
  redactJsonUpdates,
  UndoManager,
} from "../bundler/index";

it("basic example", () => {
  const doc = new LoroDoc();
  const list = doc.getList("list");
  list.insert(0, "A");
  list.insert(1, "B");
  list.insert(2, "C");

  const map = doc.getMap("map");
  // map can only has string key
  map.set("key", "value");
  expect(doc.toJSON()).toStrictEqual({
    list: ["A", "B", "C"],
    map: { key: "value" },
  });

  // delete 2 elements at index 0
  list.delete(0, 2);
  expect(doc.toJSON()).toStrictEqual({
    list: ["C"],
    map: { key: "value" },
  });

  // Insert a text container to the list
  const text = list.insertContainer(0, new LoroText());
  text.insert(0, "Hello");
  text.insert(0, "Hi! ");

  // delete 1 element at index 0
  expect(doc.toJSON()).toStrictEqual({
    list: ["Hi! Hello", "C"],
    map: { key: "value" },
  });

  // Insert a list container to the map
  const list2 = map.setContainer("test", new LoroList());
  list2.insert(0, 1);
  expect(doc.toJSON()).toStrictEqual({
    list: ["Hi! Hello", "C"],
    map: { key: "value", test: [1] },
  });
});

it("get or create on Map", () => {
  const docA = new LoroDoc();
  const map = docA.getMap("map");
  const container = map.getOrCreateContainer("list", new LoroList());
  container.insert(0, 1);
  container.insert(0, 2);
  const text = map.getOrCreateContainer("text", new LoroText());
  text.insert(0, "Hello");
  expect(docA.toJSON()).toStrictEqual({
    map: { list: [2, 1], text: "Hello" },
  });
});

it("basic sync example", () => {
  const docA = new LoroDoc();
  const docB = new LoroDoc();
  const listA = docA.getList("list");
  listA.insert(0, "A");
  listA.insert(1, "B");
  listA.insert(2, "C");
  // B import the ops from A
  docB.import(docA.export({ mode: "update" }));
  expect(docB.toJSON()).toStrictEqual({
    list: ["A", "B", "C"],
  });

  const listB = docB.getList("list");
  // delete 1 element at index 1
  listB.delete(1, 1);
  // A import the ops from B
  docA.import(docB.export({ mode: "update", from: docA.version() }));
  // list at A is now ["A", "C"], with the same state as B
  expect(docA.toJSON()).toStrictEqual({
    list: ["A", "C"],
  });
  expect(docA.toJSON()).toStrictEqual(docB.toJSON());
});

describe("list", () => {
  it("insert containers", () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    const map = list.insertContainer(0, new LoroMap());
    map.set("key", "value");
    const v = list.get(0) as LoroMap;
    expect(v instanceof LoroMap).toBeTruthy();
    expect(v.toJSON()).toStrictEqual({ key: "value" });
  });

  it("toArray", () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    list.insert(0, 1);
    list.insert(1, 2);
    expect(list.toArray()).toStrictEqual([1, 2]);
    list.insertContainer(2, new LoroText());
    const t = list.toArray()[2];
    expect(isContainer(t)).toBeTruthy();
    expect(getType(t)).toBe("Text");
    expect(getType(123)).toBe("Json");
  });

  it("convertPos bridges utf16/unicode/utf8", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    const content = "A😀BC"; // emoji is 2 UTF-16 units
    text.insert(0, content);

    expect(text.convertPos(0, "unicode", "utf16")).toBe(0);
    expect(text.convertPos(1, "unicode", "utf16")).toBe(1);
    expect(text.convertPos(2, "unicode", "utf16")).toBe(3);

    expect(text.convertPos(3, "utf16", "unicode")).toBe(2);

    const utf8BeforeEmoji = new TextEncoder().encode("A").length;
    expect(text.convertPos(1, "unicode", "utf8")).toBe(utf8BeforeEmoji);

    expect(text.convertPos(999, "unicode", "utf16")).toBeUndefined();
    expect(text.convertPos(1, "unicode", "unknown" as any)).toBeUndefined();
  });
});

describe("map", () => {
  it("get child container", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    const list = map.setContainer("key", new LoroList());
    list.insert(0, 1);
    expect(map.get("key") instanceof LoroList).toBeTruthy();
    expect((map.get("key") as LoroList).toJSON()).toStrictEqual([1]);
  });

  it("set large int", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("key", 2147483699);
    expect(map.get("key")).toBe(2147483699);
  });
});

it("top undo/redo values", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);

  let lastCommitLabel: string | null = null;
  let lastPoppedLabel: string | null = null;

  const undo = new UndoManager(doc, {
    mergeInterval: 0,
    onPush: (isUndo: boolean) => {
      // For normal commits (Undo stack), use the label set before commit.
      // For opposite pushes (Redo stack), reuse the last popped label.
      const value = isUndo ? lastCommitLabel : lastPoppedLabel;
      return { value, cursors: [] };
    },
    onPop: (_isUndo: boolean, meta: { value: unknown }) => {
      // Remember last popped label so we can assign it to the opposite stack item.
      lastPoppedLabel = meta.value as string | null;
    },
  });

  // 1st commit
  doc.getText("text").insert(0, "A");
  lastCommitLabel = 'Insert "A"';
  doc.commit();
  expect(undo.topUndoValue()).toBe('Insert "A"');
  expect(undo.topRedoValue()).toBe(undefined);

  // 2nd commit
  doc.getText("text").insert(1, "B");
  lastCommitLabel = 'Insert "B"';
  doc.commit();
  expect(undo.topUndoValue()).toBe('Insert "B"');

  // Undo once: the popped undo label should appear as the redo top value
  undo.undo();
  expect(undo.topRedoValue()).toBe('Insert "B"');
  expect(undo.topUndoValue()).toBe('Insert "A"');
});

describe("import", () => {
  it("pending and import status", () => {
    const a = new LoroDoc();
    a.setPeerId(0);
    a.getText("text").insert(0, "a");
    const b = new LoroDoc();
    b.setPeerId(1);
    b.import(a.export({ mode: "update" }));
    b.getText("text").insert(1, "b");
    const c = new LoroDoc();
    c.setPeerId(2);
    c.import(b.export({ mode: "update" }));
    c.getText("text").insert(2, "c");

    // c export from b's version, which cannot be imported directly to a.
    // This operation is pending.
    const status = a.import(c.export({ mode: "update", from: b.version() }));
    const pending = new Map();
    pending.set("2", { start: 0, end: 1 });
    expect(status).toStrictEqual({ success: new Map(), pending });
    expect(a.getText("text").toString()).toBe("a");

    // a import the missing ops from b. It makes the pending operation from c valid.
    const status2 = a.import(b.export({ mode: "update", from: a.version() }));
    pending.set("1", { start: 0, end: 1 });
    expect(status2).toStrictEqual({ success: pending, pending: null });
    expect(a.getText("text").toString()).toBe("abc");
  });

  it("import by frontiers", () => {
    const a = new LoroDoc();
    a.getText("text").insert(0, "a");
    const b = new LoroDoc();
    b.import(a.export({ mode: "update" }));
    b.getText("text").insert(1, "b");
    b.getList("list").insert(0, [1, 2]);
    const updates = b.export({
      mode: "update",
      from: b.frontiersToVV(a.frontiers()),
    });
    a.import(updates);
    expect(a.toJSON()).toStrictEqual(b.toJSON());
  });

  it("from snapshot", () => {
    const a = new LoroDoc();
    a.getText("text").insert(0, "hello");
    const bytes = a.export({ mode: "snapshot" });
    const b = LoroDoc.fromSnapshot(bytes);
    b.getText("text").insert(0, "123");
    expect(b.toJSON()).toStrictEqual({ text: "123hello" });
  });

  it("importBatch Error #181", () => {
    const docA = new LoroDoc();
    const updateA = docA.export({ mode: "snapshot" });
    const docB = new LoroDoc();
    docB.importBatch([updateA]);
    docB.getText("text").insert(0, "hello");
    docB.commit();
  });
});

describe("map", () => {
  it("keys", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("foo", "bar");
    map.set("baz", "bar");
    const entries = map.keys();
    expect(entries).toStrictEqual(["baz", "foo"]);
  });

  it("values", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("foo", "bar");
    map.set("baz", "bar");
    const entries = map.values();
    expect(entries).toStrictEqual(["bar", "bar"]);
  });

  it("entries", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("foo", "bar");
    map.set("baz", "bar");
    map.set("new", 11);
    map.delete("new");
    const entries = map.entries();
    expect(entries).toStrictEqual([
      ["baz", "bar"],
      ["foo", "bar"],
    ]);
  });

  it("entries should return container handlers", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.setContainer("text", new LoroText());
    map.set("foo", "bar");
    const entries = map.entries();
    expect((entries[1][1]! as Container).kind() === "Text").toBeTruthy();
  });
});

it("handlers should still be usable after doc is dropped", () => {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  const list = doc.getList("list");
  const map = doc.getMap("map");
  doc.free();
  text.insert(0, "123");
  expect(text.toString()).toBe("123");
  list.insert(0, 1);
  expect(list.toJSON()).toStrictEqual([1]);
  map.set("k", 8);
  expect(map.toJSON()).toStrictEqual({ k: 8 });
});

it("get change with given lamport", () => {
  const doc1 = new LoroDoc();
  doc1.setPeerId(1);
  const doc2 = new LoroDoc();
  doc2.setPeerId(2);
  doc1.getText("text").insert(0, "01234");
  doc2.import(doc1.export({ mode: "update" }));
  doc2.getText("text").insert(0, "56789");
  doc1.import(doc2.export({ mode: "update" }));
  doc1.getText("text").insert(0, "01234");
  doc1.commit();
  {
    const change = doc1.getChangeAtLamport("1", 1)!;
    expect(change.lamport).toBe(0);
    expect(change.peer).toBe("1");
    expect(change.length).toBe(5);
  }
  {
    const change = doc1.getChangeAtLamport("1", 7)!;
    expect(change.lamport).toBe(0);
    expect(change.peer).toBe("1");
    expect(change.length).toBe(5);
  }
  {
    const change = doc1.getChangeAtLamport("1", 10)!;
    expect(change.lamport).toBe(10);
    expect(change.peer).toBe("1");
    expect(change.length).toBe(5);
  }
  {
    const change = doc1.getChangeAtLamport("1", 13)!;
    expect(change.lamport).toBe(10);
    expect(change.peer).toBe("1");
    expect(change.length).toBe(5);
  }
  {
    const change = doc1.getChangeAtLamport("1", 20)!;
    expect(change.lamport).toBe(10);
    expect(change.peer).toBe("1");
    expect(change.length).toBe(5);
  }
  {
    const change = doc1.getChangeAtLamport("111", 13);
    expect(change).toBeUndefined();
  }
});

it("isContainer", () => {
  expect(isContainer("123")).toBeFalsy();
  expect(isContainer(123)).toBeFalsy();
  expect(isContainer(123n)).toBeFalsy();
  expect(isContainer(new Map())).toBeFalsy();
  expect(isContainer(new Set())).toBeFalsy();
  expect(isContainer({})).toBeFalsy();
  expect(isContainer(undefined)).toBeFalsy();
  expect(isContainer(null)).toBeFalsy();
  const doc = new LoroDoc();
  const t = doc.getText("t");
  expect(isContainer(t)).toBeTruthy();
  expect(isContainer(doc.getMap("m"))).toBeTruthy();
  expect(isContainer(doc.getMovableList("l"))).toBeTruthy();
  expect(isContainer(doc.getText("text"))).toBeTruthy();
  expect(isContainer(doc.getTree("tree"))).toBeTruthy();
  expect(isContainer(doc.getList("list"))).toBeTruthy();
  expect(getType(t)).toBe("Text");
  expect(getType(123)).toBe("Json");
});

it("getValueType", () => {
  // Type tests
  const doc = new LoroDoc();
  const t = doc.getText("t");
  expectTypeOf(getType(t)).toEqualTypeOf<"Text">();
  expect(getType(t)).toBe("Text");
  expectTypeOf(getType(123)).toEqualTypeOf<"Json">();
  expect(getType(123)).toBe("Json");
  expectTypeOf(getType(undefined)).toEqualTypeOf<"Json">();
  expect(getType(undefined)).toBe("Json");
  expectTypeOf(getType(null)).toEqualTypeOf<"Json">();
  expect(getType(null)).toBe("Json");
  expectTypeOf(getType({})).toEqualTypeOf<"Json">();
  expect(getType({})).toBe("Json");

  const map = doc.getMap("map");
  const list = doc.getList("list");
  const tree = doc.getTree("tree");
  const text = doc.getText("text");
  expectTypeOf(getType(map)).toEqualTypeOf<"Map">();
  expect(getType(map)).toBe("Map");
  expectTypeOf(getType(list)).toEqualTypeOf<"List">();
  expect(getType(list)).toBe("List");
  expectTypeOf(getType(tree)).toEqualTypeOf<"Tree">();
  expect(getType(tree)).toBe("Tree");
  expectTypeOf(getType(text)).toEqualTypeOf<"Text">();
  expect(getType(text)).toBe("Text");
});

it("enable timestamp", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.getText("123").insert(0, "123");
  doc.commit();
  {
    const c = doc.getChangeAt({ peer: "1", counter: 0 });
    expect(c.timestamp).toBe(0);
  }

  doc.setRecordTimestamp(true);
  doc.getText("123").insert(0, "123");
  doc.commit();
  {
    const c = doc.getChangeAt({ peer: "1", counter: 4 });
    expect(c.timestamp).toBeCloseTo(Date.now() / 1000, -1);
  }
});

it("commit with specified timestamp", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.getText("123").insert(0, "123");
  doc.commit({ timestamp: 111 });
  const c = doc.getChangeAt({ peer: "1", counter: 0 });
  expect(c.timestamp).toBe(111);
});

it("can control the mergeable interval", () => {
  {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.getText("123").insert(0, "1");
    doc.commit({ timestamp: 110 });
    doc.getText("123").insert(0, "1");
    doc.commit({ timestamp: 120 });
    expect(doc.getAllChanges().get("1")?.length).toBe(1);
  }

  {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.setChangeMergeInterval(9);
    doc.getText("123").insert(0, "1");
    doc.commit({ timestamp: 110 });
    doc.getText("123").insert(0, "1");
    doc.commit({ timestamp: 120 });
    console.log(doc.getAllChanges());
    expect(doc.getAllChanges().get("1")?.length).toBe(2);
  }
});

it("get container parent", () => {
  const doc = new LoroDoc();
  const m = doc.getMap("m");
  expect(m.parent()).toBeUndefined();
  const list = m.setContainer("t", new LoroList());
  expect(list.parent()!.id).toBe(m.id);
  const text = list.insertContainer(0, new LoroText());
  expect(text.parent()!.id).toBe(list.id);
  const tree = list.insertContainer(1, new LoroTree());
  expect(tree.parent()!.id).toBe(list.id);
  const treeNode = tree.createNode();
  const subtext = treeNode.data.setContainer("t", new LoroText());
  expect(subtext.parent()!.id).toBe(treeNode.data.id);
});

it("prelim support", () => {
  // Now we can create a new container directly
  const map = new LoroMap();
  map.set("3", 2);
  const list = new LoroList();
  list.insertContainer(0, map);
  // map should still be valid
  map.set("9", 9);
  // the type of setContainer/insertContainer changed
  const text = map.setContainer("text", new LoroText());
  {
    // Changes will be reflected in the container tree
    text.insert(0, "Heello");
    expect(list.toJSON()).toStrictEqual([{ "3": 2, "9": 9, text: "Heello" }]);
    text.delete(1, 1);
    expect(list.toJSON()).toStrictEqual([{ "3": 2, "9": 9, text: "Hello" }]);
  }
  const doc = new LoroDoc();
  const rootMap = doc.getMap("map");
  rootMap.setContainer("test", map); // new way to create sub-container

  // Use getAttached() to get the attached version of text
  const attachedText = text.getAttached()!;
  expect(text.isAttached()).toBeFalsy();
  expect(attachedText.isAttached()).toBeTruthy();
  text.insert(0, "Detached ");
  attachedText.insert(0, "Attached ");
  expect(text.toString()).toBe("Detached Hello");
  expect(doc.toJSON()).toStrictEqual({
    map: {
      test: {
        "3": 2,
        "9": 9,
        text: "Attached Hello",
      },
    },
  });
});

it("get elem by path", () => {
  const doc = new LoroDoc();
  const map = doc.getMap("map");
  map.set("key", 1);
  expect(doc.getByPath("map/key")).toBe(1);
  const map1 = doc.getByPath("map") as LoroMap;
  expect(getType(map1)).toBe("Map");
  map1.set("key1", 1);
  expect(doc.getByPath("map/key1")).toBe(1);
});

it("fork", () => {
  const doc = new LoroDoc();
  const map = doc.getMap("map");
  map.set("key", 1);
  const doc2 = doc.fork();
  const map2 = doc2.getMap("map");
  expect(map2.get("key")).toBe(1);
  expect(doc2.toJSON()).toStrictEqual({ map: { key: 1 } });
  map2.set("key", 2);
  expect(doc.toJSON()).toStrictEqual({ map: { key: 1 } });
  expect(doc2.toJSON()).toStrictEqual({ map: { key: 2 } });
  doc.import(doc2.export({ mode: "snapshot" }));
  expect(doc.toJSON()).toStrictEqual({ map: { key: 2 } });
});

describe("export", () => {
  it("test export update", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "123");
    doc.commit();
    const updates = doc.export({
      mode: "update",
      from: new VersionVector(null),
    });
    const doc2 = new LoroDoc();
    doc2.import(updates);
    expect(doc2.toJSON()).toStrictEqual({ text: "123" });
  });

  it("test export snapshot", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "123");
    doc.commit();
    const snapshot = doc.export({ mode: "snapshot" });
    const doc2 = new LoroDoc();
    doc2.import(snapshot);
    expect(doc2.toJSON()).toStrictEqual({ text: "123" });
  });

  it("test export shallow-snapshot", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "123");
    doc.commit();
    const snapshot = doc.export({
      mode: "shallow-snapshot",
      frontiers: doc.oplogFrontiers(),
    });
    const doc2 = new LoroDoc();
    doc2.import(snapshot);
    expect(doc2.toJSON()).toStrictEqual({ text: "123" });
  });

  it("test export updates-in-range", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.getText("text").insert(0, "123");
    doc.commit();
    const bytes = doc.export({
      mode: "updates-in-range",
      spans: [{ id: { peer: "1", counter: 0 }, len: 1 }],
    });
    const doc2 = new LoroDoc();
    doc2.import(bytes);
    expect(doc2.toJSON()).toStrictEqual({ text: "1" });
  });
});
it("has correct map value #453", async () => {
  {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "Hello");
    text.mark({ start: 0, end: 2 }, "bold", { b: {} });
    expect(text.toDelta()).toStrictEqual([
      { insert: "He", attributes: { bold: { b: {} } } },
      { insert: "llo" },
    ]);
    let diff: Diff | undefined;
    let expectedDiff: TextDiff = {
      type: "text",
      diff: [
        { insert: "He", attributes: { bold: { b: {} } } },
        { insert: "llo" },
      ],
    };
    doc.subscribe((e) => {
      console.log("Text", e);
      diff = e.events[0].diff;
    });
    doc.commit();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(diff).toStrictEqual(expectedDiff);
  }
  {
    const map = new LoroMap();
    map.set("a", { b: {} });
    expect(map.toJSON()).toStrictEqual({ a: { b: {} } });
  }
  {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("a", { b: {} });
    doc.commit();
    expect(map.toJSON()).toStrictEqual({ a: { b: {} } });
  }
  {
    const doc = new LoroDoc();
    let diff: Diff | undefined;
    const expectedDiff: MapDiff = {
      type: "map",
      updated: {
        a: {
          b: {},
        },
      },
    };
    doc.subscribe((e) => {
      diff = e.events[0].diff;
    });
    const map = doc.getMap("map");
    map.set("a", { b: {} });
    doc.commit();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(diff).toStrictEqual(expectedDiff);
  }
});

it("can set commit message", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.getText("text").insert(0, "123");
  doc.commit({ message: "Hello world" });
  expect(doc.getChangeAt({ peer: "1", counter: 0 }).message).toBe(
    "Hello world",
  );
});

it("can set next commit options", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  doc.getText("text").insert(0, "123");
  doc.setNextCommitOptions({
    message: "test message",
    origin: "test origin",
    timestamp: 123,
  });
  doc.commit();
  const change = doc.getChangeAt({ peer: "1", counter: 0 });
  expect(change.message).toBe("test message");
  expect(change.timestamp).toBe(123);
});

it("can set next commit origin", async () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  let eventOrigin = "";
  doc.subscribe((e) => {
    eventOrigin = e.origin ?? "";
  });
  doc.getText("text").insert(0, "123");
  doc.setNextCommitOrigin("test origin");
  doc.commit();
  await Promise.resolve();
  expect(eventOrigin).toBe("test origin");
});

it("can set next commit timestamp", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  doc.getText("text").insert(0, "123");
  doc.setNextCommitTimestamp(456);
  doc.commit();
  const change = doc.getChangeAt({ peer: "1", counter: 0 });
  expect(change.timestamp).toBe(456);
});

it("can clear next commit options", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  doc.getText("text").insert(0, "123");
  doc.setNextCommitOptions({
    message: "test message",
    origin: "test origin",
    timestamp: 123,
  });
  doc.clearNextCommitOptions();
  doc.commit();
  const change = doc.getChangeAt({ peer: "1", counter: 0 });
  expect(change.message).toBeUndefined();
  expect(change.timestamp).toBe(0);
});

it("commit options persist across implicit empty commits", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");

  // Set options for first commit
  doc.getText("text").insert(0, "123");
  doc.setNextCommitOptions({ message: "first commit", timestamp: 100 });
  doc.commit();

  // Set options again; do NOT explicitly commit.
  // Trigger an implicit commit via export, which should preserve
  // options across the empty commit boundary.
  doc.setNextCommitOptions({ message: "second commit", timestamp: 200 });
  doc.export({ mode: "snapshot" });
  // Options should persist for second commit
  doc.getText("text").insert(3, "456");
  doc.commit();

  const firstChange = doc.getChangeAt({ peer: "1", counter: 0 });
  const secondChange = doc.getChangeAt({ peer: "1", counter: 3 });

  expect(firstChange.message).toBe("first commit");
  expect(firstChange.timestamp).toBe(100);
  expect(secondChange.message).toBe("second commit");
  expect(secondChange.timestamp).toBe(200);
});

it("origin does not persist across empty commits", async () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");

  let firstOrigin = "<unset>";
  let count = 0;
  const unsubscribe = doc.subscribe((e) => {
    if (count === 0) {
      firstOrigin = e.origin ?? "";
      count++;
    }
  });

  // Empty commit with an origin should not leak to the next commit
  doc.commit({ origin: "A" });

  // Make a real change and commit
  doc.getText("text").insert(0, "x");
  doc.commit();
  await Promise.resolve();
  expect(firstOrigin).toBe("");
  unsubscribe();
});

it("explicit empty commit swallows next commit options", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");

  // Set options and perform an explicit empty commit
  doc.setNextCommitOptions({ message: "swallow", timestamp: 123 });
  doc.commit();

  // Next real commit should NOT carry those options
  doc.getText("text").insert(0, "x");
  doc.commit();
  const change = doc.getChangeAt({ peer: "1", counter: 0 });
  expect(change.message).toBeUndefined();
  expect(change.timestamp).toBe(0);
});

it("can query pending txn length", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  expect(doc.getPendingTxnLength()).toBe(0);
  doc.getText("text").insert(0, "123");
  expect(doc.getPendingTxnLength()).toBe(3);
  doc.commit();
  expect(doc.getPendingTxnLength()).toBe(0);
});

it("can encode/decode frontiers", () => {
  const frontiers = [
    { peer: "222", counter: 2 },
    { peer: "1123", counter: 1 },
  ] as Frontiers;
  const encoded = encodeFrontiers(frontiers);
  const decoded = decodeFrontiers(encoded);
  expect(decoded).toStrictEqual(frontiers);
});

it("travel changes", () => {
  let doc = new LoroDoc();
  doc.setPeerId(1);
  doc.getText("text").insert(0, "abc");
  doc.commit();
  let n = 0;
  doc.travelChangeAncestors([{ peer: "1", counter: 0 }], (meta: any) => {
    n += 1;
    return true;
  });
  expect(n).toBe(1);
});

it("get path to container", () => {
  const doc = new LoroDoc();
  const map = doc.getMap("map");
  const list = map.setContainer("list", new LoroList());
  const sub = list.insertContainer(0, new LoroMap());
  const path = doc.getPathToContainer(sub.id);
  expect(path).toStrictEqual(["map", "list", 0]);
});

it("json path", () => {
  const doc = new LoroDoc();
  const map = doc.getMap("map");
  map.set("key", "value");
  const books = map.setContainer("books", new LoroList());
  const book = books.insertContainer(0, new LoroMap());
  book.set("title", "1984");
  book.set("author", "George Orwell");
  const path = "$['map'].books[0].title";
  const result = doc.JSONPath(path);
  expect(result.length).toBe(1);
  expect(result).toStrictEqual(["1984"]);
});

it("can push string to text", () => {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  text.push("123");
  expect(text.toString()).toBe("123");
});

it("can push container to list", () => {
  const doc = new LoroDoc();
  const list = doc.getList("list");
  const map = list.pushContainer(new LoroMap());
  expect(list.toJSON()).toStrictEqual([{}]);
});

it("can push container to movable list", () => {
  const doc = new LoroDoc();
  const list = doc.getMovableList("list");
  const map = list.pushContainer(new LoroMap());
  expect(list.toJSON()).toStrictEqual([{}]);
});

it("can query the history for changed containers", () => {
  const doc = new LoroDoc();
  doc.setPeerId("0");
  doc.getText("text").insert(0, "H");
  doc.getMap("map").set("key", "H");
  const changed = doc.getChangedContainersIn({ peer: "0", counter: 0 }, 2);
  const changedSet = new Set(changed);
  expect(changedSet).toEqual(
    new Set([
      "cid:root-text:Text" as ContainerID,
      "cid:root-map:Map" as ContainerID,
    ]),
  );
});

it("update VV", () => {
  const vv = new VersionVector(null);
  vv.setEnd({ peer: "1", counter: 1 });
  vv.setLast({ peer: "2", counter: 1 });
  vv.setLast({ peer: "3", counter: 4 });
  vv.remove("3");
  const map = vv.toJSON();
  expect(map).toStrictEqual(
    new Map([
      ["1", 1],
      ["2", 2],
    ]),
  );
});

describe("isDeleted", () => {
  it("test text container deletion", () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    expect(list.isDeleted()).toBe(false);
    const tree = doc.getTree("root");
    const node = tree.createNode();
    const containerBefore = node.data.setContainer("container", new LoroMap());
    containerBefore.set("A", "B");
    tree.delete(node.id);
    const containerAfter = node.data;
    expect(containerAfter.isDeleted()).toBe(true);
  });

  it("movable list setContainer", () => {
    const doc = new LoroDoc();
    const list = doc.getMovableList("list1");
    const map = list.insertContainer(0, new LoroMap());
    expect(map.isDeleted()).toBe(false);
    list.set(0, 1);
    expect(map.isDeleted()).toBe(true);
  });

  it("map set", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    const sub = map.setContainer("sub", new LoroMap());
    expect(sub.isDeleted()).toBe(false);
    map.set("sub", "value");
    expect(sub.isDeleted()).toBe(true);
  });

  it("remote map set", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    const sub = map.setContainer("sub", new LoroMap());

    const docB = new LoroDoc();
    docB.import(doc.export({ mode: "snapshot" }));
    const subB = docB.getByPath("map/sub") as LoroMap;
    expect(sub.isDeleted()).toBe(false);
    expect(subB.isDeleted()).toBe(false);

    map.set("sub", "value");
    docB.import(doc.export({ mode: "snapshot" }));

    expect(sub.isDeleted()).toBe(true);
    expect(subB.isDeleted()).toBe(true);
  });
});

it("test import batch", () => {
  const doc1 = new LoroDoc();
  doc1.setPeerId("1");
  doc1.getText("text").insert(0, "Hello world!");

  const doc2 = new LoroDoc();
  doc2.setPeerId("2");
  doc2.getText("text").insert(0, "Hello world!");

  const blob11 = doc1.export({
    mode: "updates-in-range",
    spans: [{ id: { peer: "1", counter: 0 }, len: 5 }],
  });
  const blob12 = doc1.export({
    mode: "updates-in-range",
    spans: [{ id: { peer: "1", counter: 5 }, len: 2 }],
  });
  const blob13 = doc1.export({
    mode: "updates-in-range",
    spans: [{ id: { peer: "1", counter: 6 }, len: 6 }],
  });

  const blob21 = doc2.export({
    mode: "updates-in-range",
    spans: [{ id: { peer: "2", counter: 0 }, len: 5 }],
  });
  const blob22 = doc2.export({
    mode: "updates-in-range",
    spans: [{ id: { peer: "2", counter: 5 }, len: 1 }],
  });
  const blob23 = doc2.export({
    mode: "updates-in-range",
    spans: [{ id: { peer: "2", counter: 6 }, len: 6 }],
  });

  const newDoc = new LoroDoc();
  const status = newDoc.importBatch([blob11, blob13, blob21, blob23]);

  expect(status.success).toEqual(
    new Map([
      ["1", { start: 0, end: 5 }],
      ["2", { start: 0, end: 5 }],
    ]),
  );
  expect(status.pending).toEqual(
    new Map([
      ["1", { start: 6, end: 12 }],
      ["2", { start: 6, end: 12 }],
    ]),
  );

  const status2 = newDoc.importBatch([blob12, blob22]);
  expect(status2.success).toEqual(
    new Map([
      ["1", { start: 5, end: 12 }],
      ["2", { start: 5, end: 12 }],
    ]),
  );
  expect(status2.pending).toBeNull();
  expect(newDoc.getText("text").toString()).toBe("Hello world!Hello world!");
});

it("iter on text #577", () => {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  text.insert(0, "Hello");
  text.iter((_: string) => {
    return null as any;
  });
  text.insert(3, " ");
  const result: string[] = [];
  text.iter((s: string) => {
    result.push(s);
    return true;
  });
  expect(result).toStrictEqual(["Hel", " ", "lo"]);
});

it("can get shallow value of containers", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");

  // Test Text container
  const text = doc.getText("text");
  text.insert(0, "Hello");
  expect(text.getShallowValue()).toBe("Hello");

  // Test Map container
  const map = doc.getMap("map");
  map.set("key", "value");
  const subText = map.setContainer("text", new LoroText());
  subText.insert(0, "Hello");
  expect(map.getShallowValue()).toStrictEqual({
    key: "value",
    text: "cid:6@1:Text",
  });

  // Test List container
  const list = doc.getList("list");
  list.insert(0, 1);
  list.insert(1, "two");
  const subMap = list.insertContainer(2, new LoroMap());
  subMap.set("key", "value");
  expect(list.getShallowValue()).toStrictEqual([1, "two", "cid:14@1:Map"]);

  // Test MovableList container
  const movableList = doc.getMovableList("movable");
  movableList.insert(0, 1);
  movableList.insert(1, "two");
  const subList = movableList.insertContainer(2, new LoroList());
  subList.insert(0, "sub");
  expect(movableList.getShallowValue()).toStrictEqual([
    1,
    "two",
    "cid:18@1:List",
  ]);

  // Test Tree container
  const tree = doc.getTree("tree");
  const root = tree.createNode();
  root.data.set("key", "value");
  const child = root.createNode();
  child.data.set("child", true);
  expect(tree.getShallowValue()).toStrictEqual([
    {
      id: root.id,
      parent: null,
      index: 0,
      fractional_index: "80",
      meta: "cid:20@1:Map",
      children: [
        {
          id: child.id,
          parent: root.id,
          index: 0,
          fractional_index: "80",
          meta: "cid:22@1:Map",
          children: [],
        },
      ],
    },
  ]);

  const value = doc.getShallowValue();
  expect(value).toStrictEqual({
    list: "cid:root-list:List",
    map: "cid:root-map:Map",
    movable: "cid:root-movable:MovableList",
    tree: "cid:root-tree:Tree",
    text: "cid:root-text:Text",
  });
});

it("tree shallow value vs toJSON", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const tree = doc.getTree("tree");
  const root = tree.createNode();
  root.data.set("name", "root");
  const text = root.data.setContainer("content", new LoroText());
  text.insert(0, "Hello");

  expect(tree.getShallowValue()).toStrictEqual([
    {
      id: "0@1",
      parent: null,
      index: 0,
      fractional_index: "80",
      meta: "cid:0@1:Map",
      children: [],
    },
  ]);

  expect(tree.toJSON()).toStrictEqual([
    {
      id: "0@1",
      parent: null,
      index: 0,
      fractional_index: "80",
      meta: {
        name: "root",
        content: "Hello",
      },
      children: [],
    },
  ]);
});

it("map shallow value vs toJSON", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const map = doc.getMap("map");
  map.set("key", "value");
  const subText = map.setContainer("text", new LoroText());
  subText.insert(0, "Hello");

  expect(map.getShallowValue()).toStrictEqual({
    key: "value",
    text: "cid:1@1:Text",
  });

  expect(map.toJSON()).toStrictEqual({
    key: "value",
    text: "Hello",
  });
});

it("list shallow value vs toJSON", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const list = doc.getList("list");
  list.insert(0, 1);
  list.insert(1, "two");
  const subList = list.insertContainer(2, new LoroList());
  subList.insert(0, "sub");

  expect(list.getShallowValue()).toStrictEqual([1, "two", "cid:2@1:List"]);

  expect(list.toJSON()).toStrictEqual([1, "two", ["sub"]]);
});

it("can use version vector multiple times", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  doc.getText("text").update("Hello");
  doc.commit();
  const v = doc.version();
  v.toJSON();
  doc.exportJsonUpdates(v, v);
  v.toJSON();
  doc.exportJsonUpdates(v, v);
  v.toJSON();
  doc.export({ mode: "update", from: v });
  v.toJSON();
  doc.vvToFrontiers(v);
  v.toJSON();
});

it("detach and attach on empty doc", () => {
  const doc = new LoroDoc();
  expect(doc.isDetached()).toBe(false);
  doc.detach();
  expect(doc.isDetached()).toBe(true);
  doc.attach();
  expect(doc.isDetached()).toBe(false);
});

it("export json in id span #602", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  doc.getText("text").insert(0, "Hello");
  doc.commit();
  {
    const changes = doc.exportJsonInIdSpan({
      peer: "1",
      counter: 0,
      length: 1,
    });
    expect(changes).toStrictEqual([
      {
        id: "0@1",
        timestamp: expect.any(Number),
        deps: [],
        lamport: 0,
        msg: undefined,
        ops: [
          {
            container: "cid:root-text:Text",
            counter: 0,
            content: {
              type: "insert",
              pos: 0,
              text: "H",
            },
          },
        ],
      },
    ]);
  }
  {
    const changes = doc.exportJsonInIdSpan({
      peer: "2",
      counter: 0,
      length: 1,
    });
    expect(changes).toStrictEqual([]);
  }
});

it("find spans between versions", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");

  // Make some changes to create version history
  doc.getText("text").insert(0, "Hello");
  doc.commit({ message: "a" });
  const f1 = doc.oplogFrontiers();

  doc.getText("text").insert(5, " World");
  doc.commit({ message: "b" });
  const f2 = doc.oplogFrontiers();

  // Test finding spans between frontiers (f1 -> f2)
  let diff = doc.findIdSpansBetween(f1, f2);
  expect(diff.retreat).toHaveLength(0); // No changes needed to go from f2 to f1
  expect(diff.forward).toHaveLength(1); // One change needed to go from f1 to f2
  expect(diff.forward[0]).toEqual({
    peer: "1",
    counter: 5,
    length: 6,
  });

  // Test empty frontiers
  const emptyFrontiers: OpId[] = [];
  diff = doc.findIdSpansBetween(emptyFrontiers, f2);
  expect(diff.retreat).toHaveLength(0); // No changes needed to go from f2 to empty
  expect(diff.forward).toHaveLength(1); // One change needed to go from empty to f2
  expect(diff.forward[0]).toEqual({
    peer: "1",
    counter: 0,
    length: 11,
  });

  // Test with multiple peers
  const doc2 = new LoroDoc();
  doc2.setPeerId("2");
  doc2.getText("text").insert(0, "Hi");
  doc2.commit();
  doc.import(doc2.export({ mode: "snapshot" }));
  const f3 = doc.oplogFrontiers();

  // Test finding spans between f2 and f3
  diff = doc.findIdSpansBetween(f2, f3);
  expect(diff.retreat).toHaveLength(0); // No changes needed to go from f3 to f2
  expect(diff.forward).toHaveLength(1); // One change needed to go from f2 to f3
  expect(diff.forward[0]).toEqual({
    peer: "2",
    counter: 0,
    length: 2,
  });

  // Test spans in both directions between f1 and f3
  diff = doc.findIdSpansBetween(f1, f3);
  expect(diff.retreat).toHaveLength(0); // No changes needed to go from f3 to f1
  expect(diff.forward).toHaveLength(2); // Two changes needed to go from f1 to f3
  const forwardSpans = new Map(diff.forward.map((span) => [span.peer, span]));
  expect(forwardSpans.get("1")).toEqual({
    peer: "1",
    counter: 5,
    length: 6,
  });
  expect(forwardSpans.get("2")).toEqual({
    peer: "2",
    counter: 0,
    length: 2,
  });

  // Test spans in reverse direction (f3 -> f1)
  diff = doc.findIdSpansBetween(f3, f1);
  expect(diff.forward).toHaveLength(0); // No changes needed to go from f3 to f1
  expect(diff.retreat).toHaveLength(2); // Two changes needed to go from f1 to f3
  const retreatSpans = new Map(diff.retreat.map((span) => [span.peer, span]));
  expect(retreatSpans.get("1")).toEqual({
    peer: "1",
    counter: 5,
    length: 6,
  });
  expect(retreatSpans.get("2")).toEqual({
    peer: "2",
    counter: 0,
    length: 2,
  });
});

it("can travel changes from event", async () => {
  const docA = new LoroDoc();
  docA.setPeerId("1");
  const docB = new LoroDoc();

  docA.getText("text").update("Hello");
  docA.commit();
  const snapshot = docA.export({ mode: "snapshot" });
  let done = false;
  docB.subscribe((e) => {
    const spans = docB.findIdSpansBetween(e.from, e.to);
    expect(spans.retreat).toHaveLength(0);
    expect(spans.forward).toHaveLength(1);
    expect(spans.forward[0]).toEqual({
      peer: "1",
      counter: 0,
      length: 5,
    });
    const changes = docB.exportJsonInIdSpan(spans.forward[0]);
    expect(changes).toStrictEqual([
      {
        id: "0@1",
        timestamp: expect.any(Number),
        deps: [],
        lamport: 0,
        msg: undefined,
        ops: [
          {
            container: "cid:root-text:Text",
            counter: 0,
            content: {
              type: "insert",
              pos: 0,
              text: "Hello",
            },
          },
        ],
      },
    ]);
    done = true;
  });
  docB.import(snapshot);
  await Promise.resolve();
  expect(done).toBe(true);
});

it("can revert to frontiers", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  doc.getText("text").update("Hello");
  doc.commit();
  doc.revertTo([{ peer: "1", counter: 1 }]);
  expect(doc.getText("text").toString()).toBe("He");
});

it("can revert with child container recreation", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const list = doc.getList("list");
  list.insert(0, "item1");
  list.insert(1, "item2");
  const text = list.insertContainer(2, new LoroText());
  text.insert(0, "Hello");
  const v = doc.frontiers();
  text.delete(0, 5);
  list.clear();
  const vEmpty = doc.frontiers();
  doc.commit();
  expect(doc.toJSON()).toStrictEqual({
    list: [],
  });
  for (let i = 0; i < 10; i++) {
    doc.revertTo(v);
    expect(doc.toJSON()).toStrictEqual({
      list: ["item1", "item2", "Hello"],
    });
    doc.revertTo(vEmpty);
    expect(doc.toJSON()).toStrictEqual({
      list: [],
    });
  }
  expect(doc.frontiers()).toStrictEqual([{ peer: "1", counter: 125 }]);
  expect(doc.export({ mode: "snapshot" }).length).toBe(570);
});

it("can diff two versions", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  // Text edits with formatting
  const text = doc.getText("text");
  text.update("Hello");
  text.mark({ start: 0, end: 5 }, "bold", true);
  doc.commit();

  // Map edits
  const map = doc.getMap("map");
  map.set("key1", "value1");
  map.set("key2", 42);
  doc.commit();

  // List edits
  const list = doc.getList("list");
  list.insert(0, "item1");
  list.insert(1, "item2");
  list.delete(1, 1);
  doc.commit();

  // Tree edits
  const tree = doc.getTree("tree");
  const a = tree.createNode();
  a.createNode();
  doc.commit();

  const diff = doc.diff([], doc.frontiers());
  expect(diff).toMatchSnapshot();

  const doc2 = new LoroDoc();
  doc2.setPeerId("2");
  doc2.applyDiff(diff);
  expect(doc2.toJSON()).toMatchSnapshot();
  expect(doc2.getText("text").toDelta()).toStrictEqual(
    doc.getText("text").toDelta(),
  );
});

it("diff two version with serialization", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const text = doc.getMap("map").setContainer("key1", new LoroText());
  text.insert(0, "Hello");
  doc.commit();
  const diff = doc.diff([], doc.frontiers(), true);
  expectTypeOf(diff).toEqualTypeOf<[ContainerID, JsonDiff][]>();
  const newDiff = JSON.parse(JSON.stringify(diff));
  console.dir(newDiff, { depth: 100 });
  const newDoc = new LoroDoc();
  newDoc.applyDiff(newDiff);
  expect(newDoc.toJSON()).toStrictEqual(doc.toJSON());
});

it("apply diff without for_json should work", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const text = doc.getMap("map").setContainer("key1", new LoroText());
  text.insert(0, "Hello");
  doc.commit();
  const diff = doc.diff([], doc.frontiers(), false);
  const newDoc = new LoroDoc();
  newDoc.applyDiff(diff);
  expect(newDoc.toJSON()).toStrictEqual(doc.toJSON());
});

it("applyDiff should remove deleted map entries", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const map = doc.getMap("map");
  map.set("foo", "bar");
  doc.commit();
  const frontiers = doc.frontiers();

  map.delete("foo");
  doc.commit();
  const diff = doc.diff(frontiers, doc.frontiers(), false);

  const newDoc = new LoroDoc();
  newDoc.applyDiff(diff);

  expect(newDoc.getMap("map").entries()).toStrictEqual([]);
  expect(newDoc.toJSON()).toStrictEqual({ map: {} });
});

it("map entries", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const map = doc.getMap("map");
  map.set("foo", "bar");
  doc.commit();
  map.delete("foo");
  expect(map.entries()).toStrictEqual([]);
  expect(doc.toJSON()).toStrictEqual({ map: {} });
});

it("the diff will deduplication", () => {
  const doc = new LoroDoc();
  const list = doc.getList("list");
  const map = doc.getMap("map");
  doc.getText("hi").insert(0, "Hello");
  for (let i = 0; i < 100; i += 1) {
    list.push(1);
    map.set(i.toString(), i);
    doc.setNextCommitMessage("hi " + i);
    doc.commit();
  }

  list.clear();
  map.clear();
  doc.commit();

  const diff = doc.diff([], doc.frontiers());
  expect(diff).toMatchSnapshot();
});

it("merge interval", async () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  doc.setRecordTimestamp(true);
  doc.setChangeMergeInterval(1);
  doc.getText("text").update("Hello");
  doc.commit();
  await new Promise((resolve) => setTimeout(resolve, 100));
  doc.getText("text").update("Hello world!");
  doc.commit();
  await new Promise((resolve) => setTimeout(resolve, 2000));
  doc.getText("text").update("Hello ABC!");
  doc.commit();
  const updates = doc.exportJsonUpdates();
  expect(updates.changes.length).toBe(2);

  await new Promise((resolve) => setTimeout(resolve, 2000));
  doc.getText("text").update("Hello");
  doc.commit();
  await new Promise((resolve) => setTimeout(resolve, 100));
  doc.getText("text").update("Hello world!");
  doc.commit();
  const updates2 = doc.exportJsonUpdates();
  expect(updates2.changes.length).toBe(3);
});

it("setRecordTimestamp should be reflected on current txn", async () => {
  const doc = new LoroDoc();
  doc.getText("text").insert(0, "hi");
  doc.commit();
  {
    const updates = doc.exportJsonUpdates();
    expect(updates.changes[0].timestamp).toBe(0);
  }
  doc.setRecordTimestamp(true);
  doc.getText("text").insert(0, "hi");
  doc.commit();
  const updates = doc.exportJsonUpdates();
  expect(updates.changes[1].timestamp).toBeGreaterThan(0);
});

it("insert counter container", () => {
  function createItem(label: string, checked: boolean) {
    const item = new LoroMap<Record<string, Container>>();

    const $label = new LoroText();
    $label.insert(0, label);

    const $checked = new LoroCounter();
    if (checked) $checked.increment(1);

    item.setContainer("label", $label);
    item.setContainer("checked", $checked);

    return item;
  }

  const item = createItem("hello", true);

  console.log(item.get("label").toString());
  console.log((item.get("checked") as LoroCounter).value);
});

it("move tree nodes within the same parent", () => {
  const doc = new LoroDoc();
  const t = doc.getTree("myTree");
  const root = t.createNode();
  const child = root.createNode();
  for (let i = 0; i < 16; i++) {
    root.createNode();
  }
  child.data.set("test", "test");
  child.move(root);
});

it("should call subscription after diff", async () => {
  const doc = new LoroDoc();
  const tree = doc.getTree("tree");
  doc.commit();
  await Promise.resolve();

  const v0 = doc.version();
  const parent = tree.createNode();
  let called = false;
  const child = tree.createNode(parent.id);
  child.data.subscribe(() => {
    called = true;
  });

  // seems to break the subscription
  doc.diff(doc.vvToFrontiers(doc.version()), doc.vvToFrontiers(v0));

  child.data.set("type", "Hi there");
  doc.commit();

  await Promise.resolve();

  expect(child.data.get("type")).toBe("Hi there");
  expect(called).toBe(true);
});

it("should return map for get_path_by_str", () => {
  const doc = new LoroDoc();
  const tree = doc.getTree("tree");
  const root = tree.createNode();
  const child = root.createNode();
  const grandChild = child.createNode();
  grandChild.data.set("type", "grandChild");
  console.log(doc.getByPath("tree/0/0/0"));
  expect(isContainer(doc.getByPath("tree/0/0/0"))).toBe(true);
  expect((doc.getByPath("tree/0/0/0") as LoroMap).toJSON()).toStrictEqual({
    type: "grandChild",
  });
  expect(doc.getByPath("tree/0/0/0/type")).toBe("grandChild");
});

it("test container existence", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const text = doc.getMap("map").setContainer("text", new LoroText());
  const list = doc.getMap("map").setContainer("list", new LoroList());
  expect(doc.hasContainer("cid:root-map:Map")).toBe(true);
  expect(doc.hasContainer("cid:0@1:Text")).toBe(true);
  expect(doc.hasContainer("cid:1@1:List")).toBe(true);
  const doc2 = new LoroDoc();
  doc.detach();
  doc2.import(doc.export({ mode: "update" }));
  expect(doc2.hasContainer("cid:root-map:Map")).toBe(true);
  expect(doc2.hasContainer("cid:0@1:Text")).toBe(true);
  expect(doc2.hasContainer("cid:1@1:List")).toBe(true);
});

it("redactJsonUpdates removes sensitive content", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");

  // Create some content to be redacted
  const text = doc.getText("text");
  text.insert(0, "Sensitive information");
  doc.commit();

  const map = doc.getMap("map");
  map.set("password", "secret123");
  map.set("public", "public information");
  doc.commit();

  // Export JSON updates
  const jsonUpdates = doc.exportJsonUpdates();

  // Define version range to redact (redact the text content)
  const versionRange = {
    "1": [0, 21], // Redact the "Sensitive information"
  };

  // Apply redaction
  const redactedJson = redactJsonUpdates(jsonUpdates, versionRange);

  // Verify redacted content is replaced
  const redactedDoc = new LoroDoc();
  redactedDoc.importJsonUpdates(redactedJson);

  // Text should be redacted with replacement character
  expect(redactedDoc.getText("text").toString()).toBe("���������������������");

  // Map operations after counter 5 should be intact
  expect(redactedDoc.getMap("map").get("password")).toBe("secret123");
  expect(redactedDoc.getMap("map").get("public")).toBe("public information");

  // Now redact the map content
  const versionRange2 = {
    "1": [21, 22], // Redact the "secret123"
  };

  const redactedJson2 = redactJsonUpdates(jsonUpdates, versionRange2);
  const redactedDoc2 = new LoroDoc();
  redactedDoc2.importJsonUpdates(redactedJson2);

  expect(redactedDoc.getText("text").toString()).toBe("���������������������");
  expect(redactedDoc2.getMap("map").get("password")).toBe(null);
  expect(redactedDoc.getMap("map").get("public")).toBe("public information");
});

it("text mark on LoroText", () => {
  const text = new LoroText();
  text.insert(0, "Hello");
  text.mark({ start: 0, end: 5 }, "bold", true);
});

it("call toDelta on detached text", () => {
  const text = new LoroText();
  text.insert(0, "Hello");
  text.mark({ start: 0, end: 5 }, "bold", true);
  const d = text.toDelta();
  expect(d).toMatchSnapshot();
});

it("can allow default config for text style", () => {
  {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "Hello");
    expect(() => {
      text.mark({ start: 0, end: 5 }, "size", true);
    }).toThrow();
  }
  {
    const doc = new LoroDoc();
    doc.configDefaultTextStyle({ expand: "before" });
    const text = doc.getText("text");
    text.insert(0, "Hello");
    text.mark({ start: 0, end: 5 }, "size", true);
  }
  {
    const text = new LoroText();
    text.insert(0, "Hello");
    text.mark({ start: 0, end: 5 }, "size", true);
  }
});

it("can get pending ops as json", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  expect(doc.getUncommittedOpsAsJson()).toBeUndefined();
  const text = doc.getText("text");
  text.insert(0, "Hello");
  const pendingOps = doc.getUncommittedOpsAsJson();
  expect(pendingOps).toBeDefined();
  expect(JSON.stringify(pendingOps)).toContain("insert");
  expect(JSON.stringify(pendingOps)).toContain("Hello");
  expect(pendingOps).toEqual({
    peers: null,
    schema_version: 1,
    start_version: {},
    changes: [
      {
        id: "0@1",
        deps: [],
        msg: null,
        lamport: 0,
        ops: [
          {
            container: "cid:root-text:Text",
            counter: 0,
            content: {
              type: "insert",
              pos: 0,
              text: "Hello",
            },
          },
        ],
        timestamp: 0,
      },
    ],
  });
});

it("deleteRootContainers", () => {
  const doc = new LoroDoc();
  const _map = doc.getMap("map");
  doc.getMap("m");
  const _text = doc.getText("text");

  doc.deleteRootContainer("cid:root-map:Map");
  doc.deleteRootContainer("cid:root-text:Text");

  expect(doc.toJSON()).toStrictEqual({
    m: {},
  });

  const snapshot = doc.export({ mode: "snapshot" });
  const newDoc = new LoroDoc();
  newDoc.import(snapshot);

  expect(newDoc.toJSON()).toStrictEqual({
    m: {},
  });
});

it("hideEmptyRootContainers", () => {
  const doc = new LoroDoc();
  const map = doc.getMap("map");
  expect(doc.toJSON()).toStrictEqual({ map: {} });
  doc.setHideEmptyRootContainers(true);
  expect(doc.toJSON()).toStrictEqual({});
});

it("fromShallowSnapshot", () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  doc.getText("text").insert(0, "Hello");
  doc.commit();
  const snapshot = doc.export({
    mode: "shallow-snapshot",
    frontiers: doc.frontiers(),
  });
  const newDoc = LoroDoc.fromSnapshot(snapshot);
  expect(newDoc.toJSON()).toStrictEqual({
    text: "Hello",
  });
});

it("update text after switching to a version", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.getText("text").insert(0, "abc");
  const bytes = doc.export({ mode: "snapshot" });
  const newDoc = new LoroDoc();
  newDoc.import(bytes);
  newDoc.checkout([{ peer: "1", counter: 2 }]);
  newDoc.setDetachedEditing(true);
  newDoc.getText("text").update("123");
});

it("tree deleted node to json", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const tree = doc.getTree("tree");
  const root = tree.createNode();
  tree.delete(root.id);
  doc.commit();
  const node = tree.getNodes({ withDeleted: true })[0].toJSON();
  expect(node.parent).toBe("2147483647@18446744073709551615");
  // default value
  expect(node.fractionalIndex).toBe("80");
  expect(node.index).toBe(0);
});

it("counter toJSON", () => {
  const doc = new LoroDoc();
  expect(doc.getCounter("c").toJSON()).toBe(0);
});

it("returns undefined when getting a non-existent cursor", () => {
  const doc = new LoroDoc();
  doc.getText("text").insert(0, "hello");
  const cursor = doc.getText("text").getCursor(2)!;
  const newDoc = new LoroDoc();
  expect(newDoc.getCursorPos(cursor)).toBeUndefined();
});

it("should match when inserting container", () => {
  const doc = new LoroDoc();
  const list = doc.getList("list");
  const text = new LoroText();
  text.insert(0, "");
  list.insertContainer(0, text);

  const retrievedText = list.get(0) as LoroText;
  expect(retrievedText.toString()).toBe("");
});

it("keeps detached text content when inserted", () => {
  const doc = new LoroDoc();
  const list = doc.getList("list");
  const text = new LoroText();
  text.insert(0, "detached");

  list.insertContainer(0, text);

  const retrievedText = list.get(0) as LoroText;
  expect(retrievedText.toString()).toBe("detached");
});

it("keeps detached text styles when inserted", () => {
  const doc = new LoroDoc();
  const list = doc.getList("list");
  const text = new LoroText();
  text.insert(0, "styled");
  text.mark({ start: 0, end: 6 }, "bold", true);

  list.insertContainer(0, text);

  const retrievedText = list.get(0) as LoroText;
  expect(retrievedText.toDelta()).toStrictEqual([
    { insert: "styled", attributes: { bold: true } },
  ]);
});

it("keeps detached unicode text when inserted", () => {
  const doc = new LoroDoc();
  const list = doc.getList("list");
  const text = new LoroText();
  const content = "👨‍👩‍👦 family";
  text.insert(0, content);

  list.insertContainer(0, text);

  const retrievedText = list.get(0) as LoroText;
  expect(retrievedText.toString()).toBe(content);
});

it("keeps detached partial styles when inserted", () => {
  const doc = new LoroDoc();
  const list = doc.getList("list");
  const text = new LoroText();
  text.insert(0, "abcDEF");
  text.mark({ start: 0, end: 3 }, "bold", true);

  list.insertContainer(0, text);

  const retrievedText = list.get(0) as LoroText;
  expect(retrievedText.toDelta()).toStrictEqual([
    { insert: "abc", attributes: { bold: true } },
    { insert: "DEF" },
  ]);
});

it("copies attached text without sharing future edits", () => {
  const doc = new LoroDoc();
  const source = doc.getText("source");
  source.insert(0, "root");
  const list = doc.getList("list");

  list.insertContainer(0, source);
  const copied = list.get(0) as LoroText;

  source.insert(4, "-updated");
  expect(source.toString()).toBe("root-updated");
  expect(copied.toString()).toBe("root");
});

it("throws when inserting an attached text from another doc", () => {
  const docA = new LoroDoc();
  const textA = docA.getText("text");
  textA.insert(0, "cross");

  const docB = new LoroDoc();
  const listB = docB.getList("list");

  expect(() => listB.insertContainer(0, textA)).toThrow();
});

it("apply empty delta", () => {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  text.applyDelta([{ insert: "" }]);
  expect(text.toString()).toBe("");
});
