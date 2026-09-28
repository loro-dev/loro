// Writes documents with loro.js 0.2 into tests/fixtures/loro-js-0.2, so that
// tests/legacy-data.test.ts pins how later versions read data written before
// loro.js adopted Rust's text semantics ("Upgrading from 0.2" in README.md).
//
//   npm install --prefix /tmp/loro-js-0.2 loro.js@0.2.1
//   node scripts/write-legacy-fixtures.mjs /tmp/loro-js-0.2/node_modules/loro.js/dist/index.js

import { mkdirSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const legacyEntry = process.argv[2];
if (legacyEntry === undefined) {
  throw new Error("pass the path of loro.js 0.2's dist/index.js");
}
const { LoroDoc } = await import(pathToFileURL(legacyEntry).href);

const directory = new URL("../tests/fixtures/loro-js-0.2/", import.meta.url);
mkdirSync(directory, { recursive: true });
const write = (name, bytes) => writeFileSync(new URL(name, directory), bytes);
const expected = {};

function doc(peer) {
  const created = new LoroDoc();
  created.setPeerId(peer);
  return created;
}

function sync(from, to) {
  to.import(from.export({ mode: "update" }));
}

// Concurrent inserts that 0.2 ordered differently from Rust: "a" (peer 3) and
// "é" (peer 2) both follow "b", and 0.2 put "a" after "9".
{
  const [peer1, peer2, peer3] = [1, 2, 3].map(doc);
  peer2.getText("t").insert(0, "9");
  peer2.commit();
  peer1.getText("t").insert(0, "b");
  peer1.commit();
  sync(peer1, peer2);
  peer2.getText("t").insert(1, "é");
  peer2.commit();
  sync(peer1, peer3);
  peer3.getText("t").insert(1, "a");
  peer3.commit();
  sync(peer3, peer2);
  write("text-concurrent.update.blob", peer2.export({ mode: "update" }));
  write("text-concurrent.snapshot.blob", peer2.export({ mode: "snapshot" }));
  write("text-concurrent.json", JSON.stringify(peer2.exportJsonUpdates(), null, 2));
  expected["text-concurrent"] = peer2.getText("t").toString();
}

// A cursor on the end of "ab😀": 0.2 encoded its UTF-16 position.
{
  const owner = doc(1);
  const text = owner.getText("c");
  text.insert(0, "ab😀");
  owner.commit();
  const cursor = text.getCursor(4, 1);
  write("cursor-emoji.snapshot.blob", owner.export({ mode: "snapshot" }));
  write("cursor-emoji.cursor.blob", cursor.encode());
  expected["cursor-emoji"] = owner.getCursorPos(cursor).offset;
}

// Marks whose anchors 0.2 left out of text positions: with bold on "ab" in
// "abcd", inserting X at 3 and deleting 1 gave "acXd" in 0.2 (and Rust), but
// the ops 0.2 wrote mean "bXcd" to Rust.
{
  const owner = doc(1);
  owner.configTextStyle({ bold: { expand: "after" } });
  const text = owner.getText("t");
  text.insert(0, "abcd");
  owner.commit();
  text.mark({ start: 0, end: 2 }, "bold", true);
  owner.commit();
  text.insert(3, "X");
  text.delete(1, 1);
  owner.commit();
  write("richtext-positions.update.blob", owner.export({ mode: "update" }));
  write("richtext-positions.snapshot.blob", owner.export({ mode: "snapshot" }));
  write("richtext-positions.json", JSON.stringify(owner.exportJsonUpdates(), null, 2));
  expected["richtext-positions"] = text.toDelta();
}

write("expected.json", `${JSON.stringify(expected, null, 2)}\n`);
