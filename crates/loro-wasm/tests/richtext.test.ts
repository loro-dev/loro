import { describe, expect, it } from "vitest";
import { Delta, LoroDoc, TextDiff, Cursor, OpId } from "../bundler/index";
import { expectDefined } from "./helpers";

describe("richtext", () => {
  it("mark", () => {
    const doc = new LoroDoc();
    doc.configTextStyle({
      bold: { expand: "after" },
      link: { expand: "before" },
    });
    const text = doc.getText("text");
    text.insert(0, "Hello World!");
    text.mark({ start: 0, end: 5 }, "bold", true);
    expect(text.toDelta()).toStrictEqual([
      {
        insert: "Hello",
        attributes: {
          bold: true,
        },
      },
      {
        insert: " World!",
      },
    ] as Delta<string>[]);
  });

  it("unmark noop when style key missing in span", () => {
    const doc = new LoroDoc();
    doc.configTextStyle({
      bold: { expand: "after" },
      italic: { expand: "after" },
    });
    const text = doc.getText("text");
    text.insert(0, "Hello");
    text.mark({ start: 1, end: 4 }, "bold", true);
    doc.commit();
    const beforeDelta = text.toDelta();
    const beforeVersion = doc.version().toJSON();

    // Unmark a key that doesn't exist in the span; should be noop
    text.unmark({ start: 0, end: 5 }, "italic");
    doc.commit();

    expect(text.toDelta()).toStrictEqual(beforeDelta);
    expect(doc.version().toJSON()).toStrictEqual(beforeVersion);
  });

  it("insert after emoji", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "👨‍👩‍👦");
    text.insert(8, "a");
    expect(text.toString()).toBe("👨‍👩‍👦a");
  });

  it("emit event correctly", async () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    let triggered = false;
    text.subscribe((e) => {
      for (const event of e.events) {
        if (event.diff.type == "text") {
          expect(event.diff.diff).toStrictEqual([
            {
              insert: "Hello",
              attributes: {
                bold: true,
              },
            },
            {
              insert: " World!",
            },
          ] as Delta<string>[]);
          triggered = true;
        }
      }
    });
    text.insert(0, "Hello World!");
    text.mark({ start: 0, end: 5 }, "bold", true);
    doc.commit();
    await new Promise((r) => setTimeout(r, 1));
    expect(triggered).toBeTruthy();
  });

  it("emit event from merging doc correctly", async () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    let called = false;
    text.subscribe((event) => {
      if (event.events[0].diff.type == "text") {
        called = true;
        expect(event.events[0].diff.diff).toStrictEqual([
          {
            insert: "Hello",
            attributes: {
              bold: true,
            },
          },
          {
            insert: " World!",
          },
        ] as Delta<string>[]);
      }
    });

    const docB = new LoroDoc();
    const textB = docB.getText("text");
    textB.insert(0, "Hello World!");
    textB.mark({ start: 0, end: 5 }, "bold", true);
    doc.import(docB.export({ mode: "update" }));
    await new Promise((r) => setTimeout(r, 1));
    expect(called).toBeTruthy();
  });

  it("Delete emoji", async () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "012345👨‍👩‍👦6789");
    doc.commit();
    text.mark({ start: 0, end: 18 }, "bold", true);
    doc.commit();
    expect(text.toDelta()).toStrictEqual([
      {
        insert: "012345👨‍👩‍👦6789",
        attributes: { bold: true },
      },
    ]);
    text.delete(6, 8);
    doc.commit();
    expect(text.toDelta()).toStrictEqual([
      {
        insert: "0123456789",
        attributes: { bold: true },
      },
    ]);
  });

  it("apply delta", async () => {
    const doc1 = new LoroDoc();
    doc1.configTextStyle({
      link: { expand: "none" },
      bold: { expand: "after" },
    });
    const text1 = doc1.getText("text");
    const doc2 = new LoroDoc();
    doc2.configTextStyle({
      link: { expand: "none" },
      bold: { expand: "after" },
    });
    const text2 = doc2.getText("text");
    text1.subscribe((event) => {
      for (const containerDiff of event.events) {
        const e = containerDiff.diff as TextDiff;
        text2.applyDelta(e.diff);
      }
    });
    text1.insert(0, "foo");
    text1.mark({ start: 0, end: 3 }, "link", true);
    doc1.commit();
    await new Promise((r) => setTimeout(r, 1));
    expect(text2.toDelta()).toStrictEqual(text1.toDelta());
    text1.insert(3, "baz");
    doc1.commit();
    await new Promise((r) => setTimeout(r, 1));
    expect(text2.toDelta()).toStrictEqual([
      { insert: "foo", attributes: { link: true } },
      { insert: "baz" },
    ]);
    expect(text2.toDelta()).toStrictEqual(text1.toDelta());
    text1.mark({ start: 2, end: 5 }, "bold", true);
    doc1.commit();
    await new Promise((r) => setTimeout(r, 1));
    expect(text2.toDelta()).toStrictEqual(text1.toDelta());
  });

  it("custom richtext type", async () => {
    const doc = new LoroDoc();
    doc.configTextStyle({
      myStyle: {
        expand: "none",
      },
    });
    const text = doc.getText("text");
    text.insert(0, "foo");
    text.mark({ start: 0, end: 3 }, "myStyle", 123);
    expect(text.toDelta()).toStrictEqual([
      { insert: "foo", attributes: { myStyle: 123 } },
    ]);

    expect(() => {
      text.mark({ start: 0, end: 3 }, "unknownStyle", 2);
    }).toThrowError();

    expect(() => {
      // default style config should be overwritten
      text.mark({ start: 0, end: 3 }, "bold", 2);
    }).toThrowError();
  });

  it("allow overlapped styles", () => {
    const doc = new LoroDoc();
    doc.configTextStyle({
      comment: { expand: "none" },
    });
    const text = doc.getText("text");
    text.insert(0, "The fox jumped.");
    text.mark({ start: 0, end: 7 }, "comment:alice", "Hi");
    text.mark({ start: 4, end: 14 }, "comment:bob", "Jump");
    expect(text.toDelta()).toStrictEqual([
      {
        insert: "The ",
        attributes: { "comment:alice": "Hi" },
      },
      {
        insert: "fox",
        attributes: { "comment:alice": "Hi", "comment:bob": "Jump" },
      },
      {
        insert: " jumped",
        attributes: { "comment:bob": "Jump" },
      },
      {
        insert: ".",
      },
    ]);
  });

  it("Cursor example", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "123");
    const pos0 = text.getCursor(0, 0);
    {
      const ans = expectDefined(doc.getCursorPos(pos0!), "cursor pos missing");
      expect(ans.offset).toBe(0);
    }
    text.insert(0, "1");
    {
      const ans = expectDefined(doc.getCursorPos(pos0!), "cursor pos missing");
      expect(ans.offset).toBe(1);
    }
  });

  it("Get cursor only at valid UTF-16 boundaries", () => {
    for (const value of ["😀a", "a😀b", "ab😀", "😀e\u0301🚀"]) {
      const doc = new LoroDoc();
      const text = doc.getText("text");
      text.insert(0, value);
      doc.commit();

      const boundaries = new Set<number>();
      let offset = 0;
      for (const scalar of value) {
        boundaries.add(offset);
        offset += scalar.length;
      }

      for (let pos = 0; pos < text.length; pos++) {
        const cursor = text.getCursor(pos);
        if (boundaries.has(pos)) {
          expect(cursor, `${JSON.stringify(value)} at ${pos}`).toBeDefined();
          expect(doc.getCursorPos(cursor!)?.offset).toBe(pos);
        } else {
          expect(cursor, `${JSON.stringify(value)} at ${pos}`).toBeUndefined();
        }
      }

      const end = text.getCursor(text.length);
      expect(doc.getCursorPos(end!)?.offset).toBe(text.length);
      expect(end?.side()).toBe(1);
      const pastEnd = text.getCursor(text.length + 10);
      expect(doc.getCursorPos(pastEnd!)?.offset).toBe(text.length);
      expect(pastEnd?.side()).toBe(1);
    }

    const emptyDoc = new LoroDoc();
    const emptyCursor = emptyDoc.getText("text").getCursor(0);
    expect(emptyDoc.getCursorPos(emptyCursor!)?.offset).toBe(0);
  });

  it("Get cursor for a non-BMP atom across peers and remote edits", () => {
    const first = new LoroDoc();
    first.setPeerId("1");
    first.getText("text").insert(0, "a");
    first.commit();

    const second = new LoroDoc();
    second.setPeerId("2");
    second.import(first.export({ mode: "update" }));
    second.getText("text").insert(1, "😀");
    second.commit();

    const third = new LoroDoc();
    third.setPeerId("3");
    third.import(second.export({ mode: "update" }));
    third.getText("text").insert(3, "b");
    third.commit();

    const doc = new LoroDoc();
    doc.setPeerId("4");
    doc.import(third.export({ mode: "update" }));
    const text = doc.getText("text");
    expect(text.toString()).toBe("a😀b");

    const cursors = [-1, 0, 1].map((side) => {
      const cursor = text.getCursor(1, side as -1 | 0 | 1);
      expect(cursor).toBeDefined();
      expect(cursor?.side()).toBe(side);
      expect(doc.getCursorPos(cursor!)?.offset).toBe(1);
      return cursor!;
    });
    expect(text.getCursor(2)).toBeUndefined();

    const remote = new LoroDoc();
    remote.setPeerId("5");
    remote.import(doc.export({ mode: "update" }));
    remote.getText("text").insert(1, "x");
    remote.commit();
    doc.import(remote.export({ mode: "update" }));

    expect(text.toString()).toBe("ax😀b");
    for (const [index, cursor] of cursors.entries()) {
      const resolved = doc.getCursorPos(cursor);
      expect(resolved?.offset).toBe(2);
      expect(resolved?.side).toBe(index - 1);
    }
  });

  it("Get and query cursor", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    doc.setPeerId("1");
    text.insert(0, "123");
    const posEnd = text.getCursor(10, 0);
    expect(posEnd!.containerId()).toBe("cid:root-text:Text");
    expect(posEnd!.side()).toBe(1);
    const pos0 = text.getCursor(0, 0);
    expect(pos0?.containerId()).toBe("cid:root-text:Text");
    // pos0 points to the first character, i.e. the id of '1'
    expect(pos0?.pos()).toStrictEqual({ peer: "1", counter: 0 } as OpId);
    {
      const ans = expectDefined(doc.getCursorPos(pos0!), "cursor pos missing");
      expect(ans.side).toBe(0);
      expect(ans.offset).toBe(0);
      expect(ans.update).toBeUndefined();
    }
    text.insert(0, "abc");
    const bytes = pos0!.encode();
    // Sending pos0 over the network
    const pos0decoded = Cursor.decode(bytes);
    const docA = new LoroDoc();
    docA.import(doc.export({ mode: "update" }));
    {
      const ans = expectDefined(
        docA.getCursorPos(pos0decoded!),
        "cursor pos missing",
      );
      expect(ans.side).toBe(0);
      expect(ans.offset).toBe(3);
      expect(ans.update).toBeUndefined();
    }

    // If "1" is removed from the text, the stable position should be updated
    text.delete(3, 1); // remove "1", "abc23"
    doc.commit();
    {
      const ans = expectDefined(doc.getCursorPos(pos0!), "cursor pos missing");
      expect(ans.side).toBe(-1);
      expect(ans.offset).toBe(3);
      expect(ans.update).toBeDefined(); // The update of the stable position should be returned
      // It points to "2" now so the pos should be { peer: "1", counter: 1 }
      expect(ans.update?.pos()).toStrictEqual({
        peer: "1",
        counter: 1,
      } as OpId);
      // Side should be -1 because "1" was at the left side of "2"
      expect(ans.update!.side()).toBe(-1);
      expect(ans.update?.containerId()).toBe("cid:root-text:Text");
    }
  });

  it("Styles should not affect cursor pos", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "Hello");
    const pos3 = text.getCursor(3);
    text.mark({ start: 0, end: 2 }, "bold", true);
    const ans = expectDefined(doc.getCursorPos(pos3!), "cursor pos missing");
    expect(ans.offset).toBe(3);
  });

  it("Insert cursed str", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, `“aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`);
  });

  it("Insert/delete by utf8 index", () => {
    const doc = new LoroDoc();
    const text = doc.getText("t");
    text.insert(0, "你好");
    text.insertUtf8(3, "a");
    text.insertUtf8(7, "b");
    expect(text.toDelta()).toStrictEqual([{ insert: "你a好b" }]);
    text.deleteUtf8(3, 4);
    expect(text.toDelta()).toStrictEqual([{ insert: "你b" }]);
  });

  it("Slice", () => {
    const doc = new LoroDoc();
    const text = doc.getText("t");
    text.insert(0, "你好");
    expect(text.slice(0, 1)).toStrictEqual("你");
  });

  it("Slice emoji", () => {
    const doc = new LoroDoc();
    const text = doc.getText("t");
    text.insert(0, "😡😡😡");
    expect(text.slice(0, 2)).toStrictEqual("😡");
  });

  it("CharAt", () => {
    const doc = new LoroDoc();
    const text = doc.getText("t");
    text.insert(0, "你好");
    expect(text.charAt(1)).toStrictEqual("好");
  });

  it("Splice", () => {
    const doc = new LoroDoc();
    const text = doc.getText("t");
    text.insert(0, "你好");
    expect(text.splice(1, 1, "我")).toStrictEqual("好");
    expect(text.toString()).toStrictEqual("你我");
  });

  it("Text iter", () => {
    const doc = new LoroDoc();
    const text = doc.getText("t");
    text.insert(0, "你好");
    let str = "";
    text.iter((s: string) => {
      str = str + s;
      return true;
    });
    expect(text.toString(), "你好");
  });

  it("Text update", () => {
    const doc = new LoroDoc();
    const text = doc.getText("t");
    text.insert(0, "Hello 😊Bro");
    text.update("Hello World Bro😊");
    expect(text.toString()).toStrictEqual("Hello World Bro😊");
  });

  it("Delta cache", async () => {
    const doc = new LoroDoc();
    const text = doc.getText("t");
    text.insert(0, "Hello 😊Bro");
    const updates = doc.export({ mode: "snapshot" });
    const docB = new LoroDoc();
    const textB = docB.getText("t");
    const promise = new Promise<void>((r, reject) => {
      textB.subscribe((_e) => {
        try {
          expect(textB.toDelta()).toStrictEqual(text.toDelta());
          r();
        } catch (e) {
          reject(e);
        }
      });
    });

    expect(textB.toDelta()).toStrictEqual([]);
    docB.import(updates);
    expect(textB.toDelta()).toStrictEqual(text.toDelta());
    await promise;
    expect(textB.toString()).toStrictEqual("Hello 😊Bro");
  });

  it("sliceDelta basic", () => {
    const doc = new LoroDoc();
    doc.configTextStyle({
      bold: { expand: "after" },
      italic: { expand: "before" },
    });
    const text = doc.getText("text");
    text.insert(0, "Hello World!");
    text.mark({ start: 0, end: 5 }, "bold", true);
    text.mark({ start: 6, end: 11 }, "italic", true);

    const delta = text.sliceDelta(0, 12);
    expect(delta).toStrictEqual([
      { insert: "Hello", attributes: { bold: true } },
      { insert: " " },
      { insert: "World", attributes: { italic: true } },
      { insert: "!" },
    ] as Delta<string>[]);

    const partialDelta = text.sliceDelta(1, 8);
    expect(partialDelta).toStrictEqual([
      { insert: "ello", attributes: { bold: true } },
      { insert: " " },
      { insert: "Wo", attributes: { italic: true } },
    ] as Delta<string>[]);

    const emptyDelta = text.sliceDelta(5, 5);
    expect(emptyDelta).toStrictEqual([] as Delta<string>[]);

    // Out of bounds
    try {
      text.sliceDelta(0, 100);
      expect.fail("Should throw error");
    } catch (e) {
      // Expected
    }
  });

  it("sliceDeltaUtf8 basic", () => {
    const doc = new LoroDoc();
    doc.configTextStyle({
      bold: { expand: "after" },
      italic: { expand: "before" },
    });
    const text = doc.getText("text");
    text.insert(0, "你好 World!");
    text.mark({ start: 0, end: 2 }, "bold", true);
    text.mark({ start: 3, end: 8 }, "italic", true);

    const deltaUtf8 = text.sliceDeltaUtf8(0, 13);
    expect(deltaUtf8).toStrictEqual([
      { insert: "你好", attributes: { bold: true } },
      { insert: " " },
      { insert: "World", attributes: { italic: true } },
      { insert: "!" },
    ] as Delta<string>[]);

    // Partial slice by UTF8
    const partialDeltaUtf8 = text.sliceDeltaUtf8(3, 10); // "好 World" (UTF8 indices)
    expect(partialDeltaUtf8).toStrictEqual([
      { insert: "好", attributes: { bold: true } },
      { insert: " " },
      { insert: "Wor", attributes: { italic: true } },
    ] as Delta<string>[]);

    // Empty slice by UTF8
    const emptyDeltaUtf8 = text.sliceDeltaUtf8(6, 6);
    expect(emptyDeltaUtf8).toStrictEqual([] as Delta<string>[]);

    // Out of bounds by UTF8
    try {
      text.sliceDeltaUtf8(0, 100);
      expect.fail("Should throw error");
    } catch (e) {
      // Expected
    }
  });

  it("sliceDelta with emojis and styles", () => {
    const doc = new LoroDoc();
    doc.configTextStyle({
      emoji: { expand: "none" },
    });
    const text = doc.getText("text");
    text.insert(0, "🚀Hello✨World🌍");
    // Correct UTF-16 indices:
    // 🚀: 0-2
    // Hello: 2-7
    // ✨: 7-8
    // World: 8-13
    // 🌍: 13-15
    text.mark({ start: 0, end: 2 }, "emoji", "rocket");
    text.mark({ start: 7, end: 8 }, "emoji", "sparkles");
    text.mark({ start: 13, end: 15 }, "emoji", "earth");

    const delta = text.sliceDelta(0, 15);
    expect(delta).toStrictEqual([
      { insert: "🚀", attributes: { emoji: "rocket" } },
      { insert: "Hello" },
      { insert: "✨", attributes: { emoji: "sparkles" } },
      { insert: "World" },
      { insert: "🌍", attributes: { emoji: "earth" } },
    ] as Delta<string>[]);

    const partialDelta = text.sliceDelta(2, 13);
    expect(partialDelta).toStrictEqual([
      { insert: "Hello" },
      { insert: "✨", attributes: { emoji: "sparkles" } },
      { insert: "World" },
    ] as Delta<string>[]);
  });

  it("should remove the style entry when applyDelta with style that contains null value", () => {
    const doc = new LoroDoc();
    doc
      .getText("text")
      .applyDelta([{ insert: "hello", attributes: { bold: true } }]);
    doc
      .getText("text")
      .applyDelta([{ retain: 2 }, { retain: 2, attributes: { bold: null } }]);
    expect(doc.getText("text").toDelta()).toStrictEqual([
      {
        insert: "he",
        attributes: { bold: true },
      },
      {
        insert: "ll",
      },
      {
        insert: "o",
        attributes: { bold: true },
      },
    ]);
  });
});
