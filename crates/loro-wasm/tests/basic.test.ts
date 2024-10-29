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
  docB.import(docA.exportFrom());
  expect(docB.toJSON()).toStrictEqual({
    list: ["A", "B", "C"],
  });

  const listB = docB.getList("list");
  // delete 1 element at index 1
  listB.delete(1, 1);
  // A import the ops from B
  docA.import(docB.exportFrom(docA.version()));
  // list at A is now ["A", "C"], with the same state as B
  expect(docA.toJSON()).toStrictEqual({
    list: ["A", "C"],
  });
  expect(docA.toJSON()).toStrictEqual(docB.toJSON());
});

it("basic events", () => {
  const doc = new LoroDoc();
  doc.subscribe((event) => { });
  const list = doc.getList("list");
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

describe("import", () => {
  it("pending and import status", () => {
    const a = new LoroDoc();
    a.setPeerId(0);
    a.getText("text").insert(0, "a");
    const b = new LoroDoc();
    b.setPeerId(1);
    b.import(a.exportFrom());
    b.getText("text").insert(1, "b");
    const c = new LoroDoc();
    c.setPeerId(2);
    c.import(b.exportFrom());
    c.getText("text").insert(2, "c");

    // c export from b's version, which cannot be imported directly to a.
    // This operation is pending.
    const status = a.import(c.exportFrom(b.version()));
    const pending = new Map();
    pending.set("2", { start: 0, end: 1 });
    expect(status).toStrictEqual({ success: new Map(), pending });
    expect(a.getText("text").toString()).toBe("a");

    // a import the missing ops from b. It makes the pending operation from c valid.
    const status2 = a.import(b.exportFrom(a.version()));
    pending.set("1", { start: 0, end: 1 });
    expect(status2).toStrictEqual({ success: pending, pending: null });
    expect(a.getText("text").toString()).toBe("abc");
  });

  it("import by frontiers", () => {
    const a = new LoroDoc();
    a.getText("text").insert(0, "a");
    const b = new LoroDoc();
    b.import(a.exportFrom());
    b.getText("text").insert(1, "b");
    b.getList("list").insert(0, [1, 2]);
    const updates = b.exportFrom(b.frontiersToVV(a.frontiers()));
    a.import(updates);
    expect(a.toJSON()).toStrictEqual(b.toJSON());
  });

  it("from snapshot", () => {
    const a = new LoroDoc();
    a.getText("text").insert(0, "hello");
    const bytes = a.exportSnapshot();
    const b = LoroDoc.fromSnapshot(bytes);
    b.getText("text").insert(0, "123");
    expect(b.toJSON()).toStrictEqual({ text: "123hello" });
  });

  it("importBatch Error #181", () => {
    const docA = new LoroDoc();
    const updateA = docA.exportSnapshot();
    const docB = new LoroDoc();
    docB.importUpdateBatch([updateA]);
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
  doc2.import(doc1.exportFrom());
  doc2.getText("text").insert(0, "56789");
  doc1.import(doc2.exportFrom());
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
    doc.setChangeMergeInterval(10);
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
  doc.import(doc2.exportSnapshot());
  expect(doc.toJSON()).toStrictEqual({ map: { key: 2 } });
});

describe("export", () => {
  it("test export update", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "123");
    doc.commit();
    const updates = doc.export({ mode: "update", from: new VersionVector(null) });
    const doc2 = new LoroDoc();
    doc2.import(updates);
    expect(doc2.toJSON()).toStrictEqual({ text: "123" });
  })

  it("test export snapshot", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "123");
    doc.commit();
    const snapshot = doc.export({ mode: "snapshot" });
    const doc2 = new LoroDoc();
    doc2.import(snapshot);
    expect(doc2.toJSON()).toStrictEqual({ text: "123" });
  })

  it("test export shallow-snapshot", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "123");
    doc.commit();
    const snapshot = doc.export({ mode: "shallow-snapshot", frontiers: doc.oplogFrontiers() });
    const doc2 = new LoroDoc();
    doc2.import(snapshot);
    expect(doc2.toJSON()).toStrictEqual({ text: "123" });
  })

  it("test export updates-in-range", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.getText("text").insert(0, "123");
    doc.commit();
    const bytes = doc.export({ mode: "updates-in-range", spans: [{ id: { peer: "1", counter: 0 }, len: 1 }] });
    const doc2 = new LoroDoc();
    doc2.import(bytes);
    expect(doc2.toJSON()).toStrictEqual({ text: "1" });
  })
})
it("has correct map value #453", async () => {
  {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "Hello");
    text.mark({ start: 0, end: 2 }, "bold", { b: {} });
    expect(text.toDelta()).toStrictEqual([
      { insert: "He", attributes: { bold: { b: {} } } },
      { insert: "llo" }
    ]);
    let diff: Diff | undefined;
    let expectedDiff: TextDiff = {
      "type": "text",
      "diff": [
        { insert: "He", attributes: { bold: { b: {} } } },
        { insert: "llo" }
      ]
    };
    doc.subscribe(e => {
      console.log("Text", e);
      diff = e.events[0].diff;
    })
    doc.commit();
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(diff).toStrictEqual(expectedDiff);
  }
  {
    const map = new LoroMap();
    map.set('a', { b: {} });
    expect(map.toJSON()).toStrictEqual({ a: { b: {} } });
  }
  {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set('a', { b: {} });
    doc.commit();
    expect(map.toJSON()).toStrictEqual({ a: { b: {} } });
  }
  {
    const doc = new LoroDoc();
    let diff: Diff | undefined;
    const expectedDiff: MapDiff = {
      "type": "map",
      "updated": {
        "a": {
          "b": {}
        }
      }
    };
    doc.subscribe(e => {
      diff = e.events[0].diff;
    })
    const map = doc.getMap("map");
    map.set('a', { b: {} });
    doc.commit();
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(diff).toStrictEqual(expectedDiff);
  }
})

it("can set commit message", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.getText("text").insert(0, "123");
  doc.commit({ message: "Hello world" });
  expect(doc.getChangeAt({ peer: "1", counter: 0 }).message).toBe("Hello world");
})

it("can query pending txn length", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  expect(doc.getPendingTxnLength()).toBe(0);
  doc.getText("text").insert(0, "123");
  expect(doc.getPendingTxnLength()).toBe(3);
  doc.commit();
  expect(doc.getPendingTxnLength()).toBe(0);
})


it("can encode/decode frontiers", () => {
  const frontiers = [{ peer: "1123", counter: 1 }, { peer: "222", counter: 2 }] as Frontiers;
  const encoded = encodeFrontiers(frontiers);
  const decoded = decodeFrontiers(encoded);
  expect(decoded).toStrictEqual(frontiers);
})

it("travel changes", () => {
  let doc = new LoroDoc();
  doc.setPeerId(1);
  doc.getText("text").insert(0, "abc");
  doc.commit();
  let n = 0;
  doc.travelChangeAncestors([{ peer: "1", counter: 0 }], (meta: any) => {
    n += 1;
    return true
  })
  expect(n).toBe(1);
})

it("get path to container", () => {
  const doc = new LoroDoc();
  const map = doc.getMap("map");
  const list = map.setContainer("list", new LoroList());
  const path = doc.getPathToContainer(list.id);
  expect(path).toStrictEqual(["map", "list"])
})

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
  expect(result).toStrictEqual(["1984"])
})
