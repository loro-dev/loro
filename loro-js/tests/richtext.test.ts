import { describe, expect, it } from "vitest";
import { Delta, Loro, TextDiff } from "../src";
import { Cursor, OpId, PeerID, setDebug } from "loro-wasm";

describe("richtext", () => {
  it("mark", () => {
    const doc = new Loro();
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

  it("insert after emoji", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, "👨‍👩‍👦");
    text.insert(8, "a");
    expect(text.toString()).toBe("👨‍👩‍👦a");
  });

  it("emit event correctly", async () => {
    const doc = new Loro();
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
    const doc = new Loro();
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

    const docB = new Loro();
    const textB = docB.getText("text");
    textB.insert(0, "Hello World!");
    textB.mark({ start: 0, end: 5 }, "bold", true);
    doc.import(docB.exportFrom());
    await new Promise((r) => setTimeout(r, 1));
    expect(called).toBeTruthy();
  });

  it("Delete emoji", async () => {
    const doc = new Loro();
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
    const doc1 = new Loro();
    doc1.configTextStyle({
      link: { expand: "none" },
      bold: { expand: "after" },
    });
    const text1 = doc1.getText("text");
    const doc2 = new Loro();
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
    const doc = new Loro();
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
    const doc = new Loro();
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
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, "123");
    const pos0 = text.getCursor(0, 0);
    {
      const ans = doc.getCursorPos(pos0!);
      expect(ans.offset).toBe(0);
    }
    text.insert(0, "1");
    {
      const ans = doc.getCursorPos(pos0!);
      expect(ans.offset).toBe(1);
    }
  });

  it("Get and query cursor", () => {
    const doc = new Loro();
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
      const ans = doc.getCursorPos(pos0!);
      expect(ans.side).toBe(0);
      expect(ans.offset).toBe(0);
      expect(ans.update).toBeUndefined();
    }
    text.insert(0, "abc");
    const bytes = pos0!.encode();
    // Sending pos0 over the network
    const pos0decoded = Cursor.decode(bytes);
    const docA = new Loro();
    docA.import(doc.exportFrom());
    {
      const ans = docA.getCursorPos(pos0decoded!);
      expect(ans.side).toBe(0);
      expect(ans.offset).toBe(3);
      expect(ans.update).toBeUndefined();
    }

    // If "1" is removed from the text, the stable position should be updated
    text.delete(3, 1); // remove "1", "abc23"
    doc.commit();
    {
      const ans = doc.getCursorPos(pos0!);
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
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, "Hello");
    const pos3 = text.getCursor(3);
    text.mark({ start: 0, end: 2 }, "bold", true);
    const ans = doc.getCursorPos(pos3!);
    expect(ans.offset).toBe(3);
  });

  it("Insert cursed str", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, `“aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`);
  });

  it("Insert/delete by utf8 index", () => {
    const doc = new Loro();
    const text = doc.getText('t');
    text.insert(0, "你好");
    text.insertUtf8(3, "a");
    text.insertUtf8(7, "b");
    expect(text.toDelta()).toStrictEqual([
      { insert: "你a好b" },
    ]);
    text.deleteUtf8(3, 4);
    expect(text.toDelta()).toStrictEqual([
      { insert: "你b"},
    ]);
  });

  it("Slice", () => {
    const doc = new Loro();
    const text = doc.getText('t');
    text.insert(0, "你好");
    expect(text.slice(0, 1)).toStrictEqual("你");
  });

  it("Slice emoji", () => {
    const doc = new Loro();
    const text = doc.getText('t');
    text.insert(0, "😡😡😡");
    expect(text.slice(0, 2)).toStrictEqual("😡");
  });

  it("CharAt", () => {
    const doc = new Loro();
    const text = doc.getText('t');
    text.insert(0, "你好");
    expect(text.charAt(1)).toStrictEqual("好");
  });

  it("Splice", () => {
    const doc = new Loro();
    const text = doc.getText('t');
    text.insert(0, "你好");
    expect(text.splice(1, 1, "我")).toStrictEqual("好");
    expect(text.toString()).toStrictEqual("你我");
  });

  it("Text iter", () => {
    const doc = new Loro();
    const text = doc.getText('t');
    text.insert(0, "你好");
    let str = "";
    text.iter((s : string)=>{
      str = str + s;
      return true;
    });
    expect(text.toString(), "你好");
  });

  it("Text update", () => {
    const doc = new Loro();
    const text = doc.getText('t');
    text.insert(0, "Hello 😊Bro");
    text.update("Hello World Bro😊");
    expect(text.toString()).toStrictEqual("Hello World Bro😊");
  });
});
