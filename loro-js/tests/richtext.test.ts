
import { describe, expect, it } from "vitest";
import {
  Delta,
  Loro,
  setPanicHook,
} from "../src";

setPanicHook();

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
        }
      },
      {
        insert: " World!"
      }
    ] as Delta<string>[])
  })

  it("insert after emoji", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, "👨‍👩‍👦");
    text.insert(8, "a");
    expect(text.toString()).toBe("👨‍👩‍👦a")
  })

  it("emit event correctly", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.subscribe(doc, (event) => {
      if (event.diff.type == "text") {
        expect(event.diff.diff).toStrictEqual(
          [
            {
              insert: "Hello",
              attributes: {
                bold: true,
              }
            },
            {
              insert: " World!"
            }
          ] as Delta<string>[]
        )
      }
    });
    text.insert(0, "Hello World!");
    text.mark({ start: 0, end: 5 }, "bold", true);
  })

  it("emit event from merging doc correctly", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    let called = false;
    text.subscribe(doc, (event) => {
      if (event.diff.type == "text") {
        called = true;
        expect(event.diff.diff).toStrictEqual(
          [
            {
              insert: "Hello",
              attributes: {
                bold: true,
              }
            },
            {
              insert: " World!"
            }
          ] as Delta<string>[]
        )
      }
    });

    const docB = new Loro();
    const textB = docB.getText("text");
    textB.insert(0, "Hello World!");
    textB.mark({ start: 0, end: 5 }, "bold", true);
    doc.import(docB.exportFrom());
    expect(called).toBeTruthy();
  })
})
