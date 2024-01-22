import { describe, expect, it } from "vitest";
import { Delta, Loro, TextDiff } from "../src";
import { setDebug } from "loro-wasm";

describe("richtext", () => {
  it("mark", () => {
    const doc = new Loro();
    doc.configTextStyle({
      bold: { expand: "after" },
      link: { expand: "before" }
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

  it("emit event correctly", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.subscribe(doc, (event) => {
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
      }
    });
    text.insert(0, "Hello World!");
    text.mark({ start: 0, end: 5 }, "bold", true);
  });

  it("emit event from merging doc correctly", async () => {
    const doc = new Loro();
    const text = doc.getText("text");
    let called = false;
    text.subscribe(doc, (event) => {
      if (event.diff.type == "text") {
        called = true;
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
    })
    const text1 = doc1.getText("text");
    const doc2 = new Loro();
    doc2.configTextStyle({
      link: { expand: "none" },
      bold: { expand: "after" },
    })
    const text2 = doc2.getText("text");
    text1.subscribe(doc1, (event) => {
      const e = event.diff as TextDiff;
      text2.applyDelta(e.diff);
    });
    text1.insert(0, "foo");
    text1.mark({ start: 0, end: 3 }, "link", true);
    doc1.commit();
    await new Promise((r) => setTimeout(r, 1));
    expect(text2.toDelta()).toStrictEqual(text1.toDelta());
    text1.insert(3, "baz");
    doc1.commit();
    await new Promise((r) => setTimeout(r, 1));
    expect(text2.toDelta()).toStrictEqual([{ insert: 'foo', attributes: { link: true } }, { insert: 'baz' }]);
    expect(text2.toDelta()).toStrictEqual(text1.toDelta());
    text1.mark({ start: 2, end: 5 }, "bold", true);
    doc1.commit();
    await new Promise((r) => setTimeout(r, 1));
    expect(text2.toDelta()).toStrictEqual(text1.toDelta());
  })

  it("custom richtext type", async () => {
    const doc = new Loro();
    doc.configTextStyle({
      myStyle: {
        expand: "none",
      }
    })
    const text = doc.getText("text");
    text.insert(0, "foo");
    text.mark({ start: 0, end: 3 }, "myStyle", 123);
    expect(text.toDelta()).toStrictEqual([{ insert: 'foo', attributes: { myStyle: 123 } }]);

    expect(() => {
      text.mark({ start: 0, end: 3 }, "unknownStyle", 2);
    }).toThrowError()

    expect(() => {
      // default style config should be overwritten
      text.mark({ start: 0, end: 3 }, "bold", 2);
    }).toThrowError()
  })

  it("allow overlapped styles", () => {
    const doc = new Loro();
    doc.configTextStyle({
      comment: { expand: "none", }
    })
    const text = doc.getText("text");
    text.insert(0, "The fox jumped.");
    text.mark({ start: 0, end: 7 }, "comment:alice", "Hi");
    text.mark({ start: 4, end: 14 }, "comment:bob", "Jump");
    expect(text.toDelta()).toStrictEqual([
      {
        insert: "The ", attributes: { "comment:alice": "Hi" },
      },
      {
        insert: "fox", attributes: { "comment:alice": "Hi", "comment:bob": "Jump" },
      },
      {
        insert: " jumped", attributes: { "comment:bob": "Jump" },
      },
      {
        insert: ".",
      }
    ])
  })
});
