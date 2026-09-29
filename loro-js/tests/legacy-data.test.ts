import { readFileSync } from "node:fs";

import { describe, expect, test } from "vitest";

import { Cursor, LoroDoc, type JsonSchema } from "../src/index";
import { loadRustReference } from "./support/rust-reference";

// Written by loro.js 0.2.1 (scripts/write-legacy-fixtures.mjs). Later versions
// read them the way Rust does; README.md "Upgrading from 0.2" documents the
// differences.
const fixtureUrl = (name: string): URL =>
  new URL(`./fixtures/loro-js-0.2/${name}`, import.meta.url);
const fixture = (name: string): Uint8Array =>
  new Uint8Array(readFileSync(fixtureUrl(name)));
const json = (name: string): unknown =>
  JSON.parse(readFileSync(fixtureUrl(name), "utf8"));
const legacy = json("expected.json") as Record<string, unknown>;

interface Runtime {
  readonly LoroDoc: typeof LoroDoc;
  readonly Cursor: typeof Cursor;
}

const rust = loadRustReference();
const runtimes: [string, Runtime][] = [["loro.js", { LoroDoc, Cursor }]];
if (rust !== undefined) runtimes.push(["loro-crdt", rust]);

describe.each(runtimes)(
  "documents written by loro.js 0.2, read by %s",
  (name, runtime) => {
    const read = (load: (doc: LoroDoc) => void): LoroDoc => {
      const doc = new runtime.LoroDoc();
      load(doc);
      return doc;
    };

    test("orders concurrent inserts from history like Rust", () => {
      expect(legacy["text-concurrent"]).toBe("bé9a");
      const fromUpdate = read((doc) =>
        doc.import(fixture("text-concurrent.update.blob")),
      );
      expect(fromUpdate.getText("t").toString()).toBe("béa9");
      const fromJson = read((doc) =>
        doc.importJsonUpdates(json("text-concurrent.json") as JsonSchema),
      );
      expect(fromJson.getText("t").toString()).toBe("béa9");
    });

    test("keeps a snapshot's current text but reads its history like Rust", () => {
      const fromSnapshot = read((doc) =>
        doc.import(fixture("text-concurrent.snapshot.blob")),
      );
      expect(fromSnapshot.getText("t").toString()).toBe("bé9a");
      const history = read((doc) => doc.import(fromSnapshot.export({ mode: "update" })));
      expect(history.getText("t").toString()).toBe("béa9");
      // State and history disagree here, so the runtimes' forks differ: loro.js
      // replays the history, Rust copies the state.
      const fork = fromSnapshot.fork();
      expect(fork.getText("t").toString()).toBe(name === "loro.js" ? "béa9" : "bé9a");
    });

    test("reads rich-text positions with Rust's anchors", () => {
      expect(legacy["richtext-positions"]).toEqual([
        { insert: "a", attributes: { bold: true } },
        { insert: "cXd" },
      ]);
      const styled = (load: (doc: LoroDoc) => void): LoroDoc =>
        read((doc) => {
          doc.configTextStyle({ bold: { expand: "after" } });
          load(doc);
        });
      // The ops 0.2 wrote left the mark's anchors out of their positions.
      const rustReading = [
        { insert: "bX", attributes: { bold: true } },
        { insert: "cd" },
      ];
      const fromUpdate = styled((doc) =>
        doc.import(fixture("richtext-positions.update.blob")),
      );
      expect(fromUpdate.getText("t").toDelta()).toEqual(rustReading);
      const fromJson = styled((doc) =>
        doc.importJsonUpdates(json("richtext-positions.json") as JsonSchema),
      );
      expect(fromJson.getText("t").toDelta()).toEqual(rustReading);
      // A snapshot keeps the 0.2 state, but its history reads like Rust.
      const fromSnapshot = styled((doc) =>
        doc.import(fixture("richtext-positions.snapshot.blob")),
      );
      expect(fromSnapshot.getText("t").toDelta()).toEqual(legacy["richtext-positions"]);
      const history = styled((doc) =>
        doc.import(fromSnapshot.export({ mode: "update" })),
      );
      expect(history.getText("t").toDelta()).toEqual(rustReading);
      // State and history disagree, so the runtimes' forks differ: loro.js
      // replays the history, Rust copies the state.
      const fork = fromSnapshot.fork();
      fork.configTextStyle({ bold: { expand: "after" } });
      expect(fork.getText("t").toDelta()).toEqual(
        name === "loro.js" ? rustReading : legacy["richtext-positions"],
      );
    });

    // Rust shows text that matches neither reading after this round trip.
    test.skipIf(name !== "loro.js")(
      "keeps a 0.2 snapshot's state through checkouts",
      () => {
        const doc = read((created) => {
          created.configTextStyle({ bold: { expand: "after" } });
          created.import(fixture("richtext-positions.snapshot.blob"));
        });
        // The checkout finds that the replay differs from the state, so the text
        // keeps the state (#1126's unreplayable fallback) and shows older
        // versions approximately.
        const [first] = doc.frontiers();
        doc.checkout([{ peer: first!.peer, counter: 3 }]);
        doc.checkoutToLatest();
        expect(doc.getText("t").toDelta()).toEqual(legacy["richtext-positions"]);
      },
    );

    test("resolves a 0.2 cursor by its Unicode position", () => {
      expect(legacy["cursor-emoji"]).toBe(4);
      const owner = read((doc) => doc.import(fixture("cursor-emoji.snapshot.blob")));
      const cursor = runtime.Cursor.decode(fixture("cursor-emoji.cursor.blob"));
      expect(owner.getCursorPos(cursor)?.offset).toBe(2);
      const fresh = owner.getText("c").getCursor(4, 1)!;
      expect(owner.getCursorPos(fresh)?.offset).toBe(4);
    });
  },
);
