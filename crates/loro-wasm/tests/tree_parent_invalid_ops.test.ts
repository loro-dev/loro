import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// A tree create/move whose parent node is not in the op's causal history used to be
// applied on the import fast path. With a subscriber the event computation then
// panicked inside the doc locks, which traps the WASM instance ("RuntimeError:
// unreachable") and leaves the doc dead. Such imports must throw a readable error and
// leave the doc unchanged and usable. See context/tree-parent-validation.md.

/** Peer 1 creates the root node `0@1` of tree "tree". */
function base(): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.getTree("tree").createNode();
  doc.commit();
  return doc;
}

function victim(subscribed: boolean): { doc: LoroDoc; events: () => number } {
  const doc = new LoroDoc();
  doc.setPeerId(3);
  doc.import(base().export({ mode: "snapshot" }));
  let events = 0;
  if (subscribed) {
    doc.subscribe(() => {
      events += 1;
    });
  }
  return { doc, events: () => events };
}

const TREE = "cid:root-tree:Tree";

function forged(
  peers: string[],
  id: string,
  content: unknown,
  counter: number,
) {
  return JSON.stringify({
    schema_version: 1,
    start_version: {},
    peers,
    changes: [
      {
        id,
        timestamp: 0,
        deps: ["0@0"],
        lamport: 1,
        msg: null,
        ops: [{ container: TREE, content, counter }],
      },
    ],
  });
}

/** Peer 1 extends its own head with a create under a parent that never existed. */
const SAME_PEER_JSON = forged(
  ["1", "4242"],
  "1@0",
  { type: "create", target: "1@0", parent: "999999@1", fractional_index: "80" },
  1,
);
/** The same change from a new peer. */
const NEW_PEER_JSON = forged(
  ["1", "2", "4242"],
  "0@1",
  { type: "create", target: "0@1", parent: "999999@2", fractional_index: "80" },
  0,
);
/** Peer 1 moves its root node under the unknown parent. */
const MOVE_JSON = forged(
  ["1", "4242"],
  "1@0",
  { type: "move", target: "0@0", parent: "999999@1", fractional_index: "80" },
  1,
);
/** Peer 1 deletes a node that never existed. */
const DELETE_JSON = forged(
  ["1", "4242"],
  "1@0",
  { type: "delete", target: "999999@1" },
  1,
);

// Binary forms of the changes above, exported by a build that accepted them: the
// fixed build cannot produce them through its public API.
const SAME_PEER_UPDATE =
  "bG9ybwAAAAAAAAAAAAAAAK6zV9IABFUBAQEBARkCAQAAAAAAAACSEAAAAAAAAAABAQAAAAAABQEAAAEABgEEAQMAAAUEdHJlZQkBAgIBAAMBAYAOAQQCAQACAQACARACAQEACAABAAABv4Q9";
const NEW_PEER_UPDATE =
  "bG9ybwAAAAAAAAAAAAAAAFQzRM4ABF8AAQEBASMDAgAAAAAAAACSEAAAAAAAAAEAAAAAAAAAAQEBAQIBAAAAAAUBAAABAAYBBAEDAAAFBHRyZWUJAQICAQADAQGADgEEAgEAAgEAAgEQAgEBAAgAAAAAAb+EPQ==";
const MOVE_UPDATE =
  "bG9ybwAAAAAAAAAAAAAAAGeuUC4ABFUBAQEBARkCAQAAAAAAAACSEAAAAAAAAAABAQAAAAAABQEAAAEABgEEAQMAAAUEdHJlZQkBAgIBAAMBAYAOAQQCAQACAQACARACAQEACAAAAAABv4Q9";
const SAME_PEER_SNAPSHOT =
  "bG9ybwAAAAAAAAAAAAAAACjg3kIAA5wAAABMT1JPAAACAAIBGAIBAAAAAAAAAJIQAAAAAAAAAQEAAAAAAAUBAAABAAYBBAEDAAAFBHRyZWUJAQICAQADAQGADgEEAgQAAgQAAgQQAgQBAAwAAAABAAEAAAG/hD0AAgBmcgEBAgACAHZ2AQEEAABYAGAAAwAUw9z+AQAAAAUAAAAMAAAAAAAAAAABAAAAAAACAHZ2lijVh3kAAACMAAAATE9STwAEIk0YYECCTAAAAPkFAAIBAAR0cmVlBAAAAAkEAAEAAAAUAEEABgCDIQBBAwEAAR4AcAAAAAQCAgE6ABIFBwAFBgDwBAkBAgIBAAMBAYAAAAANACEAAwAAAAAAsBcRCAEAAAAFAAAADQAAAQAAAAAAAAAAAAAAAQYAgwR0cmVlcZYma2QAAAAAAAAA";

function bytes(base64: string): Uint8Array {
  return Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
}

async function expectRejected(
  subscribed: boolean,
  run: (doc: LoroDoc) => void,
) {
  const { doc, events } = victim(subscribed);
  const value = doc.toJSON();
  const vv = doc.oplogVersion().toJSON();
  let error: unknown;
  try {
    run(doc);
  } catch (e) {
    error = e;
  }
  // LoroError reaches JS as its message.
  expect(error).toBeDefined();
  expect(String(error)).toMatch(/Decode error/);
  expect(String(error)).not.toMatch(/unreachable/);
  expect(doc.toJSON()).toEqual(value);
  expect(doc.oplogVersion().toJSON()).toEqual(vv);
  await Promise.resolve();
  expect(events()).toBe(0);

  // Still usable: reads, local edits, export, and a valid remote update.
  const tree = doc.getTree("tree");
  expect(tree.getNodes().length).toBe(1);
  const local = tree.createNode(tree.roots()[0].id);
  doc.commit();
  expect(tree.has(local.id)).toBe(true);
  doc.export({ mode: "snapshot" });
  doc.export({ mode: "update" });
  const remote = new LoroDoc();
  remote.setPeerId(9);
  remote.import(doc.export({ mode: "snapshot" }));
  remote.getTree("tree").createNode(local.id);
  remote.commit();
  doc.import(remote.export({ mode: "update", from: doc.oplogVersion() }));
  expect(doc.toJSON()).toEqual(remote.toJSON());
}

describe("tree ops naming a parent outside their causal history", () => {
  for (const subscribed of [true, false]) {
    const label = subscribed ? "subscribed" : "unsubscribed";

    it(`throws on a binary update, ${label}`, async () => {
      for (const update of [SAME_PEER_UPDATE, NEW_PEER_UPDATE, MOVE_UPDATE]) {
        await expectRejected(subscribed, (doc) => doc.import(bytes(update)));
      }
    });

    it(`throws on JSON updates, ${label}`, async () => {
      for (const json of [
        SAME_PEER_JSON,
        NEW_PEER_JSON,
        MOVE_JSON,
        DELETE_JSON,
      ]) {
        await expectRejected(subscribed, (doc) => doc.importJsonUpdates(json));
      }
    });

    it(`throws on a snapshot, ${label}`, async () => {
      await expectRejected(subscribed, (doc) =>
        doc.import(bytes(SAME_PEER_SNAPSHOT)),
      );
    });
  }

  it("still accepts a parent created earlier in the same change and by another peer", () => {
    const peer1 = base();
    const peer2 = new LoroDoc();
    peer2.setPeerId(2);
    peer2.import(peer1.export({ mode: "snapshot" }));
    const tree2 = peer2.getTree("tree");
    const a = tree2.createNode(tree2.roots()[0].id);
    const b = tree2.createNode(a.id);
    peer2.commit();

    const { doc } = victim(true);
    doc.import(peer2.export({ mode: "update", from: doc.oplogVersion() }));
    const tree = doc.getTree("tree");
    const c = tree.createNode(b.id);
    doc.commit();
    peer2.import(doc.export({ mode: "update", from: peer2.oplogVersion() }));
    expect(tree.has(c.id)).toBe(true);
    expect(peer2.toJSON()).toEqual(doc.toJSON());
  });
});
