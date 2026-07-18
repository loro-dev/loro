import { describe, expect, test } from "vitest";

import {
  ContainerType,
  decodeChangeBlock,
  encodeChangeBlock,
  validateChangeBlock,
} from "../src/codec/index";
import type { DecodedChangeBlock } from "../src/codec/index";

describe("semantic change block codec", () => {
  test("round trips a map insert block", () => {
    const container = {
      kind: "root" as const,
      name: "root",
      containerType: ContainerType.Map,
    };
    const block: DecodedChangeBlock = {
      peers: [0x0102_0304_0506_0708n],
      keys: ["root", "a"],
      containers: [container],
      positions: [],
      changes: [
        {
          id: { peer: 0x0102_0304_0506_0708n, counter: 10 },
          timestamp: 1234n,
          dependencies: [],
          lamport: 100,
          message: undefined,
          operations: [
            {
              container,
              counter: 10,
              length: 1,
              content: {
                type: "map-insert",
                key: "a",
                value: { type: "i64", value: 10n },
              },
            },
          ],
        },
      ],
    };
    const encoded = encodeChangeBlock(block);
    expect(decodeChangeBlock(encoded)).toEqual(block);
    expect(validateChangeBlock(encoded)).toEqual({
      peer: 0x0102_0304_0506_0708n,
      counterStart: 10,
      counterEnd: 11,
    });
  });

  test("round trips deletes, movable-list operations and tree operations", () => {
    const peer = 7n;
    const list = {
      kind: "root" as const,
      name: "list",
      containerType: ContainerType.MovableList,
    };
    const tree = {
      kind: "root" as const,
      name: "tree",
      containerType: ContainerType.Tree,
    };
    const block: DecodedChangeBlock = {
      peers: [peer],
      keys: ["list", "tree"],
      containers: [list, tree],
      positions: [Uint8Array.of(1, 2)],
      changes: [
        {
          id: { peer, counter: 0 },
          timestamp: 0n,
          dependencies: [],
          lamport: 0,
          message: "ops",
          operations: [
            {
              container: list,
              counter: 0,
              length: 1,
              content: {
                type: "movable-list-insert",
                position: 0,
                values: [{ type: "string", value: "x" }],
              },
            },
            {
              container: list,
              counter: 1,
              length: 1,
              content: {
                type: "movable-list-delete",
                position: 0,
                length: 1n,
                startId: { peer, counter: 0 },
              },
            },
            {
              container: tree,
              counter: 2,
              length: 1,
              content: {
                type: "tree-create",
                subject: { peer, counter: 2 },
                parent: undefined,
                position: Uint8Array.of(1, 2),
              },
            },
            {
              container: tree,
              counter: 3,
              length: 1,
              content: {
                type: "tree-create",
                subject: { peer, counter: 3 },
                parent: { peer, counter: 2 },
                position: Uint8Array.of(1, 2),
              },
            },
            {
              container: tree,
              counter: 4,
              length: 1,
              content: { type: "tree-delete", subject: { peer, counter: 2 } },
            },
          ],
        },
      ],
    };
    const encoded = encodeChangeBlock(block);
    const decoded = decodeChangeBlock(encoded);
    expect(decoded.changes).toEqual(block.changes);
    expect(decoded.peers).toEqual(expect.arrayContaining([peer, 0xffff_ffff_ffff_ffffn]));
    expect(validateChangeBlock(encoded)).toEqual({
      peer,
      counterStart: 0,
      counterEnd: 5,
    });
  });
});
