import { describe, expect, expectTypeOf, it } from "vitest";
import {
    Container,
    getType,
    isContainer,
    LoroDoc,
    LoroList,
    LoroMap,
    LoroText,
    LoroTree,
} from "../src";

describe("gc", () => {
    it("should export gc snapshot", () => {
        const doc = new LoroDoc();
        doc.setPeerId(1);
        doc.getList("list").insert(0, "A");
        doc.getList("list").insert(1, "B");
        doc.getList("list").insert(2, "C");
        const bytes = doc.export({ mode: "shallow-snapshot", frontiers: doc.oplogFrontiers() });
        const newDoc = new LoroDoc();
        newDoc.import(bytes);
        expect(newDoc.toJSON()).toEqual(doc.toJSON());

        doc.getList("list").delete(1, 1); // Delete "B"
        doc.getMap("map").set("key", "value"); // Add a new key-value pair to a map

        const updatedBytes = doc.export({ mode: "update", from: newDoc.version() });
        newDoc.import(updatedBytes);
        expect(newDoc.toJSON()).toEqual(doc.toJSON());
    });

    it("cannot import outdated updates", () => {
        const doc = new LoroDoc();
        doc.setPeerId(1);
        doc.getList("list").insert(0, "A");

        const docB = doc.fork();
        const v = docB.version();
        docB.getList("list").insert(1, "C");
        const updates = docB.export({ mode: "update", from: v });

        doc.getList("list").insert(1, "B");
        doc.getList("list").insert(2, "C");
        doc.commit();
        const bytes = doc.export({ mode: "shallow-snapshot", frontiers: doc.oplogFrontiers() });
        const gcDoc = new LoroDoc();
        gcDoc.import(bytes);

        expect(() => gcDoc.import(updates)).toThrow();
    })
});
