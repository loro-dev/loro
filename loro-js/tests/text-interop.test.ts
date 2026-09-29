import { readFileSync } from "node:fs";

import { describe, expect, test } from "vitest";

import { LoroDoc, type Delta, type JsonSchema } from "../src/index";
import { applyDelta, normalizeDelta } from "./support/richtext-differential";
import { loadRustReference, type RustReference } from "./support/rust-reference";

const fixture = (name: string): Uint8Array =>
  new Uint8Array(readFileSync(new URL(`./fixtures/rust/${name}`, import.meta.url)));

function doc(peer: number): LoroDoc {
  const created = new LoroDoc();
  created.setPeerId(peer);
  return created;
}

function sync(from: LoroDoc, to: LoroDoc): void {
  to.import(from.export({ mode: "update" }));
}

describe("text interoperability with Rust", () => {
  test("applies a remote delete by its position, like Rust's tracker", () => {
    // Rust's WASM build recorded `start_id` 0@7 for a delete of a, 😀, x: it
    // advanced the ID by the UTF-16 length of 😀, so the recorded IDs name b
    // instead of x. Rust applies the delete by position and keeps "b".
    const { text, updates } = JSON.parse(
      readFileSync(
        new URL("./fixtures/rust/text-delete-positions.json", import.meta.url),
        "utf8",
      ),
    ) as { text: string; updates: JsonSchema };
    expect(text).toBe("b");

    const fromJson = new LoroDoc();
    fromJson.importJsonUpdates(updates);
    expect(fromJson.getText("text").toString()).toBe("b");

    const fromBinary = new LoroDoc();
    fromBinary.import(fixture("text-delete-positions.blob"));
    expect(fromBinary.getText("text").toString()).toBe("b");
  });

  test("orders a concurrent insert before a later sibling subtree", () => {
    const peer1 = doc(1);
    const peer2 = doc(2);
    const peer3 = doc(3);
    peer2.getText("t").insert(0, "9");
    peer2.commit();
    peer1.getText("t").insert(0, "b");
    peer1.commit();
    sync(peer1, peer2);
    expect(peer2.getText("t").toString()).toBe("b9");
    peer2.getText("t").insert(1, "é");
    peer2.commit();
    sync(peer1, peer3);
    peer3.getText("t").insert(1, "a");
    peer3.commit();

    // "a" follows b and é (b's subtree) but precedes 9, which is not in it.
    for (const target of [peer1, peer2, peer3]) {
      for (const source of [peer1, peer2, peer3]) {
        if (source !== target) sync(source, target);
      }
      expect(target.getText("t").toString()).toBe("béa9");
    }
  });

  test("orders a concurrent insert after a sibling subtree of several ID runs", () => {
    // z goes after the whole subtree of peer 2's first "a": its later run of a,
    // the B run, and peer 4's c run.
    const scenario = ({ LoroDoc: Doc }: { LoroDoc: typeof LoroDoc }): string => {
      const make = (peer: number): LoroDoc => {
        const created = new Doc();
        created.setPeerId(peer);
        return created;
      };
      const [p1, p2, p3, p4, target] = [1, 2, 3, 4, 9].map(make) as [
        LoroDoc,
        LoroDoc,
        LoroDoc,
        LoroDoc,
        LoroDoc,
      ];
      p2.getText("t").insert(0, "a".repeat(100));
      p2.commit();
      sync(p2, p4);
      p2.getText("t").insert(50, "B".repeat(40));
      p2.commit();
      p4.getText("t").insert(80, "c".repeat(30));
      p4.commit();
      p1.getText("t").insert(0, "y");
      p1.commit();
      p3.getText("t").insert(0, "z");
      p3.commit();
      for (const source of [p2, p4, p1, p3]) sync(source, target);
      return target.getText("t").toString();
    };
    const expected = `ya${"a".repeat(49)}${"B".repeat(40)}${"a".repeat(30)}${"c".repeat(30)}${"a".repeat(20)}z`;
    expect(scenario({ LoroDoc })).toBe(expected);
    // The same edits in Rust, when its WASM build is available.
    const rust: RustReference | undefined = loadRustReference();
    expect(rust === undefined ? expected : scenario(rust)).toBe(expected);
  });

  test("writes multi-run deletes last run first, like Rust", () => {
    const owner = doc(1);
    const text = owner.getText("t");
    text.insert(0, "abcd");
    text.insert(2, "X");
    owner.commit();
    text.delete(1, 3); // b, X, c: three ID runs
    owner.commit();
    const ops = owner
      .exportJsonUpdates()
      .changes.at(-1)!
      .ops.map((op) => op.content);
    // Output of Rust (loro-crdt) for the same edits; peer 1 is index 0.
    expect(ops.slice(-3)).toEqual([
      { type: "delete", pos: 3, len: 1, start_id: "2@0" },
      { type: "delete", pos: 2, len: 1, start_id: "4@0" },
      { type: "delete", pos: 1, len: 1, start_id: "1@0" },
    ]);
    expect(text.toString()).toBe("ad");
  });

  test("merges consecutive text inserts in a transaction, like Rust", () => {
    const owner = doc(1);
    const text = owner.getText("t");
    text.insert(0, "中文");
    owner.commit();
    text.insert(1, "😀");
    text.insert(3, "ab");
    owner.commit();
    const ops = owner
      .exportJsonUpdates()
      .changes.flatMap((change) => change.ops.map((op) => op.content));
    expect(ops).toEqual([
      { type: "insert", pos: 0, text: "中文" },
      { type: "insert", pos: 1, text: "😀ab" },
    ]);
  });

  test("checkout events restore an element deleted twice only once", () => {
    const peer1 = doc(1);
    const peer2 = doc(2);
    peer2.getText("t").insert(0, "中文");
    peer2.commit();
    const base = peer2.frontiers();
    sync(peer2, peer1);
    peer2.getText("t").delete(0, 2);
    peer2.commit();
    peer1.getText("t").delete(1, 1);
    peer1.commit();
    sync(peer2, peer1);
    peer1.getText("t").insert(0, "xyz");
    peer1.commit();

    let shadow: Delta<string>[] = peer1.getText("t").toDelta();
    peer1.subscribe((batch) => {
      for (const event of batch.events) {
        if (event.diff.type === "text") {
          shadow = applyDelta(shadow, event.diff.diff as Delta<string>[]);
        }
      }
    });
    peer1.checkout(base);
    expect(peer1.getText("t").toString()).toBe("中文");
    expect(normalizeDelta(shadow)).toEqual(normalizeDelta(peer1.getText("t").toDelta()));
    peer1.checkoutToLatest();
    expect(normalizeDelta(shadow)).toEqual(normalizeDelta(peer1.getText("t").toDelta()));
  });

  test("text cursors follow Rust's position semantics", () => {
    const owner = doc(1);
    const text = owner.getText("t");
    const empty = text.getCursor(0, 0)!;
    expect(empty.pos()).toBeUndefined();
    expect(empty.side()).toBe(-1);

    text.insert(0, "a😀bc");
    owner.commit();
    const end = text.getCursor(text.length, -1)!;
    expect(end.pos()).toBeUndefined();
    expect(end.side()).toBe(1);
    expect(owner.getCursorPos(end)).toEqual({ offset: 5, side: 1 });
    expect(text.getCursor(2, 0)).toBeUndefined();

    // The encoded cursor matches Rust byte for byte: its origin is the Unicode
    // position (2), not the UTF-16 position (3).
    expect([...text.getCursor(3, 1)!.encode()]).toEqual([1, 1, 4, 0, 1, 116, 0, 2, 2]);
    expect([...text.getCursor(5, 1)!.encode()]).toEqual([0, 0, 1, 116, 0, 2, 5]);

    // A visible target reports its own offset for every side.
    const right = text.getCursor(3, 1)!;
    expect(right.pos()).toEqual({ peer: "1", counter: 2 });
    expect(owner.getCursorPos(right)).toEqual({ offset: 3, side: 1 });

    // A deleted target reports the length before it, on the left side.
    text.delete(1, 3);
    owner.commit();
    const deleted = owner.getCursorPos(right)!;
    expect(deleted.offset).toBe(1);
    expect(deleted.side).toBe(-1);
    expect(deleted.update?.pos()).toEqual({ peer: "1", counter: 3 });
    expect(deleted.update?.side()).toBe(-1);
  });
});
