import { describe, expect, it } from "vitest";
import {
    Delta,
    getType,
    ListDiff,
    Loro,
    LoroDoc,
    LoroEventBatch,
    LoroList,
    LoroMap,
    LoroText,
    MapDiff,
    TextDiff,
    UndoManager,
} from "../src";

describe("detached editing", () => {
    it("basic tests", async () => {
        const doc = new LoroDoc();
        doc.setPeerId(1);
        let string = ""
        doc.subscribe((batch) => {
            for (const e of batch.events) {
                if (e.diff.type === "text") {
                    let index = 0;
                    for (const op of e.diff.diff) {
                        if (op.retain != null) {
                            index += op.retain;
                        } else if (op.insert != null) {
                            string = string.slice(0, index) + op.insert + string.slice(index);
                            index += op.insert.length;
                        } else if (op.delete != null) {
                            string = string.slice(0, index) + string.slice(index + op.delete);
                        }
                    }
                } else {
                    throw new Error("Unexpected diff type");
                }
            }
        });
        doc.setDetachedEditing(true);
        doc.getText("text").insert(0, "Hello world!");
        doc.setDetachedEditing(true);
        doc.checkout([{ peer: "1", counter: 4 }]);
        expect(doc.peerId).not.toBe(1n);
        doc.setPeerId(0);
        doc.getText("text").insert(5, " alice!");
        doc.commit();
        expect(doc.getText("text").toString()).toBe("Hello alice!");
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(string).toBe("Hello alice!");
        expect(doc.frontiers()).not.toEqual(doc.oplogFrontiers());
        expect(doc.oplogVersion().toJSON()).not.toEqual(doc.version().toJSON());
        expect(doc.frontiers()).toEqual([{ peer: "0", counter: 6 }]);
        expect(doc.oplogFrontiers()).toEqual([{ peer: "1", counter: 11 }, { peer: "0", counter: 6 }]);
        expect(doc.version().toJSON()).toEqual(new Map([["1", 5], ["0", 7]]));
        expect(doc.oplogVersion().toJSON()).toEqual(new Map([["1", 12], ["0", 7]]));

        doc.checkoutToLatest();
        expect(doc.peerId).not.toBe(0n);
        expect(doc.getText("text").toString()).toBe("Hello alice! world!");
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(string).toBe("Hello alice! world!");
        expect(doc.version().toJSON()).toEqual(doc.oplogVersion().toJSON());
    });

    it("works with undo", async () => {
        const doc = new LoroDoc();
        const undo = new UndoManager(doc, { mergeInterval: 0 });
        doc.setPeerId(1);
        doc.getText("text").insert(0, "Hello");
        doc.commit();
        doc.getText("text").insert(5, " world!");
        doc.commit();
        undo.undo();
        expect(doc.getText("text").toString()).toBe("Hello");
        undo.redo();
        expect(doc.getText("text").toString()).toBe("Hello world!");

        doc.setDetachedEditing(true);
        doc.checkout([{ peer: "1", counter: 4 }]);
        expect(!undo.canUndo());
        expect(!undo.canRedo());
        doc.getText("text").insert(5, " alice!");
        doc.commit();
        expect(undo.canUndo());
        expect(!undo.canRedo());
        undo.undo();
        expect(doc.getText("text").toString()).toBe("Hello");
        expect(!undo.canUndo());
        expect(undo.canRedo());
        undo.redo();
        expect(doc.getText("text").toString()).toBe("Hello alice!");
    });
})
