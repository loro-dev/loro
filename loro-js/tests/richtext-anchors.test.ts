import { readFileSync } from "node:fs";

import { describe, expect, test } from "vitest";

import {
  ContainerType,
  decodeFastSnapshot,
  decodeStateSnapshotStore,
} from "../src/codec/index";
import {
  LoroDoc,
  LoroText,
  UndoManager,
  type Delta,
  type Frontiers,
  type JsonSchema,
} from "../src/index";
import { applyDelta, normalizeDelta } from "./support/richtext-differential";
import { loadRustReference } from "./support/rust-reference";

/**
 * Rust-side fixtures from `scripts/write-rust-richtext-fixtures.cjs`: loro-crdt
 * ran the same edits and recorded its JSON updates, binary updates, snapshot,
 * resulting delta, and deltas at recorded versions.
 */
interface RustFixture {
  readonly delta: Delta<string>[];
  readonly checkouts: {
    readonly frontiers: Frontiers;
    readonly delta: Delta<string>[];
  }[];
  readonly updates: JsonSchema;
}

const fixtureUrl = (name: string): URL =>
  new URL(`./fixtures/rust/${name}`, import.meta.url);
const rustFixture = (name: string): RustFixture =>
  JSON.parse(readFileSync(fixtureUrl(`${name}.json`), "utf8")) as RustFixture;
const rustBlob = (name: string): Uint8Array =>
  new Uint8Array(readFileSync(fixtureUrl(name)));

function newDoc(): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.configTextStyle({ bold: { expand: "after" }, link: { expand: "none" } });
  return doc;
}

const scenarios: Record<string, (doc: LoroDoc) => Frontiers[]> = {
  "richtext-mark-positions"(doc) {
    const text = doc.getText("text");
    text.insert(0, "abcd");
    doc.commit();
    text.mark({ start: 0, end: 2 }, "bold", true);
    doc.commit();
    text.insert(3, "X");
    doc.commit();
    text.delete(1, 1);
    doc.commit();
    return [];
  },
  "richtext-unmark-emoji"(doc) {
    const text = doc.getText("text");
    text.insert(0, "a😀bc");
    doc.commit();
    text.mark({ start: 0, end: 3 }, "bold", true);
    doc.commit();
    text.unmark({ start: 0, end: 1 }, "bold");
    doc.commit();
    text.delete(3, 1);
    doc.commit();
    return [];
  },
  "richtext-checkout-mark"(doc) {
    const text = doc.getText("text");
    text.insert(0, "abc");
    doc.commit();
    const version = doc.frontiers();
    text.mark({ start: 0, end: 3 }, "bold", true);
    text.insert(3, "d");
    doc.commit();
    return [version];
  },
};

function expectCheckouts(doc: LoroDoc, fixture: RustFixture): void {
  for (const { frontiers, delta } of fixture.checkouts) {
    doc.checkout(frontiers);
    expect(doc.getText("text").toDelta()).toEqual(delta);
    doc.checkoutToLatest();
  }
  expect(doc.getText("text").toDelta()).toEqual(fixture.delta);
}

describe("rich-text style anchors match Rust positions", () => {
  for (const [name, edit] of Object.entries(scenarios)) {
    test(`${name}: writes Rust's ops and reads Rust's updates`, () => {
      const fixture = rustFixture(name);
      const doc = newDoc();
      edit(doc);
      expect(doc.getText("text").toDelta()).toEqual(fixture.delta);
      // Every op position (insert, delete, mark start/end) matches Rust's.
      expect(doc.exportJsonUpdates().changes).toEqual(fixture.updates.changes);
      expectCheckouts(doc, fixture);

      for (const source of [
        (target: LoroDoc) => target.importJsonUpdates(fixture.updates),
        (target: LoroDoc) => target.import(rustBlob(`${name}.blob`)),
      ]) {
        const imported = new LoroDoc();
        source(imported);
        expect(imported.getText("text").toDelta()).toEqual(fixture.delta);
        expectCheckouts(imported, fixture);
      }
      const fromSnapshot = new LoroDoc();
      fromSnapshot.import(rustBlob(`${name}.snapshot.blob`));
      expect(fromSnapshot.getText("text").toDelta()).toEqual(fixture.delta);

      // loro.js snapshots keep the anchors, so their history checks out too.
      const reloaded = new LoroDoc();
      reloaded.import(doc.export({ mode: "update" }));
      expectCheckouts(reloaded, fixture);
      const snapshot = new LoroDoc();
      snapshot.import(doc.export({ mode: "snapshot" }));
      expect(snapshot.getText("text").toDelta()).toEqual(fixture.delta);
    });
  }

  test("text inserted next to anchors follows Rust's expand rules", () => {
    const doc = newDoc();
    doc.configTextStyle({
      bold: { expand: "after" },
      link: { expand: "none" },
      em: { expand: "before" },
      hl: { expand: "both" },
    });
    const text = doc.getText("text");
    text.insert(0, "--AB--");
    for (const key of ["bold", "link", "em", "hl"]) {
      text.mark({ start: 2, end: 4 }, key, true);
    }
    // At the start: only em and hl expand before. At the end: bold and hl
    // expand after.
    text.insert(4, "e");
    text.insert(2, "s");
    expect(text.toDelta()).toEqual([
      { insert: "--" },
      { insert: "s", attributes: { em: true, hl: true } },
      { insert: "AB", attributes: { bold: true, em: true, hl: true, link: true } },
      { insert: "e", attributes: { bold: true, hl: true } },
      { insert: "--" },
    ]);
    // The unmark's end anchor expands after (the reverse of none), so "u" stays
    // unbolded; before "A", the bold start anchor keeps "v" outside bold and
    // link. The expected delta is Rust's output for the same edits.
    text.unmark({ start: 3, end: 4 }, "bold");
    text.insert(4, "u");
    text.insert(3, "v");
    expect(text.toDelta()).toEqual([
      { insert: "--" },
      { insert: "sv", attributes: { em: true, hl: true } },
      { insert: "Au", attributes: { link: true, em: true, hl: true } },
      { insert: "B", attributes: { bold: true, link: true, em: true, hl: true } },
      { insert: "e", attributes: { bold: true, hl: true } },
      { insert: "--" },
    ]);
  });

  test("marks that change nothing write no anchors", () => {
    const doc = newDoc();
    const text = doc.getText("text");
    text.insert(0, "abc");
    doc.commit();
    const ops = (): number =>
      doc.exportJsonUpdates().changes.flatMap((c) => c.ops).length;
    const before = ops();
    text.unmark({ start: 0, end: 2 }, "bold");
    doc.commit();
    expect(ops()).toBe(before);
    text.mark({ start: 0, end: 2 }, "bold", true);
    doc.commit();
    expect(ops()).toBe(before + 2);
    text.mark({ start: 0, end: 1 }, "bold", true);
    doc.commit();
    expect(ops()).toBe(before + 2);
    expect(() => text.mark({ start: 1, end: 1 }, "bold", true)).toThrow(RangeError);
  });

  test("positions convert across Unicode, UTF-16, and UTF-8 around anchors", () => {
    const doc = newDoc();
    const text = doc.getText("text");
    text.insert(0, "a😀b中c");
    text.mark({ start: 1, end: 4 }, "bold", true);
    text.mark({ start: 3, end: 5 }, "link", "x");
    expect(text.length).toBe(6);
    expect(text.convertPos(5, "unicode", "utf16")).toBe(6);
    expect(text.convertPos(3, "unicode", "utf8")).toBe(6);
    expect(text.convertPos(6, "utf8", "unicode")).toBe(3);
    expect(text.convertPos(3, "utf16", "unicode")).toBe(2);
    expect(text.convertPos(2, "utf16", "unicode")).toBeUndefined();
    expect(text.slice(1, 4)).toBe("😀b");
    expect(text.charAt(3)).toBe("b");
    expect(text.getCursor(3)?.pos()).toEqual({ peer: "1", counter: 2 });
    let chunks = "";
    text.iter((chunk) => {
      chunks += chunk;
    });
    expect(chunks).toBe("a😀b中c");
    text.delete(1, 3); // 😀b: the anchors stay
    expect(text.toString()).toBe("a中c");
    expect(text.toDelta()).toEqual([
      { insert: "a" },
      { insert: "中", attributes: { link: "x" } },
      { insert: "c" },
    ]);
  });

  test("concurrent inserts at a style boundary follow the anchors' Fugue order", () => {
    const base = newDoc();
    base.getText("text").insert(0, "ab");
    base.commit();
    const left = new LoroDoc();
    left.setPeerId(2);
    left.import(base.export({ mode: "update" }));
    const right = new LoroDoc();
    right.setPeerId(3);
    right.import(base.export({ mode: "update" }));
    left.configTextStyle({ bold: { expand: "after" } });
    left.getText("text").mark({ start: 0, end: 1 }, "bold", true);
    left.commit();
    right.getText("text").insert(1, "X");
    right.commit();
    left.import(right.export({ mode: "update" }));
    right.import(left.export({ mode: "update" }));
    // X was inserted concurrently after "a" without seeing the mark; Fugue
    // places it after the end anchor, outside the style.
    for (const doc of [left, right]) {
      expect(doc.getText("text").toDelta()).toEqual([
        { insert: "a", attributes: { bold: true } },
        { insert: "Xb" },
      ]);
    }
  });

  test("undo and redo restore the attributes a mark replaced", () => {
    const doc = newDoc();
    const undo = new UndoManager(doc, { mergeInterval: 0 });
    const text = doc.getText("text");
    text.insert(0, "Hello World");
    doc.commit();
    text.mark({ start: 0, end: 5 }, "bold", true);
    doc.commit();
    doc.configTextStyle({ bold: { expand: "after" }, italic: { expand: "after" } });
    text.mark({ start: 3, end: 8 }, "italic", true);
    doc.commit();
    text.unmark({ start: 1, end: 4 }, "bold");
    doc.commit();
    // Each state below is what Rust's UndoManager produces for the same edits.
    const steps = [
      () => undo.undo(),
      () => undo.undo(),
      () => undo.undo(),
      () => undo.redo(),
      () => undo.redo(),
    ];
    const deltas = steps.map((step) => {
      step();
      return text.toDelta();
    });
    expect(deltas).toEqual([
      [
        { insert: "Hel", attributes: { bold: true } },
        { insert: "lo", attributes: { bold: true, italic: true } },
        { insert: " Wo", attributes: { italic: true } },
        { insert: "rld" },
      ],
      [{ insert: "Hello", attributes: { bold: true } }, { insert: " World" }],
      [{ insert: "Hello World" }],
      [{ insert: "Hello", attributes: { bold: true } }, { insert: " World" }],
      [
        { insert: "Hel", attributes: { bold: true } },
        { insert: "lo", attributes: { bold: true, italic: true } },
        { insert: " Wo", attributes: { italic: true } },
        { insert: "rld" },
      ],
    ]);
  });

  test("detached text marks with anchors and keeps them when attached", () => {
    const doc = newDoc();
    const map = doc.getMap("map");
    const detached = new LoroText();
    detached.insert(0, "hello");
    detached.mark({ start: 0, end: 5 }, "bold", true);
    detached.insert(5, "!");
    expect(detached.toDelta()).toEqual([
      { insert: "hello!", attributes: { bold: true } },
    ]);
    const attached = map.setContainer("text", detached);
    expect(attached.toDelta()).toEqual([
      { insert: "hello!", attributes: { bold: true } },
    ]);
  });
});

interface Runtime {
  readonly LoroDoc: typeof LoroDoc;
  readonly LoroText: typeof LoroText;
}

const rust = loadRustReference();
const runtimes: [string, Runtime][] = [["loro.js", { LoroDoc, LoroText }]];
if (rust !== undefined) runtimes.push(["loro-crdt", rust]);

/** The values of the style marks in a Text state of an encoded state store. */
function markValues(stateBytes: Uint8Array): unknown[] {
  const store = decodeStateSnapshotStore(stateBytes);
  if (store.kind !== "sstable") return [];
  return store.containers.flatMap(({ wrapper: { state } }) =>
    state.kind === ContainerType.Text
      ? state.marks.map(({ value }) => (value.type === "null" ? null : value.value))
      : [],
  );
}

describe.each(runtimes)("rich-text anchors in %s", (runtimeName, runtime) => {
  test("inserts into a nested text that a snapshot loaded lazily", () => {
    const source = new runtime.LoroDoc();
    source.setPeerId(1);
    source.configTextStyle({ bold: { expand: "before" } });
    const text = source.getMap("m").setContainer("t", new runtime.LoroText());
    text.insert(0, "abcd");
    text.mark({ start: 0, end: 4 }, "bold", true);
    source.commit();
    const snapshot = source.export({ mode: "snapshot" });

    const insertInto = (position: number): Delta<string>[] => {
      const doc = new runtime.LoroDoc();
      doc.setPeerId(2);
      doc.configTextStyle({ bold: { expand: "before" } });
      doc.import(snapshot);
      // Not read before the insert, so the text is still unhydrated.
      const nested = doc.getMap("m").get("t") as LoroText;
      nested.insert(position, "X");
      return nested.toDelta();
    };
    // bold expands before: X at 0 joins it, X at 4 does not.
    expect(insertInto(0)).toEqual([{ insert: "Xabcd", attributes: { bold: true } }]);
    expect(insertInto(2)).toEqual([{ insert: "abXcd", attributes: { bold: true } }]);
    expect(insertInto(4)).toEqual([
      { insert: "abcd", attributes: { bold: true } },
      { insert: "X" },
    ]);
  });

  test.each(["none", "before", "after", "both"] as const)(
    "shallow snapshots null a %s style that no text is left in",
    (expand) => {
      const shallow = (styleDiesAfterRoot: boolean): Uint8Array => {
        const doc = new runtime.LoroDoc();
        doc.setPeerId(1);
        doc.setChangeMergeInterval(0);
        doc.configTextStyle({ link: { expand } });
        const text = doc.getText("t");
        text.insert(0, "x");
        doc.commit();
        text.mark({ start: 0, end: 1 }, "link", "private");
        doc.commit();
        let root: Frontiers | undefined;
        if (styleDiesAfterRoot) {
          doc.getMap("meta").set("v", 0);
          doc.commit();
          root = doc.frontiers();
        }
        text.delete(0, 1);
        doc.commit();
        doc.getMap("meta").set("v", 1);
        doc.commit();
        return doc.export({
          mode: "shallow-snapshot",
          frontiers: root ?? doc.frontiers(),
        });
      };

      // Dead at the root: the value is nulled there and in the latest state,
      // except for a both-expand style, which still styles future inserts.
      const dead = decodeFastSnapshot(shallow(false));
      const kept = expand === "both" ? "private" : null;
      expect(markValues(dead.shallowRootState)).toEqual([kept]);
      // Alive at the root: the latest state keeps it for historical checkout.
      const late = decodeFastSnapshot(shallow(true));
      expect(markValues(late.shallowRootState)).toEqual(["private"]);
      // Rust writes no latest state for so few ops; loro.js always does.
      const writesLatest = runtimeName === "loro.js";
      expect(markValues(dead.state)).toEqual(writesLatest ? [kept] : []);
      expect(markValues(late.state)).toEqual(writesLatest ? ["private"] : []);
      const replica = new runtime.LoroDoc();
      replica.import(shallow(false));
      expect(replica.getText("t").toString()).toBe("");
    },
  );

  test("a checkout that adds only a mark's start anchor changes no text", () => {
    const doc = new runtime.LoroDoc();
    doc.setPeerId(5);
    doc.configTextStyle({ bold: { expand: "after" } });
    const text = doc.getText("t");
    text.insert(0, "abc");
    doc.commit();
    text.mark({ start: 1, end: 3 }, "bold", "x");
    doc.commit();
    text.delete(1, 2);
    doc.commit();

    // 2@5 is before the mark, 3@5 has its start anchor but not its end.
    const versions: Frontiers[] = [2, 3, 4, 3, 2, 4, 5, 3].map((counter) => [
      { peer: "5", counter },
    ]);
    doc.checkout(versions[0]!);
    let mirror = text.toDelta();
    doc.subscribe((batch) => {
      for (const event of batch.events) {
        if (event.diff.type === "text") {
          mirror = applyDelta(mirror, event.diff.diff as Delta<string>[]);
        }
      }
    });
    for (const version of versions.slice(1)) {
      doc.checkout(version);
      expect(normalizeDelta(mirror)).toEqual(normalizeDelta(text.toDelta()));
    }
  });

  test("a snapshot's anchors survive moving between old versions", () => {
    const source = new runtime.LoroDoc();
    source.setPeerId(1);
    source.configTextStyle({ link: { expand: "none" } });
    source.getText("t").insert(0, "init");
    source.getText("t").mark({ start: 1, end: 4 }, "link", "y");
    // The movable-list set keeps the checkout from moving through anchors only.
    source.getMovableList("ml").insert(0, 37);
    source.getMovableList("ml").set(0, 56);
    source.commit();
    const snapshot = source.export({ mode: "snapshot" });
    const styled = [{ insert: "i" }, { insert: "nit", attributes: { link: "y" } }];
    const at = (counter: number): Frontiers => [{ peer: "1", counter }];

    for (const [first, second] of [
      [5, 3],
      [4, 3],
    ] as const) {
      const doc = new runtime.LoroDoc();
      doc.configTextStyle({ link: { expand: "none" } });
      doc.import(snapshot);
      const latest = doc.frontiers();
      doc.checkout(at(first));
      doc.checkout(at(second));
      expect(doc.getText("t").toDelta()).toEqual([{ insert: "init" }]);
      doc.checkout(at(7));
      expect(normalizeDelta(doc.getText("t").toDelta())).toEqual(normalizeDelta(styled));
      expect(doc.getMovableList("ml").toJSON()).toEqual([56]);
      doc.checkout(at(second));
      doc.attach();
      expect(doc.frontiers()).toEqual(latest);
      expect(normalizeDelta(doc.getText("t").toDelta())).toEqual(normalizeDelta(styled));
    }

    const doc = new runtime.LoroDoc();
    doc.configTextStyle({ link: { expand: "none" } });
    doc.import(snapshot);
    doc.checkout(at(5));
    doc.checkout(at(3));
    const diff = doc.diff(at(3), at(7));
    expect(diff).toEqual([
      [
        "cid:root-t:Text",
        {
          type: "text",
          diff: [{ retain: 1 }, { retain: 3, attributes: { link: "y" } }],
        },
      ],
      ["cid:root-ml:MovableList", { type: "list", diff: [{ insert: [56] }] }],
    ]);
    doc.attach();
    expect(normalizeDelta(doc.getText("t").toDelta())).toEqual(normalizeDelta(styled));
  });
});
