import { describe, expect, test } from "vitest";

import {
  ContainerType,
  decodeChangeKeys,
  decodeChangesHeader,
  decodeChangesMetadata,
  decodeContainerArena,
  decodeDeleteStartIds,
  decodeEncodedOperations,
  decodePositionArena,
  encodeChangeKeys,
  encodeChangesHeader,
  encodeChangesMetadata,
  encodeContainerArena,
  encodeDeleteStartIds,
  encodeEncodedOperations,
  encodePositionArena,
} from "../src/codec/index";
import type {
  ChangesHeader,
  ChangesMetadata,
  ContainerId,
  EncodedDeleteStartIdRow,
  EncodedOperationRow,
} from "../src/codec/index";

describe("change block tables", () => {
  test("round trips a change header and metadata", () => {
    const header: ChangesHeader = {
      peer: 7n,
      peers: [7n, 9n],
      counters: [10, 12, 15],
      lengths: [2, 3],
      lamports: [20, 23],
      dependencies: [
        [{ peer: 9n, counter: 3 }],
        [
          { peer: 7n, counter: 11 },
          { peer: 9n, counter: 4 },
        ],
      ],
    };
    const encodedHeader = encodeChangesHeader(header);
    expect(
      decodeChangesHeader(encodedHeader, {
        changeCount: 2,
        counterStart: 10,
        counterLength: 5,
        lamportStart: 20,
        lamportLength: 6,
      }),
    ).toEqual(header);

    const metadata: ChangesMetadata = {
      timestamps: [-1n, 42n],
      commitMessages: [undefined, "commit 😀"],
    };
    expect(decodeChangesMetadata(encodeChangesMetadata(metadata), 2)).toEqual(metadata);
  });

  test.each([
    { counter: 0, dependencies: [] },
    {
      counter: 10,
      dependencies: [{ peer: 0xffff_ffff_ffff_ffffn, counter: 9 }],
    },
  ])(
    "round trips a single-change header at counter $counter",
    ({ counter, dependencies }) => {
      const peer = 0xffff_ffff_ffff_ffffn;
      const header: ChangesHeader = {
        peer,
        peers: [peer],
        counters: [counter, counter + 1],
        lengths: [1],
        lamports: [20],
        dependencies: [dependencies],
      };
      expect(
        decodeChangesHeader(encodeChangesHeader(header), {
          changeCount: 1,
          counterStart: counter,
          counterLength: 1,
          lamportStart: 20,
          lamportLength: 1,
        }),
      ).toEqual(header);
    },
  );

  test("round trips default single-change metadata", () => {
    const metadata: ChangesMetadata = {
      timestamps: [0n],
      commitMessages: [undefined],
    };
    expect(decodeChangesMetadata(encodeChangesMetadata(metadata), 1)).toEqual(metadata);
  });

  test("round trips keys and the container arena", () => {
    const keys = ["map", "key 😀"];
    const containers: ContainerId[] = [
      { kind: "root", name: "map", containerType: ContainerType.Map },
      {
        kind: "normal",
        peer: 9n,
        counter: 4,
        containerType: ContainerType.Text,
      },
    ];
    expect(decodeChangeKeys(encodeChangeKeys(keys))).toEqual(keys);
    expect(
      decodeContainerArena(
        encodeContainerArena(containers, [7n, 9n], keys),
        [7n, 9n],
        keys,
      ),
    ).toEqual(containers);
  });

  test("round trips operation and delete tables", () => {
    const operations: EncodedOperationRow[] = [
      { containerIndex: 0, property: 2, valueType: 11, length: 1 },
      { containerIndex: 1, property: -1, valueType: 9, length: 3 },
    ];
    const deletes: EncodedDeleteStartIdRow[] = [
      { peerIndex: 1n, counter: 4, length: -3n },
    ];
    expect(decodeEncodedOperations(encodeEncodedOperations(operations))).toEqual(
      operations,
    );
    expect(decodeDeleteStartIds(encodeDeleteStartIds(deletes))).toEqual(deletes);
  });

  test("round trips a single operation at numeric boundaries", () => {
    const operations: EncodedOperationRow[] = [
      {
        containerIndex: 0xffff_ffff,
        property: -0x8000_0000,
        valueType: 0xff,
        length: 0xffff_ffff,
      },
    ];
    expect(decodeEncodedOperations(encodeEncodedOperations(operations))).toEqual(
      operations,
    );
  });

  test("round trips one ASCII root container", () => {
    const keys = ["x".repeat(0x7f)];
    const containers: ContainerId[] = [
      { kind: "root", name: keys[0]!, containerType: ContainerType.Text },
    ];
    expect(decodeChangeKeys(encodeChangeKeys(keys))).toEqual(keys);
    expect(
      decodeContainerArena(encodeContainerArena(containers, [], keys), [], keys),
    ).toEqual(containers);
  });

  test("round trips the prefix-compressed position arena", () => {
    const positions = [
      Uint8Array.of(1, 2, 3),
      Uint8Array.of(1, 2, 4),
      Uint8Array.of(1, 2, 4, 255),
      Uint8Array.of(1),
      new Uint8Array(),
    ];
    expect(decodePositionArena(encodePositionArena(positions))).toEqual(positions);
    expect(decodePositionArena(new Uint8Array())).toEqual([]);
  });
});
