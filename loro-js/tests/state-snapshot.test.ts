import { describe, expect, test } from "vitest";

import {
  ContainerType,
  decodeContainerStateSnapshot,
  decodeContainerStateWrapper,
  decodeStateSnapshotStore,
  encodeContainerStateSnapshot,
  encodeContainerStateWrapper,
  encodePositionArena,
  encodeStateSnapshotStore,
  unknownContainerType,
  type ContainerStateSnapshot,
  type ContainerStateWrapper,
  type MapStateSnapshot,
  type StateSnapshotStore,
} from "../src/codec/index";

const peer = 0x0102_0304_0506_0708n;

describe("container state snapshot codec", () => {
  test("canonicalizes and round trips map state", () => {
    const state: MapStateSnapshot = {
      kind: ContainerType.Map,
      values: [
        ["b", { type: "bool", value: true }],
        ["a", { type: "i64", value: 1n }],
      ],
      deletedKeys: ["d", "c"],
      peers: [peer],
      metadata: [
        { key: "d", peerIndex: 0n, lamport: 4n },
        { key: "b", peerIndex: 0n, lamport: 2n },
        { key: "a", peerIndex: 0n, lamport: 1n },
        { key: "c", peerIndex: 0n, lamport: 3n },
      ],
    };
    expect(
      decodeContainerStateSnapshot(
        ContainerType.Map,
        encodeContainerStateSnapshot(state),
      ),
    ).toEqual({
      ...state,
      values: [...state.values].reverse(),
      deletedKeys: [...state.deletedKeys].reverse(),
      metadata: [...state.metadata].sort((left, right) =>
        left.key.localeCompare(right.key),
      ),
    });
  });

  test("round trips list, text, tree, movable-list, counter and unknown state", () => {
    const states: ContainerStateSnapshot[] = [
      {
        kind: ContainerType.List,
        values: [{ type: "string", value: "a" }, { type: "null" }],
        peers: [peer],
        ids: [
          { peerIndex: 0n, counter: 0, lamportSub: 0 },
          { peerIndex: 0n, counter: 1, lamportSub: 1 },
        ],
      },
      {
        kind: ContainerType.Text,
        text: "a😀",
        peers: [peer],
        spans: [
          { peerIndex: 0n, counter: 0, lamportSub: 0, length: 2 },
          { peerIndex: 0n, counter: 2, lamportSub: 0, length: 0 },
        ],
        keys: ["bold"],
        marks: [{ keyIndex: 0, value: { type: "bool", value: true }, info: 1 }],
      },
      {
        kind: ContainerType.Tree,
        peers: [peer],
        nodes: [
          {
            peerIndex: 0n,
            counter: 3,
            parentIndexPlusTwo: 0n,
            lastSetPeerIndex: 0n,
            lastSetCounter: 3,
            lastSetLamportSub: 0,
            fractionalIndexIndex: 0,
          },
        ],
        positions: [Uint8Array.of(1, 2, 3)],
        reserved: new Uint8Array(),
      },
      {
        kind: ContainerType.MovableList,
        values: [{ type: "i64", value: 1n }],
        peers: [peer],
        items: [
          {
            invisibleListItems: 0n,
            positionIdEqualsElementId: true,
            elementIdEqualsLastSetId: true,
          },
          {
            invisibleListItems: 0n,
            positionIdEqualsElementId: false,
            elementIdEqualsLastSetId: false,
          },
        ],
        listItemIds: [{ peerIndex: 0n, counter: 4, lamportSub: 0 }],
        elementIds: [{ peerIndex: 0n, lamport: 4 }],
        lastSetIds: [{ peerIndex: 0n, lamport: 5 }],
      },
      { kind: ContainerType.Counter, bits: 0x3ff0_0000_0000_0000n },
      { kind: unknownContainerType(19), payload: Uint8Array.of(1, 2, 3) },
    ];

    for (const state of states) {
      expect(
        decodeContainerStateSnapshot(state.kind, encodeContainerStateSnapshot(state)),
      ).toEqual(state);
    }
  });

  test("round trips empty tree position arena in its non-empty Rust encoding", () => {
    const state: ContainerStateSnapshot = {
      kind: ContainerType.Tree,
      peers: [],
      nodes: [],
      positions: [],
      reserved: new Uint8Array(),
    };
    expect(encodePositionArena([], { encodeEmpty: true }).length).toBeGreaterThan(0);
    expect(
      decodeContainerStateSnapshot(
        ContainerType.Tree,
        encodeContainerStateSnapshot(state),
      ),
    ).toEqual(state);
  });
});

describe("state wrapper and store codec", () => {
  const state: MapStateSnapshot = {
    kind: ContainerType.Map,
    values: [["answer", { type: "i64", value: 42n }]],
    deletedKeys: [],
    peers: [peer],
    metadata: [{ key: "answer", peerIndex: 0n, lamport: 1n }],
  };
  const wrapper: ContainerStateWrapper = {
    containerType: ContainerType.Map,
    depth: 1n,
    parent: undefined,
    state,
  };

  test("round trips a wrapper", () => {
    expect(decodeContainerStateWrapper(encodeContainerStateWrapper(wrapper))).toEqual(
      wrapper,
    );
  });

  test("preserves absent and empty stores", () => {
    for (const store of [{ kind: "absent" }, { kind: "empty" }] as const) {
      expect(decodeStateSnapshotStore(encodeStateSnapshotStore(store))).toEqual(store);
    }
  });

  test("round trips an SSTable store", () => {
    const store: StateSnapshotStore = {
      kind: "sstable",
      frontiers: [{ peer, counter: 0 }],
      containers: [
        {
          id: { kind: "root", name: "map", containerType: ContainerType.Map },
          wrapper,
        },
      ],
    };
    expect(
      decodeStateSnapshotStore(encodeStateSnapshotStore(store, { compression: "none" })),
    ).toEqual(store);
  });
});
