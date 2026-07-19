import { describe, expect, test } from "vitest";
import { ContainerType, decodeChangeKeys, decodeChangesHeader, decodeChangesMetadata, decodeContainerArena, decodeDeleteStartIds, decodeEncodedOperations, decodePositionArena, encodeChangeKeys, encodeChangesHeader, encodeChangesMetadata, encodeContainerArena, encodeDeleteStartIds, encodeEncodedOperations, encodePositionArena, } from "../src/codec/index";
describe("change block tables", () => {
    test("round trips a change header and metadata", () => {
        const header = {
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
        expect(decodeChangesHeader(encodedHeader, {
            changeCount: 2,
            counterStart: 10,
            counterLength: 5,
            lamportStart: 20,
            lamportLength: 6,
        })).toEqual(header);
        const metadata = {
            timestamps: [-1n, 42n],
            commitMessages: [undefined, "commit 😀"],
        };
        expect(decodeChangesMetadata(encodeChangesMetadata(metadata), 2)).toEqual(metadata);
    });
    test("round trips keys and the container arena", () => {
        const keys = ["map", "key 😀"];
        const containers = [
            { kind: "root", name: "map", containerType: ContainerType.Map },
            {
                kind: "normal",
                peer: 9n,
                counter: 4,
                containerType: ContainerType.Text,
            },
        ];
        expect(decodeChangeKeys(encodeChangeKeys(keys))).toEqual(keys);
        expect(decodeContainerArena(encodeContainerArena(containers, [7n, 9n], keys), [7n, 9n], keys)).toEqual(containers);
    });
    test("round trips operation and delete tables", () => {
        const operations = [
            { containerIndex: 0, property: 2, valueType: 11, length: 1 },
            { containerIndex: 1, property: -1, valueType: 9, length: 3 },
        ];
        const deletes = [
            { peerIndex: 1n, counter: 4, length: -3n },
        ];
        expect(decodeEncodedOperations(encodeEncodedOperations(operations))).toEqual(operations);
        expect(decodeDeleteStartIds(encodeDeleteStartIds(deletes))).toEqual(deletes);
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
