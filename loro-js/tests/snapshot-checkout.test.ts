import { readFileSync } from "node:fs";

import { describe, expect, test, vi } from "vitest";

import {
  LoroDoc,
  LoroList,
  LoroMap,
  LoroText,
  type Delta,
  type Diff,
  type Frontiers,
  type LoroEventBatch,
} from "../src/index";
import { SequenceIndex } from "../src/runtime/sequence-index";

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
 *
 * loro.js does not count style anchors in Text positions, so replaying either
 * text differs from its snapshot state. Both keep the snapshot state; an older
 * version is approximate when it needs text deleted before the snapshot (it
 * is exact at versions 1, 2, and 6), but the latest state never changes.
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

const EXACT_VERSIONS = new Set([1, 2, 6]);

const mirrorOf = (doc: LoroDoc): LoroDoc => {
  const mirror = new LoroDoc();
  mirror.configTextStyle({ keep: { expand: "both" }, bold: { expand: "after" } });
  mirror.getText("both").applyDelta(doc.getText("both").toDelta());
  mirror.getText("after").applyDelta(doc.getText("after").toDelta());
  doc.subscribe((batch: LoroEventBatch) => {
    mirror.applyDiff(batch.events.map(({ target, diff }) => [target, diff]));
  });
  return mirror;
};

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
      const mirror = mirrorOf(doc);
      const exact: { index: number; both: unknown; after: unknown }[] = [];
      const check = (index: number): void => {
        const both = doc.getText("both").toDelta();
        const after = doc.getText("after").toDelta();
        if (EXACT_VERSIONS.has(index)) exact.push({ index, both, after });
        expect(mirror.getText("both").toDelta()).toEqual(both);
        expect(mirror.getText("after").toDelta()).toEqual(after);
      };
      for (const index of order) {
        doc.checkout(versions[index]!.frontiers);
        check(index);
      }
      doc.checkoutToLatest();
      check(versions.length - 1);
      expect(exact).toEqual(
        exact.map(({ index }) => ({
          index,
          both: versions[index]!.both,
          after: versions[index]!.after,
        })),
      );

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
    }
    doc.checkoutToLatest();
    expect(doc.getText("both").toDelta()).toEqual(latest.both);
    expect(doc.getText("after").toDelta()).toEqual(latest.after);
  });

  test("keeps the latest state exact through detached imports and local edits", () => {
    const { bytes, versions } = styledText();
    const latest = versions.at(-1)!;
    const remote = new LoroDoc();
    remote.import(bytes);
    remote.setPeerId(2);
    remote.getText("after").insert(0, "A");
    remote.commit();
    const update = remote.export({ mode: "update", from: new LoroDoc().oplogVersion() });
    const withA: Delta<string>[] = [{ insert: "A" }, ...latest.after];
    for (const mode of ["import", "importBatch", "attached"] as const) {
      const doc = new LoroDoc();
      doc.import(bytes);
      const mirror = mirrorOf(doc);
      doc.checkout(versions[4]!.frontiers);
      doc.checkoutToLatest();
      if (mode !== "attached") doc.detach();
      if (mode === "importBatch") doc.importBatch([update]);
      else doc.import(update);
      doc.attach();
      expect(doc.getText("after").toDelta()).toEqual(withA);
      expect(mirror.getText("after").toDelta()).toEqual(withA);
      const again = new LoroDoc();
      again.import(doc.export({ mode: "snapshot" }));
      expect(again.getText("after").toDelta()).toEqual(withA);
    }

    const doc = new LoroDoc();
    doc.import(bytes);
    doc.checkout(versions[4]!.frontiers);
    doc.checkoutToLatest();
    doc.getText("after").insert(0, "A");
    doc.commit();
    doc.checkout(versions[4]!.frontiers);
    doc.checkout(versions[6]!.frontiers);
    expect(doc.getText("after").toDelta()).toEqual(latest.after);
    doc.checkoutToLatest();
    expect(doc.getText("after").toDelta()).toEqual(withA);
    const fork = doc.fork();
    expect(fork.getText("after").toDelta()).toEqual(withA);
    expect(doc.forkAt(versions[6]!.frontiers).getText("after").toDelta()).toEqual(
      latest.after,
    );
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

  test("forks an older version from the fork's own history", () => {
    const { bytes, versions } = styledText();
    const deltas = (doc: LoroDoc): unknown => [
      doc.getText("both").toDelta(),
      doc.getText("after").toDelta(),
    ];
    const doc = new LoroDoc();
    doc.import(bytes);
    doc.checkout(versions[4]!.frontiers);
    doc.checkoutToLatest();
    for (const index of [0, 3, 4, 5]) {
      const frontiers = versions[index]!.frontiers;
      doc.checkout(frontiers);
      const checkedOut = doc.fork();
      doc.checkoutToLatest();
      for (const fork of [doc.forkAt(frontiers), checkedOut]) {
        // The fork has no operation after its version, so its state is the
        // replay of its own operations: exports, imports, and local edits agree
        // with a document that only replays them.
        const replay = new LoroDoc();
        replay.configTextStyle({ keep: { expand: "both" }, bold: { expand: "after" } });
        replay.import(fork.export({ mode: "update" }));
        expect(deltas(fork)).toEqual(deltas(replay));
        fork.import(bytes);
        replay.import(bytes);
        expect(deltas(fork)).toEqual(deltas(replay));
        fork.getText("after").insert(0, "Q");
        fork.commit();
        replay.import(fork.export({ mode: "update", from: replay.oplogVersion() }));
        expect(deltas(fork)).toEqual(deltas(replay));
      }
    }
    // Before the insert next to the mark, the replay matches Rust.
    expect(doc.forkAt(versions[4]!.frontiers).getText("after").toDelta()).toEqual(
      versions[4]!.after,
    );
  });

  test("keeps the snapshot state when checking a replay throws", () => {
    const { bytes, versions } = styledText();
    const latest = versions.at(-1)!;
    const failOnce = (
      target: object,
      method: "visibleIdRuns" | "_swapState",
      call: number,
    ): (() => void) => {
      const original = (target as Record<string, (...args: unknown[]) => unknown>)[
        method
      ]!;
      let calls = 0;
      const spy = vi
        .spyOn(target as Record<string, (...args: unknown[]) => unknown>, method)
        .mockImplementation(function (this: unknown, ...args: unknown[]) {
          calls += 1;
          if (calls === call) throw new Error("injected");
          return original.apply(this, args);
        });
      return () => spy.mockRestore();
    };
    const failures: unknown[] = [];
    for (const operation of ["checkout", "diff"] as const) {
      for (const method of ["visibleIdRuns", "_swapState"] as const) {
        for (let call = 1; call <= 4; call += 1) {
          const doc = new LoroDoc();
          doc.import(bytes);
          const text = doc.getText("after");
          const restore = failOnce(
            method === "visibleIdRuns" ? SequenceIndex.prototype : text,
            method,
            call,
          );
          try {
            if (operation === "checkout") doc.checkout(versions[4]!.frontiers);
            else doc.diff(latest.frontiers, versions[4]!.frontiers, false);
          } catch (error) {
            failures.push({
              message: (error as Error).message,
              detached: doc.isDetached(),
              after: text.toDelta(),
            });
          } finally {
            restore();
          }
          // A retry and the export still see the snapshot state.
          doc.checkout(versions[4]!.frontiers);
          doc.checkoutToLatest();
          expect(text.toDelta()).toEqual(latest.after);
          const again = new LoroDoc();
          again.import(doc.export({ mode: "snapshot" }));
          expect(again.getText("after").toDelta()).toEqual(latest.after);
        }
      }
    }
    expect(failures.length).toBeGreaterThan(0);
    expect(failures).toEqual(
      failures.map(() => ({ message: "injected", detached: false, after: latest.after })),
    );
  });
});

/**
 * `rich-text-history.json`: for each seed, a Rust snapshot of 25 random Text
 * inserts, deletes, and marks (bold/link/keep/pre expand after/none/both/
 * before), four Rust updates from another peer that insert at the start, and
 * the snapshot's versions.
 */
interface RichTextHistory {
  readonly seed: number;
  readonly snapshot: string;
  readonly updates: readonly string[];
  readonly versions: readonly Frontiers[];
}

const decodeBase64 = (text: string): Uint8Array =>
  new Uint8Array(Buffer.from(text, "base64"));

const canonicalDelta = (delta: readonly Delta<string>[]): unknown =>
  delta.map((item) =>
    "attributes" in item && item.attributes !== undefined
      ? {
          ...item,
          attributes: Object.fromEntries(
            Object.entries(item.attributes).sort(([left], [right]) =>
              left < right ? -1 : left > right ? 1 : 0,
            ),
          ),
        }
      : item,
  );

describe("latest state of Rust rich text across checkouts", () => {
  const histories = JSON.parse(
    readFileSync(
      new URL("./fixtures/rust/rich-text-history.json", import.meta.url),
      "utf8",
    ),
  ) as RichTextHistory[];
  for (const history of histories) {
    test(`checkouts and imports keep the imported latest state (seed ${history.seed})`, () => {
      const snapshot = decodeBase64(history.snapshot);
      const updates = history.updates.map(decodeBase64);
      const reference = new LoroDoc();
      reference.import(snapshot);
      for (const update of updates) reference.import(update);
      const expected = canonicalDelta(reference.getText("t").toDelta());

      let state = history.seed;
      const random = (): number => {
        state = (state * 1_103_515_245 + 12_345) % 2_147_483_648;
        return state / 2_147_483_648;
      };
      for (const subscribed of [false, true]) {
        const doc = new LoroDoc();
        doc.import(snapshot);
        const mirror = new LoroDoc();
        mirror.configTextStyle({
          bold: { expand: "after" },
          link: { expand: "none" },
          keep: { expand: "both" },
          pre: { expand: "before" },
        });
        mirror.getText("t").applyDelta(doc.getText("t").toDelta());
        if (subscribed) {
          doc.subscribe((batch: LoroEventBatch) => {
            mirror.applyDiff(batch.events.map(({ target, diff }) => [target, diff]));
          });
        }
        const versions = [...history.versions];
        const mirrorMismatches: number[] = [];
        let next = 0;
        for (let step = 0; step < 16; step += 1) {
          const choice = random();
          if (choice < 0.5) {
            doc.checkout(versions[Math.floor(random() * versions.length)]!);
          } else if (choice < 0.65) {
            doc.checkoutToLatest();
          } else if (next < updates.length) {
            if (random() < 0.5) doc.detach();
            if (choice < 0.8) doc.import(updates[next++]!);
            else doc.importBatch([updates[next++]!]);
            versions.push(doc.oplogFrontiers());
          }
          if (
            subscribed &&
            JSON.stringify(canonicalDelta(mirror.getText("t").toDelta())) !==
              JSON.stringify(canonicalDelta(doc.getText("t").toDelta()))
          ) {
            mirrorMismatches.push(step);
          }
        }
        expect(mirrorMismatches).toEqual([]);
        while (next < updates.length) doc.import(updates[next++]!);
        doc.attach();
        expect(canonicalDelta(doc.getText("t").toDelta())).toEqual(expected);
        const again = new LoroDoc();
        again.import(doc.export({ mode: "snapshot" }));
        expect(canonicalDelta(again.getText("t").toDelta())).toEqual(expected);
      }
    });
  }
});

/**
 * `movable-moves.json` is a Rust history of MovableList `ml` (peer 1): insert
 * 0, 1, 2 (version 0); move(0, 2) (version 1, snapshot `moved`); insert(0, 9)
 * (version 2, snapshot `inserted`). `values` are Rust's values at each version.
 * `update` is peer 2's move(2, 0) and set(1, 7) on `moved`, and `merged` is
 * Rust's value after it.
 */
interface MovableMoves {
  readonly moved: string;
  readonly inserted: string;
  readonly update: string;
  readonly versions: readonly Frontiers[];
  readonly values: readonly (readonly number[])[];
  readonly merged: readonly number[];
}

const applyListDiff = (values: readonly unknown[], diff: Diff): unknown[] => {
  if (diff.type !== "list") throw new Error(`unexpected ${diff.type} diff`);
  const result: unknown[] = [];
  let index = 0;
  for (const item of diff.diff) {
    if ("retain" in item) {
      result.push(...values.slice(index, index + item.retain));
      index += item.retain;
    } else if ("delete" in item) {
      index += item.delete;
    } else {
      result.push(...item.insert);
    }
  }
  return [...result, ...values.slice(index)];
};

describe("checkout after a Rust MovableList snapshot with a move", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/rust/movable-moves.json", import.meta.url), "utf8"),
  ) as MovableMoves;
  const { versions, values } = fixture;
  const load = (snapshot = fixture.moved): LoroDoc => {
    const doc = new LoroDoc();
    doc.import(decodeBase64(snapshot));
    return doc;
  };

  // The hydrated state names a moved element by its Rust position id, so the
  // list takes the replay of its history, as on main.
  test("checks out, diffs, forks, and reverts to the version before the move", () => {
    const doc = load();
    const list = doc.getMovableList("ml");
    let mirror: unknown[] = list.toJSON();
    doc.subscribe((batch: LoroEventBatch) => {
      for (const event of batch.events) {
        if (event.target === list.id) mirror = applyListDiff(mirror, event.diff);
      }
    });
    for (const [frontiers, value] of [
      [versions[0]!, values[0]!],
      [versions[1]!, values[1]!],
      [versions[0]!, values[0]!],
    ] as const) {
      doc.checkout(frontiers);
      expect(list.toJSON()).toEqual(value);
      expect(mirror).toEqual(value);
    }
    doc.checkoutToLatest();
    expect(list.toJSON()).toEqual(values[1]);
    expect(mirror).toEqual(values[1]);

    const diffed = load();
    const diff = diffed
      .diff(versions[1]!, versions[0]!, false)
      .find(([id]) => id === diffed.getMovableList("ml").id);
    expect(applyListDiff(values[1]!, diff![1])).toEqual(values[0]);
    expect(diffed.getMovableList("ml").toJSON()).toEqual(values[1]);

    expect(load().forkAt(versions[0]!).getMovableList("ml").toJSON()).toEqual(values[0]);

    const reverted = load();
    reverted.revertTo(versions[0]!);
    reverted.commit();
    expect(reverted.getMovableList("ml").toJSON()).toEqual(values[0]);
    const again = new LoroDoc();
    again.import(reverted.export({ mode: "snapshot" }));
    expect(again.getMovableList("ml").toJSON()).toEqual(values[0]);
  });

  test("applies a later move by element after a checkout round trip", () => {
    const doc = load();
    doc.checkout(versions[0]!);
    doc.checkoutToLatest();
    doc.import(decodeBase64(fixture.update));
    expect(doc.getMovableList("ml").toJSON()).toEqual(fixture.merged);
  });

  test("moves by delta once the first checkout replayed it", () => {
    const doc = load();
    const list = doc.getMovableList("ml");
    doc.checkout(versions[0]!);
    doc.checkoutToLatest();
    const reset = vi.spyOn(list, "_reset");
    for (let round = 0; round < 3; round += 1) {
      doc.checkout(versions[0]!);
      expect(list.toJSON()).toEqual(values[0]);
      doc.checkoutToLatest();
      expect(list.toJSON()).toEqual(values[1]);
    }
    expect(reset).not.toHaveBeenCalled();
  });

  test("forks before the move from the fork's own history", () => {
    for (const roundTrip of [false, true]) {
      const doc = load(fixture.inserted);
      if (roundTrip) {
        doc.checkout(versions[0]!);
        doc.checkoutToLatest();
      }
      const fork = doc.forkAt(versions[0]!);
      expect(fork.getMovableList("ml").toJSON()).toEqual(values[0]);
      fork.import(decodeBase64(fixture.inserted));
      expect(fork.getMovableList("ml").toJSON()).toEqual(values[2]);
      expect(fork.oplogFrontiers()).toEqual(doc.oplogFrontiers());
    }
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
