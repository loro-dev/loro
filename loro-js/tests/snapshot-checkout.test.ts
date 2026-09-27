import { readFileSync } from "node:fs";

import { describe, expect, test, vi } from "vitest";

import {
  LoroDoc,
  LoroList,
  LoroMap,
  LoroText,
  type Delta,
  type Frontiers,
  type LoroEventBatch,
} from "../src/index";

/**
 * A snapshot import keeps non-root containers encoded until they are first read.
 * Checkout must not hydrate such a container with its latest state after moving
 * the document to another version.
 */
function buildHistory(): { doc: LoroDoc; versions: Frontiers[] } {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const root = doc.getMap("root");
  const map = root.setContainer("map", new LoroMap());
  const list = root.setContainer("list", new LoroList());
  const text = root.setContainer("text", new LoroText());
  map.set("a", 1);
  list.push(1);
  text.insert(0, "abc");
  doc.commit();
  const versions = [doc.frontiers()];
  map.set("a", 2);
  map.set("b", 3);
  list.push(2);
  text.insert(0, "X");
  doc.commit();
  versions.push(doc.frontiers());
  map.delete("b");
  list.delete(0, 1);
  text.delete(1, 2);
  doc.commit();
  versions.push(doc.frontiers());
  return { doc, versions };
}

const valuesAt = (doc: LoroDoc, versions: readonly Frontiers[]): unknown[] =>
  versions.map((version) => {
    doc.checkout(version);
    return doc.toJSON();
  });

describe("checkout after a lazy snapshot import", () => {
  test("matches the full-history document at every version", () => {
    const { doc, versions } = buildHistory();
    const expected = valuesAt(doc, versions);
    doc.checkoutToLatest();
    const bytes = doc.export({ mode: "snapshot" });

    // Checking out before anything is read exercises both the version transition
    // and the full-replay paths on unhydrated children.
    for (const order of [versions, [...versions].reverse()]) {
      const loaded = new LoroDoc();
      loaded.import(bytes);
      const expectedInOrder = order.map((version) => expected[versions.indexOf(version)]);
      expect(valuesAt(loaded, order)).toEqual(expectedInOrder);
      loaded.checkoutToLatest();
      expect(loaded.toJSON()).toEqual(doc.toJSON());
    }
  });

  test("reads a child first touched after checkout at its historical state", () => {
    const { doc, versions } = buildHistory();
    const loaded = new LoroDoc();
    loaded.import(doc.export({ mode: "snapshot" }));
    loaded.checkout(versions[0]!);
    const root = loaded.getMap("root");
    expect((root.get("map") as LoroMap).toJSON()).toEqual({ a: 1 });
    expect((root.get("list") as LoroList).toJSON()).toEqual([1]);
    expect((root.get("text") as LoroText).toString()).toBe("abc");
  });

  test("keeps exported snapshots consistent after checkout round trips", () => {
    const { doc, versions } = buildHistory();
    const loaded = new LoroDoc();
    loaded.import(doc.export({ mode: "snapshot" }));
    loaded.checkout(versions[0]!);
    loaded.checkoutToLatest();
    const again = new LoroDoc();
    again.import(loaded.export({ mode: "snapshot" }));
    expect(again.toJSON()).toEqual(doc.toJSON());
    again.checkout(versions[1]!);
    doc.checkout(versions[1]!);
    expect(again.toJSON()).toEqual(doc.toJSON());
  });

  test("checks out a shallow snapshot through its retained history", () => {
    const { doc, versions } = buildHistory();
    const loaded = new LoroDoc();
    loaded.import(doc.export({ mode: "shallow-snapshot", frontiers: versions[0]! }));
    const expected = valuesAt(doc, versions);
    expect(valuesAt(loaded, versions)).toEqual(expected);
  });
});

/**
 * `styled-text.blob` is a Rust snapshot (peer 1, `bold` expands after, `keep`
 * expands both ways). Versions, one commit each:
 * 0. `both`: insert "a", mark it `keep`.
 * 1. `both`: delete "a", leaving an empty both-expand pair.
 * 2. `both`: insert "b" at 0; Rust places it inside the pair, so it is kept.
 * 3. `after`: insert "hello".
 * 4. `after`: mark 1..3 `bold`.
 * 5. `after`: insert "X" at 4, after the end anchor of the mark.
 * 6. `after`: delete 0..1.
 * The expected file holds the Rust `toDelta()` of both texts at each version.
 */
interface StyledTextVersion {
  readonly frontiers: Frontiers;
  readonly both: Delta<string>[];
  readonly after: Delta<string>[];
}

const styledText = (): { bytes: Uint8Array; versions: StyledTextVersion[] } => ({
  bytes: new Uint8Array(
    readFileSync(new URL("./fixtures/rust/styled-text.blob", import.meta.url)),
  ),
  versions: JSON.parse(
    readFileSync(
      new URL("./fixtures/rust/styled-text.expected.json", import.meta.url),
      "utf8",
    ),
  ) as StyledTextVersion[],
});

// loro.js interprets Text operation positions without Rust style anchors, so
// replaying version 5 places "X" after "o". Every other state matches Rust.
const REPLAY_POSITION_GAP = 5;
const REPLAYED_AT_GAP: Delta<string>[] = [
  { insert: "h" },
  { insert: "el", attributes: { bold: true } },
  { insert: "loX" },
];

describe("checkout after a Rust rich-text snapshot import", () => {
  test("keeps snapshot styles and text that a history replay cannot reproduce", () => {
    const { bytes, versions } = styledText();
    const latest = versions.at(-1)!;
    for (const order of [
      [0, 1, 2, 3, 4, 5, 6],
      [6, 5, 4, 3, 2, 1, 0],
      [1, 6, 2, 5, 0, 3, 6, 4],
    ]) {
      const doc = new LoroDoc();
      doc.import(bytes);
      const mirror = new LoroDoc();
      mirror.configTextStyle({ keep: { expand: "both" }, bold: { expand: "after" } });
      mirror.getText("both").applyDelta(doc.getText("both").toDelta());
      mirror.getText("after").applyDelta(doc.getText("after").toDelta());
      doc.subscribe((batch: LoroEventBatch) => {
        mirror.applyDiff(batch.events.map(({ target, diff }) => [target, diff]));
      });
      const check = (expected: StyledTextVersion, index: number): void => {
        const both = doc.getText("both").toDelta();
        const after = doc.getText("after").toDelta();
        expect(both).toEqual(expected.both);
        expect(after).toEqual(
          index === REPLAY_POSITION_GAP ? REPLAYED_AT_GAP : expected.after,
        );
        expect(mirror.getText("both").toDelta()).toEqual(both);
        expect(mirror.getText("after").toDelta()).toEqual(after);
      };
      for (const index of order) {
        doc.checkout(versions[index]!.frontiers);
        check(versions[index]!, index);
      }
      doc.checkoutToLatest();
      check(latest, versions.length - 1);

      const again = new LoroDoc();
      again.import(doc.export({ mode: "snapshot" }));
      expect(again.getText("both").toDelta()).toEqual(latest.both);
      expect(again.getText("after").toDelta()).toEqual(latest.after);
    }
  });

  test("returns to the snapshot state after edits and imports elsewhere", () => {
    const { bytes, versions } = styledText();
    const latest = versions.at(-1)!;
    const doc = new LoroDoc();
    doc.import(bytes);
    const remote = new LoroDoc();
    remote.import(bytes);
    remote.setPeerId(2);
    remote.getMap("other").set("remote", 1);
    remote.commit();

    doc.checkout(versions[4]!.frontiers);
    doc.checkoutToLatest();
    doc.getMap("other").set("local", 1);
    doc.commit();
    doc.checkout(versions[2]!.frontiers);
    doc.import(remote.export({ mode: "update", from: doc.oplogVersion() }));
    doc.checkoutToLatest();
    expect(doc.getText("both").toDelta()).toEqual(latest.both);
    expect(doc.getText("after").toDelta()).toEqual(latest.after);
    expect(doc.getMap("other").toJSON()).toEqual({ local: 1, remote: 1 });

    doc.getText("after").insert(0, "A");
    doc.commit();
    expect(doc.getText("after").toString()).toBe("AellXo");
    doc.checkout(versions[6]!.frontiers);
    expect(doc.getText("after").toDelta()).toEqual(latest.after);
  });

  test("exports the latest state from every detached checkout", () => {
    const { bytes, versions } = styledText();
    const latest = versions.at(-1)!;
    const doc = new LoroDoc();
    doc.import(bytes);
    for (const version of [...versions].reverse()) {
      doc.checkout(version.frontiers);
      const again = new LoroDoc();
      again.import(doc.export({ mode: "snapshot" }));
      expect(again.getText("both").toDelta()).toEqual(latest.both);
      expect(again.getText("after").toDelta()).toEqual(latest.after);
      expect(doc.getText("both").toDelta()).toEqual(version.both);
    }
  });

  test("keeps the snapshot state through a shallow export", () => {
    const { bytes, versions } = styledText();
    const latest = versions.at(-1)!;
    for (const read of [false, true]) {
      const doc = new LoroDoc();
      doc.import(bytes);
      if (read) doc.toJSON();
      const shallow = new LoroDoc();
      shallow.import(
        doc.export({ mode: "shallow-snapshot", frontiers: versions[3]!.frontiers }),
      );
      for (const replica of [doc, shallow]) {
        expect(replica.getText("both").toDelta()).toEqual(latest.both);
        expect(replica.getText("after").toDelta()).toEqual(latest.after);
      }
    }
  });

  test("diffs a Rust snapshot like a full-history document", () => {
    const { bytes, versions } = styledText();
    const doc = new LoroDoc();
    doc.import(bytes);
    const diff = doc.diff(versions[6]!.frontiers, versions[1]!.frontiers, false);
    expect(doc.getText("both").toDelta()).toEqual(versions[6]!.both);
    const mirror = new LoroDoc();
    mirror.configTextStyle({ keep: { expand: "both" }, bold: { expand: "after" } });
    mirror.getText("both").applyDelta(versions[6]!.both);
    mirror.getText("after").applyDelta(versions[6]!.after);
    mirror.applyDiff(diff);
    expect(mirror.getText("both").toDelta()).toEqual(versions[1]!.both);
    expect(mirror.getText("after").toDelta()).toEqual(versions[1]!.after);
  });
});

describe("checkout events after a lazy snapshot import", () => {
  test("reports an unread nested container change as its delta", () => {
    const build = (): { doc: LoroDoc; before: Frontiers } => {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const parent = doc.getMap("root").setContainer("parent", new LoroMap());
      const child = parent.setContainer("child", new LoroText());
      child.insert(0, "abc");
      doc.commit();
      const before = doc.frontiers();
      child.insert(0, "X");
      doc.commit();
      return { doc, before };
    };
    const { doc, before } = build();
    const bytes = doc.export({ mode: "snapshot" });
    const diffs = (read: boolean): unknown[] => {
      const loaded = new LoroDoc();
      loaded.import(bytes);
      if (read) loaded.toJSON();
      const events: unknown[] = [];
      loaded.subscribe((batch) => {
        for (const event of batch.events) events.push(event.diff);
      });
      loaded.checkout(before);
      expect(loaded.toJSON()).toEqual({ root: { parent: { child: "abc" } } });
      return events;
    };
    expect(diffs(false)).toEqual([{ type: "text", diff: [{ delete: 1 }] }]);
    expect(diffs(true)).toEqual(diffs(false));
  });

  test("rebuilds only the unread nested sequence that a checkout crosses", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getMap("root").setContainer("text", new LoroText());
    text.insert(0, "abc");
    doc.getMap("other").set("k", 1);
    doc.commit();
    const before = doc.frontiers();
    text.delete(1, 1);
    doc.commit();

    const loaded = new LoroDoc();
    loaded.import(doc.export({ mode: "snapshot" }));
    const resets = [loaded.getMap("root"), loaded.getMap("other")].map((container) =>
      vi.spyOn(container, "_reset"),
    );
    loaded.checkout(before);
    expect(loaded.toJSON()).toEqual({ root: { text: "abc" }, other: { k: 1 } });
    loaded.checkoutToLatest();
    expect(loaded.toJSON()).toEqual({ root: { text: "ac" }, other: { k: 1 } });
    expect(resets.every((reset) => reset.mock.calls.length === 0)).toBe(true);
  });

  test("exports unread nested containers after history is loaded", () => {
    const { doc, versions } = buildHistory();
    for (const load of [
      (loaded: LoroDoc) => loaded.getAllChanges(),
      (loaded: LoroDoc) => {
        loaded.checkout(versions[0]!);
        loaded.checkoutToLatest();
      },
    ]) {
      const loaded = new LoroDoc();
      loaded.import(doc.export({ mode: "snapshot" }));
      load(loaded);
      const again = new LoroDoc();
      again.import(loaded.export({ mode: "snapshot" }));
      expect(again.toJSON()).toEqual(doc.toJSON());
    }
  });
});
