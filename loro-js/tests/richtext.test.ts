import { describe, expect, it } from "vitest";
import { Delta, Loro, TextDiff } from "../src";
import { setDebug } from "loro-wasm";

describe("richtext", () => {
  it("mark", () => {
    const doc = new Loro();
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
    const doc = new Loro();
    const text = doc.getText("text");
    const doc2 = new Loro();
    const text2 = doc2.getText("text");
    text.subscribe(doc, (event) => {
      const e = event.diff as TextDiff;
      text2.applyDelta(e.diff);
    });
    text.insert(0, "foo");
    text.mark({ start: 0, end: 3 }, "link", true);
    doc.commit();
    text.insert(3, "baz");
    doc.commit();
    await new Promise((r) => setTimeout(r, 1));
    expect(text2.toDelta()).toStrictEqual([{ insert: 'foo', attributes: { link: true } }, { insert: 'baz' }]);
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
    const text = doc.getText("text");
    text.insert(0, "Hello world!");
    text.mark({ start: 0, end: 5 }, "comment:alice", "Hi");
    text.mark({ start: 4, end: 11 }, "comment:bob", "I'm a comment!");
    expect(text.toDelta()).toStrictEqual([
      {
        insert: "Hell", attributes: { "comment:alice": "Hi" },
      },
      {
        insert: "o", attributes: { "comment:alice": "Hi", "comment:bob": "I'm a comment!" },
      },
      {
        insert: " world", attributes: { "comment:bob": "I'm a comment!" },
      },
      {
        insert: "!",
      }
    ])
  })
});
