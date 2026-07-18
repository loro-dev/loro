import { readFileSync } from "node:fs";

import { describe, expect, test } from "vitest";

import {
  ContainerType,
  decodeFastSnapshot,
  decodePostcardVersionVector,
  decodeStateSnapshotStore,
  decodeSstable,
  encodeFastSnapshot,
  encodePostcardVersionVector,
  encodeStateSnapshotStore,
  encodeSstable,
} from "../src/codec";

import {
  Awareness,
  AwarenessWasm,
  Cursor,
  decodeFrontiers,
  decodeImportBlobMeta,
  encodeFrontiers,
  EphemeralStore,
  EphemeralStoreWasm,
  LORO_VERSION,
  LoroCounter,
  LoroDoc,
  LoroList,
  LoroMap,
  LoroMovableList,
  LoroText,
  redactJsonUpdates,
  type LoroEventBatch,
  UndoManager,
  VersionVector,
} from "../src/index";

const fixture = (name: string): Uint8Array =>
  new Uint8Array(readFileSync(new URL(`./fixtures/rust/${name}`, import.meta.url)));

const withDuplicateTextId = (
  snapshot: ReturnType<typeof decodeFastSnapshot>,
): Uint8Array => {
  const stateStore = decodeStateSnapshotStore(snapshot.state);
  if (stateStore.kind !== "sstable") throw new Error("expected snapshot state");
  let changed = false;
  const containers = stateStore.containers.map((entry) => {
    const state = entry.wrapper.state;
    if (changed || state.kind !== ContainerType.Text) return entry;
    const span = state.spans.find(({ length }) => length >= 2);
    if (span === undefined) return entry;
    const peer = state.peers[Number(span.peerIndex)]!;
    const aliasPeerIndex = BigInt(state.peers.length);
    changed = true;
    return {
      ...entry,
      wrapper: {
        ...entry.wrapper,
        state: {
          ...state,
          peers: [...state.peers, peer],
          spans: state.spans.flatMap((item) =>
            item === span
              ? [
                  { ...span, length: 1 },
                  { ...span, peerIndex: aliasPeerIndex, length: 1 },
                  { ...span, counter: span.counter + 2, length: span.length - 2 },
                ].filter(({ length }) => length > 0)
              : [item],
          ),
        },
      },
    };
  });
  if (!changed) throw new Error("expected a splittable text span");
  return encodeFastSnapshot({
    ...snapshot,
    state: encodeStateSnapshotStore(
      { ...stateStore, containers },
      { compression: "none" },
    ),
  });
};

describe("loro-wasm-compatible runtime", () => {
  test("keeps existing container handlers usable after freeing the doc wrapper", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    const list = doc.getList("list");
    const map = doc.getMap("map");

    doc.free();
    text.insert(0, "123");
    list.insert(0, 1);
    map.set("key", 8);

    expect(text.toString()).toBe("123");
    expect(list.toJSON()).toEqual([1]);
    expect(map.toJSON()).toEqual({ key: 8 });
  });

  test("edits map, list, text, movable-list, tree and counter containers", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);

    doc.getMap("map").set("answer", 42);
    const list = doc.getList("list");
    list.push("a");
    list.push("b");
    list.delete(0, 1);
    const text = doc.getText("text");
    text.insert(0, "a😀b");
    text.delete(0, 1);
    const movable = doc.getMovableList("movable");
    movable.push(1);
    movable.push(2);
    movable.move(0, 1);
    movable.set(0, 3);
    const tree = doc.getTree("tree");
    const root = tree.createNode();
    root.data.set("title", "root");
    root.createNode();
    doc.getCounter("counter").increment(2.5);

    expect(doc.toJSON()).toEqual({
      map: { answer: 42 },
      list: ["b"],
      text: "😀b",
      movable: [3, 1],
      tree: [
        expect.objectContaining({
          id: root.id,
          meta: { title: "root" },
          children: [expect.objectContaining({ parent: root.id })],
        }),
      ],
      counter: 2.5,
    });
  });

  test("returns wasm-shaped shallow values and deep values with container ids", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const map = doc.getMap("map");
    map.set("plain", 1);
    const child = map.setContainer("body", new LoroText());
    child.insert(0, "hello");
    const tree = doc.getTree("tree");
    const node = tree.createNode();
    node.data.set("name", "root");

    expect(map.getShallowValue()).toEqual({ plain: 1, body: child.id });
    expect(doc.getShallowValue()).toEqual({ map: map.id, tree: tree.id });
    expect(tree.getShallowValue()).toEqual([
      {
        id: node.id,
        parent: null,
        index: 0,
        fractional_index: "80",
        meta: node.data.id,
        children: [],
      },
    ]);
    expect(tree.toJSON()).toEqual([
      {
        id: node.id,
        parent: null,
        index: 0,
        fractional_index: "80",
        meta: { name: "root" },
        children: [],
      },
    ]);
    expect(doc.getDeepValueWithID()).toMatchObject({
      map: {
        cid: map.id,
        value: {
          body: { cid: child.id, value: "hello" },
          plain: 1,
        },
      },
    });
  });

  test("evaluates JSONPath selectors, recursion, slices, and filters", () => {
    const doc = new LoroDoc();
    const store = doc.getMap("store");
    store.set("books", [
      { title: "1984", author: "George Orwell", price: 10, available: true },
      { title: "Animal Farm", author: "George Orwell", price: 8, available: true },
      { title: "Brave New World", author: "Aldous Huxley", price: 12, available: false },
      { title: "Fahrenheit 451", author: "Ray Bradbury", price: 9, available: true },
      { title: "Pride and Prejudice", author: "Jane Austen", price: 7, available: true },
    ]);
    store.set("featured_author", "George Orwell");
    store.set("featured_authors", ["George Orwell", "Jane Austen"]);
    const body = store.setContainer("body", new LoroText());
    body.insert(0, "content");
    const outline = doc.getTree("outline");
    const firstRoot = outline.createNode();
    firstRoot.data.set("label", "first");
    firstRoot.createNode().data.set("label", "child");
    outline.createNode().data.set("label", "last");

    expect(doc.JSONPath("$['store'].books[0].title")).toEqual(["1984"]);
    expect(doc.JSONPath("$.store.books[*].title")).toEqual([
      "1984",
      "Animal Farm",
      "Brave New World",
      "Fahrenheit 451",
      "Pride and Prejudice",
    ]);
    expect(doc.JSONPath("$..title")).toHaveLength(5);
    expect(doc.JSONPath("$.store.books[0,2,-1].title")).toEqual([
      "1984",
      "Brave New World",
      "Pride and Prejudice",
    ]);
    expect(doc.JSONPath("$.store.books[0:5:2].title")).toEqual([
      "1984",
      "Brave New World",
      "Pride and Prejudice",
    ]);
    expect(
      doc.JSONPath(
        "$.store.books[?(@.author == $.store.featured_author && @.price < 10)].title",
      ),
    ).toEqual(["Animal Farm"]);
    expect(
      doc.JSONPath("$.store.books[?(@.author in $.store.featured_authors)].title"),
    ).toEqual(["1984", "Animal Farm", "Pride and Prejudice"]);
    expect(doc.JSONPath("$..[?(@.title contains 'Farm')].title")).toEqual([
      "Animal Farm",
    ]);
    expect(doc.JSONPath("$.store.books[?(count(@.title) == 1)].title")).toHaveLength(5);
    expect(doc.JSONPath("$.store.body")).toEqual([body]);
    expect(doc.JSONPath("$.outline[0].meta.label")).toEqual(["first"]);
    expect(doc.JSONPath("$.outline[-1].meta.label")).toEqual(["last"]);
    expect(doc.JSONPath("$")).toEqual([{ store, outline }]);
    expect(doc.getByPath("outline/0/0/label")).toBe("child");

    doc.commit();
    let subscriptionHits = 0;
    const unsubscribe = doc.subscribeJsonpath("$.store.featured_author", () => {
      subscriptionHits += 1;
    });
    firstRoot.data.set("unrelated", true);
    doc.commit();
    expect(subscriptionHits).toBe(0);
    store.set("featured_author", "Jane Austen");
    doc.commit();
    expect(subscriptionHits).toBe(1);
    unsubscribe();
    expect(() => doc.subscribeJsonpath("store", () => {})).toThrow(/start with/u);
  });

  test("syncs updates in both directions", () => {
    const left = new LoroDoc();
    left.setPeerId(1);
    left.getList("list").push("A");
    left.getList("list").push("B");
    left.getText("text").insert(0, "hello");

    const right = new LoroDoc();
    right.setPeerId(2);
    right.import(left.export({ mode: "update" }));
    expect(right.toJSON()).toEqual(left.toJSON());

    const from = right.oplogVersion();
    right.getMap("map").set("x", true);
    right.getList("list").delete(0, 1);
    left.import(right.export({ mode: "update", from }));
    expect(left.toJSON()).toEqual(right.toJSON());
  });

  test("slices update exports at operation boundaries", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    source.getText("text").insert(0, "abcde");
    source.commit();

    const prefix = source.export({
      mode: "updates-in-range",
      spans: [{ id: { peer: "1", counter: 0 }, len: 2 }],
    });
    const version = new VersionVector(new Map([["1", 2]]));
    const suffix = source.export({ mode: "update", from: version });

    const target = new LoroDoc();
    target.import(prefix);
    expect(target.getText("text").toString()).toBe("ab");
    target.import(suffix);
    expect(target.getText("text").toString()).toBe("abcde");
  });

  test("converges when updates are imported in opposite orders", () => {
    const base = new LoroDoc();
    base.setPeerId(1);
    base.getList("list").push("base");
    base.getText("text").insert(0, "x");
    const initial = base.export({ mode: "update" });

    const left = new LoroDoc();
    left.setPeerId(2);
    left.import(initial);
    const right = new LoroDoc();
    right.setPeerId(3);
    right.import(initial);
    const version = left.oplogVersion();

    left.getList("list").insert(0, "left");
    left.getText("text").insert(0, "L");
    left.getMap("map").set("winner", "left");
    const leftUpdate = left.export({ mode: "update", from: version });

    right.getList("list").insert(0, "right");
    right.getText("text").insert(0, "R");
    right.getMap("map").set("winner", "right");
    const rightUpdate = right.export({ mode: "update", from: version });

    left.import(rightUpdate);
    right.import(leftUpdate);
    expect(left.toJSON()).toEqual(right.toJSON());
  });

  test("keeps concurrent backward insertions in Fugue order", () => {
    const left = new LoroDoc();
    left.setPeerId(1);
    for (const character of ["o", "l", "l", "e", "H"]) {
      left.getText("text").insert(0, character);
    }

    const right = new LoroDoc();
    right.setPeerId(2);
    for (const character of ["!", "d", "l", "r", "o", "W", " "]) {
      right.getText("text").insert(0, character);
    }

    const leftUpdate = left.export({ mode: "update" });
    const rightUpdate = right.export({ mode: "update" });
    left.import(rightUpdate);
    right.import(leftUpdate);

    expect(left.getText("text").toString()).toBe("Hello World!");
    expect(right.getText("text").toString()).toBe("Hello World!");
  });

  test("does not interleave a concurrent insertion into an existing Fugue run", () => {
    const first = new LoroDoc();
    first.setPeerId(1);
    const concurrent = new LoroDoc();
    concurrent.setPeerId(2);
    const seed = new LoroDoc();
    seed.setPeerId(3);
    seed.getText("text").insert(0, "2");

    first.import(seed.export({ mode: "update" }));
    first.getText("text").insert(0, "1");
    concurrent.getText("text").insert(0, "b");
    first.import(concurrent.export({ mode: "update" }));

    expect(first.getText("text").toString()).toBe("b12");
  });

  test("checks out partial changes and attaches back to the latest state", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    text.insert(0, "abc");
    doc.commit();

    doc.checkout([{ peer: "1", counter: 1 }]);
    expect(text.toString()).toBe("ab");
    expect(doc.isDetached()).toBe(true);
    expect(doc.version().toJSON()).toEqual(new Map([["1", 2]]));
    expect(doc.oplogVersion().toJSON()).toEqual(new Map([["1", 3]]));
    expect(() => text.push("x")).toThrow(/detached document/u);

    doc.attach();
    expect(text.toString()).toBe("abc");
    expect(doc.isDetached()).toBe(false);
  });

  test("records imports in the oplog while the state is detached", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    source.getText("text").insert(0, "base");
    const base = source.export({ mode: "update" });

    const doc = new LoroDoc();
    doc.setPeerId(2);
    doc.import(base);
    const checkedOut = doc.frontiers();
    doc.checkout(checkedOut);

    source.getText("text").push(" latest");
    doc.import(source.export({ mode: "update", from: doc.oplogVersion() }));
    expect(doc.getText("text").toString()).toBe("base");
    expect(doc.oplogVersion().compare(doc.version())).toBe(1);

    doc.checkoutToLatest();
    expect(doc.getText("text").toString()).toBe("base latest");
  });

  test("holds causally incomplete updates pending until dependencies arrive", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    source.getList("list").push("first");
    source.commit();
    const firstVersion = source.oplogVersion();
    const firstUpdate = source.export({ mode: "update" });
    source.getList("list").push("second");
    source.commit();
    const secondUpdate = source.export({ mode: "update", from: firstVersion });

    const target = new LoroDoc();
    const pending = target.import(secondUpdate);
    expect(target.getList("list").toArray()).toEqual([]);
    expect(pending.success.size).toBe(0);
    expect(pending.pending).toEqual(new Map([["1", { start: 1, end: 2 }]]));

    const applied = target.import(firstUpdate);
    expect(target.getList("list").toArray()).toEqual(["first", "second"]);
    expect(applied.success).toEqual(new Map([["1", { start: 0, end: 2 }]]));
    expect(applied.pending).toBeNull();
  });

  test("imports the unseen suffix of a change that overlaps local history", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    source.getText("text").insert(0, "a");
    source.commit();
    const prefix = source.export({ mode: "update" });

    source.getText("text").insert(1, "b");
    source.commit();
    expect(source.changeCount()).toBe(1);

    const target = new LoroDoc();
    target.import(prefix);
    const status = target.import(source.export({ mode: "snapshot" }));

    expect(target.getText("text").toString()).toBe("ab");
    expect(status.success).toEqual(new Map([["1", { start: 1, end: 2 }]]));
    expect(status.pending).toBeNull();
  });

  test("forkAt keeps only history reachable from the requested frontiers", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.getList("list").push("first");
    doc.commit();
    const first = doc.frontiers();
    doc.getList("list").push("second");
    doc.commit();

    const fork = doc.forkAt(first);
    expect(fork.getList("list").toArray()).toEqual(["first"]);
    expect(fork.opCount()).toBe(1);
  });

  test("queries version spans, ancestors and changed containers", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.getText("text").insert(0, "Hello");
    doc.commit({ message: "first" });
    const first = doc.frontiers();
    doc.getText("text").push(" World");
    doc.getMap("map").set("ready", true);
    doc.commit({ message: "second" });
    const second = doc.frontiers();

    expect(doc.findIdSpansBetween(first, second)).toEqual({
      retreat: [],
      forward: [{ peer: "1", counter: 5, length: 7 }],
    });
    expect(doc.findIdSpansBetween(second, first)).toEqual({
      retreat: [{ peer: "1", counter: 5, length: 7 }],
      forward: [],
    });
    expect(new Set(doc.getChangedContainersIn({ peer: "1", counter: 5 }, 7))).toEqual(
      new Set([doc.getText("text").id, doc.getMap("map").id]),
    );

    const messages: (string | undefined)[] = [];
    doc.travelChangeAncestors(second, (change) => {
      messages.push(change.message);
    });
    expect(messages).toEqual(["first", "second"]);
  });

  test("emits compatible text, list and map diffs", () => {
    const doc = new LoroDoc();
    let batch: LoroEventBatch | undefined;
    doc.subscribe((event) => (batch = event));

    const text = doc.getText("text");
    text.insert(0, "3");
    doc.commit();
    expect(batch?.events[0]?.diff).toEqual({ type: "text", diff: [{ insert: "3" }] });
    text.insert(1, "12");
    doc.commit();
    expect(batch?.events[0]?.diff).toEqual({
      type: "text",
      diff: [{ retain: 1 }, { insert: "12" }],
    });

    const list = doc.getList("list");
    list.insert(0, "3");
    doc.commit();
    expect(batch?.events[0]?.diff).toEqual({ type: "list", diff: [{ insert: ["3"] }] });
    list.insert(1, "12");
    doc.commit();
    expect(batch?.events[0]?.diff).toEqual({
      type: "list",
      diff: [{ retain: 1 }, { insert: ["12"] }],
    });

    const map = doc.getMap("map");
    map.set("a", 1);
    map.set("b", 2);
    doc.commit();
    expect(batch?.events[0]?.diff).toEqual({
      type: "map",
      updated: { a: 1, b: 2 },
    });
  });

  test("calculates raw and JSON-compatible diffs without moving document state", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    const child = map.setContainer("child", new LoroMap());
    child.set("value", 1);
    doc.getList("list").push("item");
    doc.commit();
    const latest = doc.frontiers();

    const raw = doc.diff([], latest, false);
    const rawMap = raw.find(([id]) => id === map.id)?.[1];
    expect(rawMap).toEqual({ type: "map", updated: { child } });

    const json = doc.diff([], latest);
    const jsonMap = json.find(([id]) => id === map.id)?.[1];
    expect(jsonMap).toEqual({
      type: "map",
      updated: { child: `🦜:${child.id}` },
    });
    expect(doc.toJSON()).toEqual({ map: { child: { value: 1 } }, list: ["item"] });
  });

  test("keeps map tombstones that are visible only in operation history", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("created-then-deleted", 1);
    doc.commit();
    map.delete("created-then-deleted");
    doc.commit();

    expect(doc.diff([], doc.frontiers(), false)).toEqual([
      [map.id, { type: "map", updated: { "created-then-deleted": undefined } }],
    ]);
  });

  test("applies raw and serialized diffs with fresh child and tree ids", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const map = source.getMap("map");
    const child = map.setContainer("child", new LoroText());
    child.insert(0, "Hello🙂");
    child.mark({ start: 0, end: 5 }, "bold", true);
    map.ensureMergeableCounter("revision").increment(3);
    const listChild = source.getList("list").pushContainer(new LoroMap());
    listChild.set("nested", true);
    const treeNode = source.getTree("tree").createNode();
    treeNode.data.set("name", "root");
    source.commit();

    const raw = source.diff([], source.frontiers(), false);
    const serialized = JSON.parse(
      JSON.stringify(source.diff([], source.frontiers())),
    ) as typeof raw;

    for (const batch of [raw, serialized]) {
      const target = new LoroDoc();
      target.setPeerId(2);
      target.applyDiff(batch);

      const targetChild = target.getMap("map").get("child") as LoroText;
      expect(targetChild.id).not.toBe(child.id);
      expect(targetChild.toDelta()).toEqual(child.toDelta());
      expect(target.toJSON()).toMatchObject({
        map: { child: "Hello🙂", revision: 3 },
        list: [{ nested: true }],
      });
      const targetRoot = target.getTree("tree").roots()[0];
      expect(targetRoot?.id).not.toBe(treeNode.id);
      expect(targetRoot?.data.toJSON()).toEqual({ name: "root" });
    }
  });

  test("reverts by generating operations, including recreated child containers", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getList("list");
    list.push("item1");
    list.push("item2");
    const child = list.pushContainer(new LoroText());
    child.insert(0, "Hello🙂");
    doc.getText("title").insert(0, "A🙂B");
    doc.commit();
    const populated = doc.frontiers();

    child.delete(0, child.length);
    list.clear();
    doc.getText("title").update("changed");
    doc.commit();

    doc.revertTo(populated);
    expect(doc.toJSON()).toMatchObject({
      list: ["item1", "item2", "Hello🙂"],
      title: "A🙂B",
    });
    expect((list.get(2) as LoroText).id).not.toBe(child.id);

    doc.commit();
    doc.revertTo([]);
    expect(doc.toJSON()).toEqual({ list: [], title: "" });
    expect(doc.isDetached()).toBe(false);
  });

  test("undoes and redoes local commit groups", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const pushed: [boolean, number, number][] = [];
    const popped: boolean[] = [];
    const undo = new UndoManager(doc, {
      mergeInterval: 0,
      onPush: (isUndo, range) => {
        pushed.push([isUndo, range.start, range.end]);
        return { value: range.end, cursors: [] };
      },
      onPop: (isUndo) => popped.push(isUndo),
    });

    doc.getText("text").insert(0, "hello");
    doc.commit();
    doc.getText("text").insert(5, " world");
    doc.commit();
    expect(undo.canUndo()).toBe(true);
    expect(undo.undo()).toBe(true);
    expect(doc.toJSON()).toEqual({ text: "hello" });
    expect(undo.undo()).toBe(true);
    expect(doc.toJSON()).toEqual({ text: "" });
    expect(undo.canUndo()).toBe(false);

    expect(undo.redo()).toBe(true);
    expect(doc.toJSON()).toEqual({ text: "hello" });
    expect(undo.redo()).toBe(true);
    expect(doc.toJSON()).toEqual({ text: "hello world" });
    expect(undo.canRedo()).toBe(false);
    expect(pushed).toHaveLength(6);
    expect(popped).toEqual([true, true, false, false]);
  });

  test("trims undo history from the front", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    const undo = new UndoManager(doc, { mergeInterval: 0, maxUndoSteps: 2 });
    for (const value of ["a", "b", "c"]) {
      text.push(value);
      doc.commit();
    }

    expect(undo.undo()).toBe(true);
    expect(undo.undo()).toBe(true);
    expect(undo.undo()).toBe(false);
    expect(text.toString()).toBe("a");
  });

  test("undo preserves remote and excluded text edits", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    const undo = new UndoManager(doc, {
      mergeInterval: 0,
      excludeOriginPrefixes: ["sys:"],
    });
    text.insert(0, "hello");
    doc.commit();
    text.insert(0, "1");
    doc.commit({ origin: "sys:test" });
    text.insert(2, "2");
    doc.commit({ origin: "sys:test" });
    text.insert(4, "3");
    doc.commit({ origin: "sys:test" });

    expect(undo.undo()).toBe(true);
    expect(text.toString()).toBe("123");
    expect(undo.redo()).toBe(true);
    expect(text.toString()).toBe("1h2e3llo");

    const remote = new LoroDoc();
    remote.setPeerId(2);
    remote.import(doc.export({ mode: "snapshot" }));
    remote.getText("text").insert(0, "R");
    remote.commit();
    doc.import(remote.export({ mode: "update" }));

    expect(undo.undo()).toBe(true);
    expect(text.toString()).toBe("R123");
  });

  test("undo follows the local peer after detached checkout", () => {
    const doc = new LoroDoc();
    const undo = new UndoManager(doc, { mergeInterval: 0 });
    doc.setPeerId(1);
    const text = doc.getText("text");
    text.insert(0, "Hello");
    doc.commit();
    text.insert(5, " world!");
    doc.commit();
    doc.setDetachedEditing(true);
    doc.checkout([{ peer: "1", counter: 4 }]);

    expect(undo.canUndo()).toBe(false);
    text.insert(5, " alice!");
    doc.commit();
    expect(undo.undo()).toBe(true);
    expect(text.toString()).toBe("Hello");
  });

  test("round-trips compressed and uncompressed JSON updates", () => {
    const source = new LoroDoc();
    source.setPeerId(19);
    source.getText("text").insert(0, "Hello🙂");
    const map = source.getMap("map");
    const child = map.setContainer("child", new LoroMap());
    child.set("value", { nested: [1, true, null] });
    source.getList("list").push("item");
    const movable = source.getMovableList("movable");
    movable.push("first");
    movable.push("second");
    movable.move(1, 0);
    movable.set(1, "edited");
    const node = source.getTree("tree").createNode();
    node.data.set("name", "root");
    source.getCounter("counter").increment(2.5);
    source.commit();

    const compressed = source.exportJsonUpdates();
    expect(compressed.peers).toEqual(["19"]);
    expect(compressed.changes[0]?.id).toBe("0@0");
    expect(source.getOpsInChange({ peer: "19", counter: 0 })).toEqual(
      source.exportJsonInIdSpan({
        peer: "19",
        counter: 0,
        length: source.opCount(),
      })[0]?.ops,
    );

    const uncompressed = source.exportJsonUpdates(undefined, undefined, false);
    expect(uncompressed.peers).toBeNull();
    expect(uncompressed.changes[0]?.id).toBe("0@19");

    for (const update of [compressed, JSON.stringify(compressed), uncompressed]) {
      const target = new LoroDoc();
      target.importJsonUpdates(update);
      expect(target.toJSON()).toEqual(source.toJSON());
      expect(target.export({ mode: "update" })).toBeInstanceOf(Uint8Array);
    }
  });

  test("redacts JSON update content while preserving child-container structure", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    source.getText("text").insert(0, "secret");
    source.getMap("map").set("token", "private");
    const child = source.getMap("map").setContainer("child", new LoroMap());
    child.set("inside", 42);
    source.getList("list").push("hidden");
    source.commit();

    const redacted = redactJsonUpdates(source.exportJsonUpdates(), {
      "1": [0, source.opCount()],
    });
    const target = new LoroDoc();
    target.importJsonUpdates(redacted);
    expect(target.toJSON()).toEqual({
      text: "������",
      map: { token: null, child: { inside: null } },
      list: [null],
    });
  });

  test("exports pending operations as JSON without committing", () => {
    const doc = new LoroDoc();
    doc.setPeerId(7);
    doc.getText("text").insert(0, "pending");

    const pending = doc.getUncommittedOpsAsJson();
    expect(pending).toMatchObject({ peers: null });
    expect(pending?.changes[0]).toMatchObject({ id: "0@7", ops: [{ counter: 0 }] });
    expect(doc.changeCount()).toBe(0);

    const target = new LoroDoc();
    target.importJsonUpdates(pending!);
    expect(target.toJSON()).toEqual({ text: "pending" });
  });

  test("decodes update, snapshot, and shallow-snapshot blob metadata", () => {
    const doc = new LoroDoc();
    doc.setPeerId(5);
    doc.setChangeMergeInterval(9);
    doc.getText("text").insert(0, "abc");
    doc.commit({ timestamp: 10 });
    const first = doc.version();
    const firstFrontiers = doc.frontiers();
    doc.getMap("map").set("value", 1);
    doc.commit({ timestamp: 20 });

    const update = decodeImportBlobMeta(doc.export({ mode: "update", from: first }));
    expect(update.mode).toBe("update");
    expect(update.partialStartVersionVector.get("5")).toBe(3);
    expect(update.partialEndVersionVector.get("5")).toBe(4);
    expect(update.startTimestamp).toBe(20);
    expect(update.endTimestamp).toBe(20);
    expect(update.changeNum).toBe(1);

    const snapshot = decodeImportBlobMeta(doc.export({ mode: "snapshot" }));
    expect(snapshot).toMatchObject({
      mode: "snapshot",
      startTimestamp: 0,
      endTimestamp: 20,
      changeNum: 2,
    });
    expect(snapshot.partialEndVersionVector.get("5")).toBe(4);

    const shallow = decodeImportBlobMeta(
      doc.export({ mode: "shallow-snapshot", frontiers: firstFrontiers }),
    );
    expect(shallow.mode).toBe("shallow-snapshot");
    expect(shallow.startFrontiers).toEqual(firstFrontiers);
    expect(shallow.partialStartVersionVector.get("5")).toBe(2);
    expect(shallow.partialEndVersionVector.get("5")).toBe(4);
  });

  test("merges continuous changes using the wasm change interval", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const updates: Uint8Array[] = [];
    source.subscribeLocalUpdates((update) => updates.push(update));

    source.getMap("map").set("first", { shared: 1, left: true });
    source.commit({ timestamp: 110 });
    source.getMap("map").set("second", { right: 2, shared: 3 });
    source.commit({ timestamp: 120 });

    expect(source.changeCount()).toBe(1);
    expect(source.getAllChanges().get("1")).toEqual([
      expect.objectContaining({ counter: 0, length: 2, timestamp: 110 }),
    ]);
    expect(source.exportJsonUpdates().changes).toHaveLength(1);
    expect(() => source.debugHistory()).not.toThrow();

    const target = new LoroDoc();
    for (const update of updates) target.import(update);
    expect(target.changeCount()).toBe(1);
    expect(target.toJSON()).toEqual(source.toJSON());

    const snapshotTarget = LoroDoc.fromSnapshot(source.export({ mode: "snapshot" }));
    expect(snapshotTarget.toJSON()).toEqual({
      map: {
        first: { shared: 1, left: true },
        second: { right: 2, shared: 3 },
      },
    });

    const unmerged = new LoroDoc();
    unmerged.setPeerId(1);
    unmerged.setChangeMergeInterval(9);
    unmerged.getText("text").insert(0, "a");
    unmerged.commit({ timestamp: 110 });
    unmerged.getText("text").insert(1, "b");
    unmerged.commit({ timestamp: 120 });
    expect(unmerged.getAllChanges().get("1")).toHaveLength(2);
  });

  test("appends many merged commits without changing their public history", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const updates: Uint8Array[] = [];
    source.subscribeLocalUpdates((update) => updates.push(update));
    const text = source.getText("text");

    for (let index = 0; index < 512; index += 1) {
      text.insert(index, "x");
      source.commit();
    }

    expect(source.changeCount()).toBe(1);
    expect(source.getAllChanges().get("1")).toEqual([
      expect.objectContaining({ counter: 0, length: 512 }),
    ]);

    const target = new LoroDoc();
    target.importBatch(updates);
    expect(target.getText("text").toString()).toBe("x".repeat(512));
    expect(target.changeCount()).toBe(1);
  });

  test("coalesces consecutive list inserts in one pending change", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const list = source.getList("list");
    for (let index = 0; index < 128; index += 1) list.insert(index, index);

    expect(source.getUncommittedOpsAsJson()?.changes[0]?.ops).toHaveLength(1);
    source.commit();
    expect(source.exportJsonUpdates().changes[0]?.ops).toHaveLength(1);

    const target = new LoroDoc();
    target.import(source.export({ mode: "update" }));
    expect(target.getList("list").toArray()).toEqual(
      Array.from({ length: 128 }, (_, index) => index),
    );
  });

  test("preserves next commit options across implicit empty commits only", () => {
    const implicit = new LoroDoc();
    implicit.setPeerId(1);
    implicit.setNextCommitMessage("kept");
    implicit.export({ mode: "update" });
    implicit.getText("text").insert(0, "a");
    implicit.commit();
    expect(implicit.getAllChanges().get("1")?.[0]?.message).toBe("kept");

    const explicit = new LoroDoc();
    explicit.setPeerId(2);
    explicit.setNextCommitMessage("discarded");
    explicit.commit();
    explicit.getText("text").insert(0, "b");
    explicit.commit();
    expect(explicit.getAllChanges().get("2")?.[0]?.message).toBeUndefined();
  });

  test("groups all changes by peer like the wasm Map API", () => {
    const first = new LoroDoc();
    first.setPeerId(1);
    first.getText("text").insert(0, "a");
    first.commit();

    const second = new LoroDoc();
    second.setPeerId(2);
    second.getText("text").insert(0, "b");
    second.commit();

    const target = new LoroDoc();
    target.import(first.export({ mode: "update" }));
    target.import(second.export({ mode: "update" }));
    expect([...target.getAllChanges().keys()].sort()).toEqual(["1", "2"]);
  });

  test("gets the latest peer change at or before a lamport", () => {
    const first = new LoroDoc();
    first.setPeerId(1);
    const second = new LoroDoc();
    second.setPeerId(2);

    first.getText("text").insert(0, "01234");
    second.import(first.export({ mode: "update" }));
    second.getText("text").insert(0, "56789");
    first.import(second.export({ mode: "update" }));
    first.getText("text").insert(0, "01234");
    first.commit();

    expect(first.getChangeAtLamport("1", 1)).toMatchObject({
      peer: "1",
      lamport: 0,
      length: 5,
    });
    expect(first.getChangeAtLamport("1", 7)?.lamport).toBe(0);
    expect(first.getChangeAtLamport("1", 10)?.lamport).toBe(10);
    expect(first.getChangeAtLamport("1", 20)?.lamport).toBe(10);
    expect(first.getChangeAtLamport("111", 20)).toBeUndefined();
    expect(() => first.getChangeAt({ peer: "1", counter: 99 })).toThrow(/unknown/u);
    expect(() => first.getOpsInChange({ peer: "1", counter: 99 })).toThrow(/unknown/u);
  });

  test("emits absolute paths and bubbles descendant events to container subscribers", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    const seen: LoroEventBatch[] = [];
    map.subscribe((event) => seen.push(event));

    const subMap = map.setContainer("sub", new LoroMap());
    subMap.set("value", 1);
    doc.commit();
    expect(seen.at(-1)?.events.find((event) => event.target === subMap.id)?.path).toEqual(
      ["map", "sub"],
    );

    const list = subMap.setContainer("list", new LoroList());
    list.insert(0, "item");
    const text = list.insertContainer(1, new LoroText());
    doc.commit();
    text.insert(0, "nested");
    doc.commit();

    const textEvent = seen.at(-1)?.events.find((event) => event.target === text.id);
    expect(textEvent?.path).toEqual(["map", "sub", "list", 1]);
    expect(seen.at(-1)?.currentTarget).toBe(map.id);
    expect(doc.getPathToContainer(text.id)).toEqual(["map", "sub", "list", 1]);
    text._parentLink = { container: list };
    expect(doc.getPathToContainer(text.id)).toEqual(["map", "sub", "list", 1]);
    expect(text._parentLink.binding?.kind).toBe("sequence");
    expect(doc.getByPath("map/sub")).toBe(subMap);
    expect(doc.getByPath("map/sub/list/1")).toBe(text);
  });

  test("merges first-peer callbacks into the commit and supports ChangeModifier", () => {
    const doc = new LoroDoc();
    doc.setPeerId(9);
    doc.subscribeFirstCommitFromPeer(({ peer }) => {
      doc.getMap("peers").set(peer, `user-${peer}`);
    });
    doc.subscribePreCommit(({ changeMeta, modifier }) => {
      expect(changeMeta.length).toBe(2);
      modifier.setMessage("signed").setTimestamp(123);
    });

    doc.getList("list").push("value");
    doc.commit();

    expect(doc.changeCount()).toBe(1);
    expect(doc.opCount()).toBe(2);
    expect(doc.getMap("peers").get("9")).toBe("user-9");
    expect(doc.getAllChanges().get("9")?.[0]).toMatchObject({
      message: "signed",
      timestamp: 123,
    });
  });

  test("round trips a TypeScript snapshot", () => {
    const source = new LoroDoc();
    source.setPeerId(7);
    source.getMap("map").set("nested", { a: [1, true, "x"] });
    source.getText("text").insert(0, "snapshot");
    source.getCounter("counter").increment(3);
    source.commit({ message: "snapshot" });

    const target = LoroDoc.fromSnapshot(source.export({ mode: "snapshot" }));
    expect(target.toJSON()).toEqual(source.toJSON());
    expect(target.getAllChanges()).toEqual(source.getAllChanges());
  });

  test("coalesces consecutive text IDs without splitting Unicode scalars", () => {
    const source = new LoroDoc();
    source.setPeerId(7);
    const text = source.getText("text");
    const value = `${"a".repeat(31)}😀b`;
    text.insert(0, `${"a".repeat(31)}😀`);
    text.push("b");
    source.commit();

    const snapshot = decodeFastSnapshot(source.export({ mode: "snapshot" }));
    const stateStore = decodeStateSnapshotStore(snapshot.state);
    expect(stateStore.kind).toBe("sstable");
    if (stateStore.kind !== "sstable") throw new Error("expected snapshot state");
    const textState = stateStore.containers.find(
      ({ wrapper }) => wrapper.state.kind === ContainerType.Text,
    )?.wrapper.state;
    expect(textState?.kind).toBe(ContainerType.Text);
    if (textState?.kind !== ContainerType.Text) throw new Error("expected text state");
    expect(textState.text).toBe(value);
    expect(textState.spans.filter(({ length }) => length > 0)).toEqual([
      { peerIndex: 0n, counter: 0, lamportSub: 0, length: 33 },
    ]);

    const target = LoroDoc.fromSnapshot(encodeFastSnapshot(snapshot));
    expect(target.getText("text").toString()).toBe(value);
  });

  test("rejects duplicate text IDs before changing the document", () => {
    const source = new LoroDoc();
    source.setPeerId(8);
    source.getText("text").insert(0, "x".repeat(64));
    source.commit();
    const malformed = withDuplicateTextId(
      decodeFastSnapshot(source.export({ mode: "snapshot" })),
    );

    const target = new LoroDoc();
    const events: LoroEventBatch[] = [];
    target.subscribe((event) => events.push(event));
    expect(() => target.import(malformed)).toThrow(/duplicate sequence IDs/u);
    expect(events).toEqual([]);
    expect(target.version().toJSON()).toEqual(new Map());
    expect(target.toJSON()).toEqual({});
  });

  test("rejects duplicate text IDs before installing shallow metadata", () => {
    const source = new LoroDoc();
    source.setPeerId(9);
    source.getText("text").insert(0, "x".repeat(64));
    source.commit();
    const shallowRoot = source.frontiers();
    source.getMap("meta").set("ready", true);
    source.commit();
    const malformed = withDuplicateTextId(
      decodeFastSnapshot(
        source.export({ mode: "shallow-snapshot", frontiers: shallowRoot }),
      ),
    );

    const target = new LoroDoc();
    const events: LoroEventBatch[] = [];
    target.subscribe((event) => events.push(event));
    expect(() => target.import(malformed)).toThrow(/duplicate sequence IDs/u);
    expect(events).toEqual([]);
    expect(target.isShallow()).toBe(false);
    expect(target.version().toJSON()).toEqual(new Map());
    expect(target.toJSON()).toEqual({});

    const concurrent = new LoroDoc();
    concurrent.setPeerId(10);
    concurrent.getMap("concurrent").set("value", true);
    const batchTarget = new LoroDoc();
    const batchEvents: LoroEventBatch[] = [];
    batchTarget.subscribe((event) => batchEvents.push(event));
    expect(() =>
      batchTarget.importBatch([malformed, concurrent.export({ mode: "update" })]),
    ).toThrow(/duplicate sequence IDs/u);
    expect(batchEvents).toEqual([]);
    expect(batchTarget.isShallow()).toBe(false);
    expect(batchTarget.version().toJSON()).toEqual(new Map());
    expect(batchTarget.toJSON()).toEqual({});
  });

  test("keeps snapshot history lazy and owns the deferred bytes", () => {
    const source = new LoroDoc();
    source.setPeerId(7);
    source.getText("text").insert(0, "snapshot");
    source.getMap("meta").set("ready", true);
    source.commit({ message: "seed" });
    const expectedVersion = source.version().toJSON();
    const expectedFrontiers = source.frontiers();
    const expectedChanges = source.getAllChanges();
    const snapshot = source.export({ mode: "snapshot" });

    const target = new LoroDoc();
    const events: LoroEventBatch[] = [];
    target.subscribe((event) => events.push(event));
    const status = target.import(snapshot);

    expect(target.toJSON()).toEqual(source.toJSON());
    expect(target.version().toJSON()).toEqual(expectedVersion);
    expect(target.frontiers()).toEqual(expectedFrontiers);
    expect(target.opCount()).toBe(source.opCount());
    expect(status).toEqual({
      success: new Map([["7", { start: 0, end: 9 }]]),
      pending: null,
    });
    expect(events).toHaveLength(1);
    expect(events[0]).toMatchObject({ by: "import", from: [], to: expectedFrontiers });

    snapshot.fill(0);
    expect(target.getAllChanges()).toEqual(expectedChanges);
    expect(target.changeCount()).toBe(source.changeCount());
  });

  test("materializes a snapshot before local edits without repeating first-peer events", () => {
    const source = new LoroDoc();
    source.setPeerId(7);
    source.getText("text").insert(0, "a");
    source.commit();

    const target = new LoroDoc();
    target.setPeerId(7);
    target.import(source.export({ mode: "snapshot" }));
    const firstPeers: string[] = [];
    target.subscribeFirstCommitFromPeer(({ peer }) => firstPeers.push(peer));

    target.getText("text").push("b");
    target.commit();

    expect(firstPeers).toEqual([]);
    expect(target.getText("text").toString()).toBe("ab");
    expect(target.version().get("7")).toBe(2);
  });

  test("materializes a lazy snapshot before importing a later update", () => {
    const source = new LoroDoc();
    source.setPeerId(9);
    source.getText("text").insert(0, "a");
    source.commit();
    const snapshotVersion = source.version();
    const snapshot = source.export({ mode: "snapshot" });
    source.getText("text").push("b");
    source.commit();
    const update = source.export({ mode: "update", from: snapshotVersion });

    const target = LoroDoc.fromSnapshot(snapshot);
    const status = target.import(update);

    expect(status).toEqual({
      success: new Map([["9", { start: 1, end: 2 }]]),
      pending: null,
    });
    expect(target.toJSON()).toEqual({ text: "ab" });
    expect(target.getAllChanges()).toEqual(source.getAllChanges());
  });

  test("keeps deferred history atomic when snapshot metadata is inconsistent", () => {
    const source = new LoroDoc();
    source.setPeerId(11);
    source.getText("text").insert(0, "seed");
    source.commit();
    const snapshot = decodeFastSnapshot(source.export({ mode: "snapshot" }));
    const oplog = decodeSstable(snapshot.oplog).map((entry) => {
      if (entry.key.length !== 2 || entry.key[0] !== 0x76 || entry.key[1] !== 0x76) {
        return entry;
      }
      const version = decodePostcardVersionVector(entry.value).map((id) => ({
        ...id,
        counter: id.counter + 1,
      }));
      return { ...entry, value: encodePostcardVersionVector(version) };
    });
    const malformed = encodeFastSnapshot({
      ...snapshot,
      oplog: encodeSstable(oplog, { compression: "none" }),
    });

    const target = new LoroDoc();
    target.import(malformed);
    expect(target.toJSON()).toEqual({ text: "seed" });
    expect(() => target.changeCount()).toThrow(/version does not match/u);
    expect(target.toJSON()).toEqual({ text: "seed" });
    expect(() => target.changeCount()).toThrow(/version does not match/u);
  });

  test("rejects a corrupt deferred frontier block before changing the document", () => {
    const source = new LoroDoc();
    source.setPeerId(12);
    source.getText("text").insert(0, "seed");
    source.commit();
    const snapshot = decodeFastSnapshot(source.export({ mode: "snapshot" }));
    const oplog = decodeSstable(snapshot.oplog).map((entry) =>
      entry.key.length === 12 ? { ...entry, value: Uint8Array.of(0xff) } : entry,
    );
    const malformed = encodeFastSnapshot({
      ...snapshot,
      oplog: encodeSstable(oplog, { compression: "none" }),
    });

    const target = new LoroDoc();
    const events: LoroEventBatch[] = [];
    target.subscribe((event) => events.push(event));
    expect(() => target.import(malformed)).toThrow(/unexpected end|change block/u);
    expect(events).toEqual([]);
    expect(target.version().toJSON()).toEqual(new Map());
    expect(target.toJSON()).toEqual({});
  });

  test("owns shallow root bytes until deferred history is materialized", () => {
    const source = new LoroDoc();
    source.setPeerId(13);
    const original = new Uint8Array(64);
    let random = 0x1234_5678;
    for (let index = 0; index < original.length; index += 1) {
      random ^= random << 13;
      random ^= random >>> 17;
      random ^= random << 5;
      original[index] = random & 0xff;
    }
    source.getMap("map").set("bytes", original);
    source.commit();
    const shallowRoot = source.frontiers();
    source.getMap("map").set("bytes", Uint8Array.of(9));
    source.commit();
    const bytes = source.export({
      mode: "shallow-snapshot",
      frontiers: shallowRoot,
    });

    const target = LoroDoc.fromSnapshot(bytes);
    expect(target.getMap("map").get("bytes")).toEqual(Uint8Array.of(9));
    bytes.fill(0);
    target.checkout(shallowRoot);
    expect(target.getMap("map").get("bytes")).toEqual(original);
  });

  test("does not install shallow metadata before latest state decoding succeeds", () => {
    const source = new LoroDoc();
    source.setPeerId(14);
    source.getText("text").insert(0, "a");
    source.commit();
    const shallowRoot = source.frontiers();
    source.getText("text").push("b");
    source.commit();
    const snapshot = decodeFastSnapshot(
      source.export({ mode: "shallow-snapshot", frontiers: shallowRoot }),
    );
    const malformed = encodeFastSnapshot({
      ...snapshot,
      state: Uint8Array.of(0xff),
    });

    const target = new LoroDoc();
    expect(() => target.import(malformed)).toThrow(/SSTable is too short/u);
    expect(target.isShallow()).toBe(false);
    expect(target.version().toJSON()).toEqual(new Map());
    expect(target.toJSON()).toEqual({});
  });

  test("exports and imports shallow snapshots at a partial change boundary", () => {
    const source = new LoroDoc();
    source.setPeerId(3);
    source.getText("text").insert(0, "01234");
    source.commit();
    const shallowRoot = source.frontiers();
    source.getText("text").push("56789");
    source.getMap("meta").set("ready", true);
    source.commit();

    const snapshot = source.export({
      mode: "shallow-snapshot",
      frontiers: shallowRoot,
    });
    const target = LoroDoc.fromSnapshot(snapshot);

    expect(target.toJSON()).toEqual(source.toJSON());
    expect(target.isShallow()).toBe(true);
    expect(target.shallowSinceFrontiers()).toEqual(shallowRoot);
    expect(target.shallowSinceVV().toJSON()).toEqual(new Map([["3", 4]]));
    expect(() => target.checkout([])).toThrow(/before the shallow history root/u);

    target.checkout(shallowRoot);
    expect(target.toJSON()).toEqual({ text: "01234", meta: {} });
    target.checkoutToLatest();
    expect(target.toJSON()).toEqual(source.toJSON());

    const resnapshot = target.export({ mode: "snapshot" });
    const restored = LoroDoc.fromSnapshot(resnapshot);
    expect(restored.isShallow()).toBe(true);
    expect(restored.toJSON()).toEqual(source.toJSON());
  });

  test("preserves rich-text attributes in snapshots", () => {
    const source = new LoroDoc();
    source.setPeerId(7);
    const text = source.getText("text");
    text.insert(0, "a😀bc");
    text.mark({ start: 1, end: 3 }, "bold", true);
    source.commit();

    const target = LoroDoc.fromSnapshot(source.export({ mode: "snapshot" }));
    expect(target.getText("text").toDelta()).toEqual([
      { insert: "a" },
      { insert: "😀", attributes: { bold: true } },
      { insert: "bc" },
    ]);
  });

  test("uses compatible text-style expansion rules", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "hi");
    text.mark({ start: 0, end: 2 }, "bold", true);
    text.insert(2, "!");
    text.insert(0, "(");

    expect(text.toDelta()).toEqual([
      { insert: "(" },
      { insert: "hi!", attributes: { bold: true } },
    ]);

    const link = doc.getText("link");
    link.insert(0, "x");
    link.mark({ start: 0, end: 1 }, "link", "url");
    link.insert(1, "y");
    expect(link.toDelta()).toEqual([
      { insert: "x", attributes: { link: "url" } },
      { insert: "y" },
    ]);
  });

  test("matches sorted map and text edge contracts", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("z", 1);
    map.set("a", 2);
    expect(map.entries()).toEqual([
      ["a", 2],
      ["z", 1],
    ]);

    const text = doc.getText("text");
    text.insert(0, "x");
    expect(text.convertPos(0, "invalid" as never, "unicode")).toBeUndefined();
    expect(() => text.mark({ start: 0, end: 1 }, "not-configured", true)).toThrow(
      /not configured/u,
    );

    const detached = new LoroText();
    detached.insert(0, "x");
    expect(() =>
      detached.mark({ start: 0, end: 1 }, "not-configured", true),
    ).not.toThrow();
  });

  test("tracks stable cursors and replaces deleted anchors", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    text.insert(0, "123");
    const cursor = text.getCursor(0, 0)!;
    const end = text.getCursor(10, 0)!;

    expect(cursor.kind()).toBe("Cursor");
    expect(cursor.containerId()).toBe(text.id);
    expect(cursor.pos()).toEqual({ peer: "1", counter: 0 });
    expect(end.side()).toBe(1);
    expect(doc.getCursorPos(cursor)).toEqual({ offset: 0, side: 0 });

    text.insert(0, "abc");
    expect(doc.getCursorPos(cursor)).toEqual({ offset: 3, side: 0 });

    const decoded = Cursor.decode(cursor.encode());
    const remote = new LoroDoc();
    remote.import(doc.export({ mode: "update" }));
    expect(remote.getCursorPos(decoded)).toEqual({ offset: 3, side: 0 });

    text.delete(3, 1);
    doc.commit();
    const result = doc.getCursorPos(cursor)!;
    expect(result.offset).toBe(3);
    expect(result.side).toBe(-1);
    expect(result.update?.pos()).toEqual({ peer: "1", counter: 1 });
    expect(result.update?.side()).toBe(-1);
  });

  test("copies detached child containers and exposes their attached handlers", () => {
    const child = new LoroMap();
    child.set("ready", true);
    const nestedList = new LoroList();
    nestedList.push(1);
    child.setContainer("items", nestedList);

    const doc = new LoroDoc();
    doc.setPeerId(3);
    const attachedChild = doc.getMap("root").setContainer("child", child);

    expect(doc.toJSON()).toEqual({ root: { child: { ready: true, items: [1] } } });
    expect(child.isAttached()).toBe(false);
    expect(child.getAttached()).toBe(attachedChild);
    expect(nestedList.isAttached()).toBe(false);
    expect(nestedList.getAttached()?.isAttached()).toBe(true);

    child.set("detached-only", true);
    attachedChild.set("attached-only", true);
    expect(doc.toJSON()).toEqual({
      root: { child: { ready: true, items: [1], "attached-only": true } },
    });
  });

  test("converges concurrent edits in deterministic mergeable children", () => {
    const left = new LoroDoc();
    left.setPeerId(1);
    const right = new LoroDoc();
    right.setPeerId(2);

    const leftCounter = left.getMap("state").ensureMergeableCounter("revision");
    const rightCounter = right.getMap("state").ensureMergeableCounter("revision");
    leftCounter.increment(1);
    rightCounter.increment(1);

    const leftUpdate = left.export({ mode: "update" });
    const rightUpdate = right.export({ mode: "update" });
    left.import(rightUpdate);
    right.import(leftUpdate);

    expect(leftCounter.id).toBe(rightCounter.id);
    expect(leftCounter.value).toBe(2);
    expect(rightCounter.value).toBe(2);
    expect(left.toJSON()).toEqual({ state: { revision: 2 } });
    expect(right.toJSON()).toEqual(left.toJSON());
  });

  test("integrates update batches once in arbitrary blob order", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const text = source.getText("text");
    text.push("a");
    source.commit();
    const firstVersion = source.oplogVersion();
    const first = source.export({ mode: "update" });
    text.push("b");
    source.commit();
    const second = source.export({ mode: "update", from: firstVersion });

    const target = new LoroDoc();
    let events = 0;
    target.subscribe(() => {
      events += 1;
    });
    const status = target.importBatch([second, first]);

    expect(target.toJSON()).toEqual({ text: "ab" });
    expect(events).toBe(1);
    expect(status.success.get("1")).toEqual({ start: 0, end: 2 });
    expect(status.pending).toBeNull();
  });

  test("integrates mixed snapshot and update batches once", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const text = source.getText("text");
    text.push("a");
    source.commit();
    const firstSnapshot = source.export({ mode: "snapshot" });
    text.push("b");
    source.commit();
    const secondVersion = source.oplogVersion();
    const secondSnapshot = source.export({ mode: "snapshot" });
    text.push("c");
    source.commit();
    const update = source.export({ mode: "update", from: secondVersion });

    const target = new LoroDoc();
    const events: LoroEventBatch[] = [];
    target.subscribe((event) => events.push(event));
    const status = target.importBatch([update, firstSnapshot, secondSnapshot]);

    expect(target.toJSON()).toEqual({ text: "abc" });
    expect(events).toHaveLength(1);
    expect(events[0]!.events).toEqual([
      {
        target: "cid:root-text:Text",
        path: ["text"],
        diff: { type: "text", diff: [{ insert: "abc" }] },
      },
    ]);
    expect(status.success.get("1")).toEqual({ start: 0, end: 3 });
    expect(status.pending).toBeNull();
  });

  test("keeps mergeable ensure idempotent and guards non-mergeable values", () => {
    const doc = new LoroDoc();
    const root = doc.getMap("state");
    const first = root.ensureMergeableMap("profile");
    doc.commit();
    const opCount = doc.opCount();

    expect(root.ensureMergeableMap("profile")).toBe(first);
    doc.commit();
    expect(doc.opCount()).toBe(opCount);

    root.set("scalar", 1);
    expect(() => root.ensureMergeableList("scalar")).toThrow(/non-mergeable value/u);
    expect(root.get("scalar")).toBe(1);

    root.setContainer("regular", new LoroMap());
    expect(() => root.ensureMergeableMap("regular")).toThrow(/non-mergeable value/u);
  });

  test("does not record semantic no-op edits", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.set("value", { nested: [1, true, null] });
    const movable = doc.getMovableList("movable");
    movable.push("same");
    doc.commit();
    const opCount = doc.opCount();

    map.set("value", { nested: [1, true, null] });
    map.delete("missing");
    movable.set(0, "same");
    doc.getCounter("counter").increment(0);
    doc.commit();

    expect(doc.opCount()).toBe(opCount);
  });

  test("resurfaces preserved state and switches mergeable kinds", () => {
    const doc = new LoroDoc();
    const root = doc.getMap("state");
    const text = root.ensureMergeableText("field");
    text.insert(0, "hello");
    doc.commit();

    root.delete("field");
    doc.commit();
    expect(root.get("field")).toBeUndefined();
    expect(text.isDeleted()).toBe(true);

    const restored = root.ensureMergeableText("field");
    expect(restored).toBe(text);
    expect(restored.toString()).toBe("hello");

    const map = root.ensureMergeableMap("field");
    map.set("value", 1);
    expect(doc.toJSON()).toEqual({ state: { field: { value: 1 } } });
    expect(root.ensureMergeableText("field").toString()).toBe("hello");
  });

  test("uses flattened mergeable ids and logical event paths", () => {
    const doc = new LoroDoc();
    const root = doc.getMap("state");
    const profile = root.ensureMergeableMap("profile");
    const counter = profile.ensureMergeableCounter("revision");
    const batches: LoroEventBatch[] = [];
    root.subscribe((event) => batches.push(event));

    counter.increment(3);
    doc.commit();

    expect(profile.id).toBe("cid:root-🤝:$state>profile:Map");
    expect(counter.id).toBe("cid:root-🤝:$state>profile>revision:Counter");
    expect(doc.getContainerById(counter.id)).toBe(counter);
    expect(
      batches.at(-1)?.events.find((event) => event.target === counter.id)?.path,
    ).toEqual(["state", "profile", "revision"]);
  });

  test("distinguishes existing roots, normal children, and mergeable children", () => {
    const doc = new LoroDoc();
    expect(doc.hasContainer("cid:root-new:Text")).toBe(true);
    expect(doc.getContainerById("cid:root-new:Text")?.kind()).toBe("Text");

    expect(doc.hasContainer("cid:0@99:Text")).toBe(false);
    expect(() => doc.getText("cid:0@99:Text")).toThrow(/does not exist/);

    const root = doc.getMap("state");
    const empty = root.ensureMergeableText("empty");
    expect(doc.hasContainer(empty.id)).toBe(true);
    root.delete("empty");
    expect(doc.hasContainer(empty.id)).toBe(false);

    const preserved = root.ensureMergeableText("preserved");
    preserved.insert(0, "kept");
    doc.commit();
    root.delete("preserved");
    doc.commit();
    expect(doc.hasContainer(preserved.id)).toBe(true);
    expect(preserved.isDeleted()).toBe(true);
  });

  test("passes live containers through toJsonWithReplacer", () => {
    const doc = new LoroDoc();
    const root = doc.getMap("root");
    root.set("count", 1);
    root.set("drop", true);
    const body = root.setContainer("body", new LoroText());
    body.insert(0, "hello");

    const seen: string[] = [];
    expect(
      doc.toJsonWithReplacer((key, value) => {
        if (value instanceof LoroMap || value instanceof LoroText) {
          seen.push(`${String(key)}:${value.kind()}`);
        }
        if (value instanceof LoroText) return value.toString().toUpperCase();
        if (key === "count") return 2;
        if (key === "drop") return undefined;
        return value;
      }),
    ).toEqual({ root: { count: 2, body: "HELLO" } });
    expect(seen).toEqual(["root:Map", "body:Text"]);

    expect(() =>
      doc.toJsonWithReplacer((_key, value) => (value instanceof LoroText ? root : value)),
    ).toThrow(/different container/u);
  });

  test("iterates text by contiguous operation runs", () => {
    const left = new LoroDoc();
    left.setPeerId(1);
    left.getText("text").insert(0, "Hello");
    left.commit();

    const right = left.fork();
    right.setPeerId(2);
    right.getText("text").insert(3, " ");
    left.import(right.export({ mode: "update", from: left.oplogVersion() }));

    const chunks: string[] = [];
    left.getText("text").iter((chunk) => {
      chunks.push(chunk);
    });
    expect(chunks).toEqual(["Hel", " ", "lo"]);
    const firstChunk: string[] = [];
    left.getText("text").iter((chunk) => {
      firstChunk.push(chunk);
      return false;
    });
    expect(firstChunk).toEqual(["Hel"]);
    expect(left.getText("text").sliceDeltaUtf8(3, 4)).toEqual([{ insert: " " }]);
    expect(left.getText("text").getEditorOf(3)).toBe("2");
  });

  test("guards tree cycles and reports move and fractional-index state", () => {
    const doc = new LoroDoc();
    doc.setPeerId(7);
    const tree = doc.getTree("tree");
    const root = tree.createNode();
    const child = root.createNode();
    const creation = child.creationId();

    expect(child.creator()).toBe("7");
    expect(child.getLastMoveId()).toEqual(creation);
    expect(child.fractionalIndex()).toBeTypeOf("string");
    expect(() => root.move(child)).toThrow(/descendant/u);

    child.move(undefined, 0);
    expect(child.getLastMoveId()).not.toEqual(creation);
    tree.disableFractionalIndex();
    expect(tree.isFractionalIndexEnabled()).toBe(false);
    expect(child.fractionalIndex()).toBeUndefined();
    tree.enableFractionalIndex();
    expect(tree.isFractionalIndexEnabled()).toBe(true);
    expect(() => tree.enableFractionalIndex(256)).toThrow(/unsigned byte/u);

    const deletedFractionalIndex = child.fractionalIndex();
    tree.delete(child.id);
    expect(tree.isNodeDeleted(child.id)).toBe(true);
    expect(tree.getNodes({ withDeleted: true })).toHaveLength(2);
    expect(root.toJSON()).toMatchObject({ parent: undefined, fractionalIndex: "80" });
    expect(child.toJSON()).toMatchObject({
      parent: "2147483647@18446744073709551615",
      index: 0,
      fractionalIndex: deletedFractionalIndex,
    });
  });

  test("round-trips frontiers and reports a package version", () => {
    const doc = new LoroDoc();
    doc.setPeerId(11);
    doc.getText("text").insert(0, "hello");
    doc.commit();
    const frontiers = doc.frontiers();

    expect(decodeFrontiers(encodeFrontiers(frontiers))).toEqual(frontiers);
    expect(LORO_VERSION()).toMatch(/^\d+\.\d+\.\d+(?:[-+].+)?$/u);
  });

  test("synchronizes postcard-compatible awareness state", () => {
    const a = new AwarenessWasm("1", 30_000);
    const b = new AwarenessWasm("2", 30_000);
    a.setLocalState({ cursor: 3, bytes: Uint8Array.of(1, 2, 3) });

    expect(b.apply(a.encode(["1"]))).toEqual({ added: ["1"], updated: [] });
    expect(b.getState("1")).toEqual({ cursor: 3, bytes: Uint8Array.of(1, 2, 3) });
    expect(b.peer()).toBe("2");
    expect(b.length()).toBe(1);

    const old = a.encodeAll();
    a.setLocalState({ cursor: 4 });
    const current = a.encodeAll();
    expect(b.apply(current)).toEqual({ added: [], updated: ["1"] });
    expect(b.apply(old)).toEqual({ added: [], updated: [] });
    expect(b.getState("1")).toEqual({ cursor: 4 });
    expect(() => b.apply(Uint8Array.of(0xff))).toThrow(/Failed to decode awareness/u);

    const wrapped = new Awareness("3", 30_000);
    const events: unknown[] = [];
    wrapped.addListener((update, origin) => events.push([update, origin]));
    wrapped.setLocalState("online");
    expect(wrapped.getLocalState()).toBe("online");
    expect(events).toEqual([[{ updated: [], added: ["3"], removed: [] }, "local"]]);
    wrapped.destroy();
  });

  test("synchronizes typed ephemeral state and deletion tombstones", async () => {
    const a = new EphemeralStoreWasm(30_000);
    const b = new EphemeralStoreWasm(30_000);
    const localUpdates: Uint8Array[] = [];
    const events: unknown[] = [];
    a.subscribeLocalUpdates((bytes) => localUpdates.push(bytes));
    b.subscribe((event) => events.push(event));

    a.set("cursor", { x: 1, bytes: Uint8Array.of(4, 5) });
    b.apply(localUpdates.at(-1)!);
    expect(b.getAllStates()).toEqual({
      cursor: { x: 1, bytes: Uint8Array.of(4, 5) },
    });
    expect(events.at(-1)).toEqual({
      by: "import",
      added: ["cursor"],
      updated: [],
      removed: [],
    });

    await new Promise((resolve) => setTimeout(resolve, 2));
    a.delete("cursor");
    b.apply(localUpdates.at(-1)!);
    expect(b.get("cursor")).toBeUndefined();
    expect(events.at(-1)).toEqual({
      by: "import",
      added: [],
      updated: [],
      removed: ["cursor"],
    });

    const typed = new EphemeralStore<{ name: string; position: number }>(30_000);
    typed.set("name", "Ada");
    typed.set("position", 2);
    expect(typed.getAllStates()).toEqual({ name: "Ada", position: 2 });
    typed.destroy();
  });

  test("queues nested ephemeral updates until the current callback returns", () => {
    const store = new EphemeralStore(30_000);
    let calls = 0;
    store.subscribe(() => {
      if (calls === 0) store.set("a", 2);
      calls += 1;
    });

    store.set("a", 1);
    store.set("b", 2);
    store.set("c", 3);

    expect(calls).toBe(4);
    expect(store.getAllStates()).toEqual({ a: 2, b: 2, c: 3 });
    store.destroy();
  });

  test("provides compatible detached classes and version vectors", () => {
    expect(new LoroMap().kind()).toBe("Map");
    expect(new LoroList().kind()).toBe("List");
    expect(new LoroMovableList().kind()).toBe("MovableList");
    expect(new LoroText().kind()).toBe("Text");
    expect(new LoroCounter().kind()).toBe("Counter");

    const version = new VersionVector(new Map([["1", 2]]));
    version.setLast({ peer: "2", counter: 3 });
    expect(VersionVector.decode(version.encode()).toJSON()).toEqual(
      new Map([
        ["1", 2],
        ["2", 4],
      ]),
    );
  });

  test("loads the deep value from a current Rust snapshot", () => {
    const doc = LoroDoc.fromSnapshot(fixture("snapshot.blob"));
    const expected = JSON.parse(
      readFileSync(
        new URL("./fixtures/rust/snapshot.deep.json", import.meta.url),
        "utf8",
      ),
    ) as Record<string, unknown>;
    const actual = doc.toJSON();
    const actualMap = actual.map as Record<string, unknown>;
    const expectedMap = expected.map as Record<string, unknown>;
    const actualChildMap = actualMap.child_map as Record<string, unknown>;

    expect(actualChildMap.a).toBe(1);
    expect(actualChildMap.t).toBe("inn");
    expect(actualChildMap.v1476140860).toEqual(Uint8Array.of(0, 1, 2, 46));
    expect(actualMap.child_list).toEqual(expectedMap.child_list);
    expect(actualMap.child_mlist).toEqual(expectedMap.child_mlist);
    expect(actual.list).toEqual(expected.list);
    expect(actual.mlist).toEqual(expected.mlist);
    expect(actual.text).toEqual(expected.text);
    expect((actual.tree as unknown[]).length).toBe((expected.tree as unknown[]).length);

    const version = doc.version().toJSON();
    expect(doc.changeCount()).toBeGreaterThan(0);
    expect(doc.version().toJSON()).toEqual(version);
    expect(doc.toJSON()).toEqual(actual);
  });
});
