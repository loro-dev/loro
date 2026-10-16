import { describe, expect, it } from "vitest";
import * as bundler from "../bundler/index";
import * as nodejs from "../nodejs/index";

const targets = {
  bundler: { createDoc: () => new bundler.LoroDoc(), Cursor: bundler.Cursor },
  nodejs: { createDoc: () => new nodejs.LoroDoc(), Cursor: nodejs.Cursor },
};

for (const [target, { Cursor, createDoc }] of Object.entries(targets)) {
  describe(`${target}: invalid cursor counters (#1178)`, () => {
    it.each(["01", "03", "7f", "ffffffff0f"])(
      "rejects zigzag counter %s and keeps the document usable",
      (counter) => {
        const doc = createDoc();
        doc.setPeerId(1n);
        const text = doc.getText("text");
        text.insert(0, "hello");
        doc.commit();
        const valid = text.getCursor(2)!;
        expect(Buffer.from(valid.encode()).toString("hex")).toBe(
          "010104000474657874000102",
        );

        // Exact issue bytes for -1, plus -2, -64 and i32::MIN.
        const bytes = Buffer.from(`0101${counter}000474657874000102`, "hex");
        expect(() => Cursor.decode(bytes)).toThrowError(
          "The original data was not well encoded",
        );
        expect(text.toString()).toBe("hello");
        expect(doc.getCursorPos(valid)?.offset).toBe(2);

        const replica = createDoc();
        replica.import(doc.export({ mode: "snapshot" }));
        expect(replica.getText("text").toString()).toBe("hello");
        replica.setPeerId(2n);
        replica.getText("text").insert(5, "!");
        replica.commit();
        doc.import(replica.export({ mode: "update" }));
        text.insert(6, "?");
        doc.commit();
        replica.import(doc.export({ mode: "update" }));
        expect(text.toString()).toBe("hello!?");
        expect(replica.toJSON()).toEqual(doc.toJSON());
      },
    );

    it("preserves valid cursors, empty cursors and unknown positive ids", () => {
      const doc = createDoc();
      doc.setPeerId(1n);
      const text = doc.getText("text");
      const emptyStart = Cursor.decode(text.getCursor(0, -1)!.encode());
      const emptyEnd = Cursor.decode(text.getCursor(0, 1)!.encode());
      text.insert(0, "hello");
      doc.commit();
      expect(doc.getCursorPos(emptyStart)?.offset).toBe(0);
      expect(doc.getCursorPos(emptyEnd)?.offset).toBe(5);
      for (const offset of [0, 2, 5]) {
        const cursor = Cursor.decode(text.getCursor(offset)!.encode());
        expect(doc.getCursorPos(cursor)?.offset).toBe(offset);
      }
      const unknown = Cursor.decode(
        Buffer.from("01017e000474657874000102", "hex"),
      );
      expect(unknown.pos()?.counter).toBe(63);
      expect(doc.getCursorPos(unknown)).toBeUndefined();
      expect(doc.export({ mode: "snapshot" }).length).toBeGreaterThan(0);
    });
  });
}
