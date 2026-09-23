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
} from "../bundler/index";

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
    });

    it("can fork a shallow snapshot", () => {
        const docA = new LoroDoc();
        const listA = docA.getList("list");
        listA.insert(0, "A");
        listA.insert(1, "B");
        listA.insert(2, "C");

        const bytes = docA.export({
            mode: "shallow-snapshot",
            frontiers: docA.oplogFrontiers(),
        });

        const docB = new LoroDoc();
        docB.import(bytes);

        const docC = docB.fork();
        expect(docC.toJSON()).toEqual(docB.toJSON());
    });

    it("can forkAt a shallow doc at or after its shallow root", () => {
        const docA = new LoroDoc();
        docA.setPeerId(1);
        docA.getText("text").insert(0, "Hello");
        docA.commit();
        const root = docA.oplogFrontiers();

        const docB = new LoroDoc();
        docB.import(docA.export({ mode: "shallow-snapshot", frontiers: root }));
        docB.setPeerId(2);
        docB.getText("text").insert(5, "!");
        docB.commit();
        const afterRoot = docB.oplogFrontiers();
        docB.getText("text").insert(6, "?");
        docB.commit();

        const atRoot = docB.forkAt(root);
        expect(atRoot.isShallow()).toBe(true);
        expect(atRoot.shallowSinceFrontiers()).toEqual(root);
        expect(atRoot.toJSON()).toEqual({ text: "Hello" });
        expect(docB.forkAt(afterRoot).toJSON()).toEqual({ text: "Hello!" });
        expect(() => docB.forkAt([{ peer: "1", counter: 0 }])).toThrow();
    });
});
