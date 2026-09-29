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
const legacy = json("expected.json") as Record<string, string | number>;

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

    test("resolves a 0.2 end cursor one character earlier, like Rust", () => {
      expect(legacy["cursor-emoji"]).toBe(4);
      const owner = read((doc) => doc.import(fixture("cursor-emoji.snapshot.blob")));
      const cursor = runtime.Cursor.decode(fixture("cursor-emoji.cursor.blob"));
      expect(owner.getCursorPos(cursor)?.offset).toBe(2);
      const fresh = owner.getText("c").getCursor(4, 1)!;
      expect(owner.getCursorPos(fresh)?.offset).toBe(4);
    });
  },
);
