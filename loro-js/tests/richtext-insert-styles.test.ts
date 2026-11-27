import { describe, expect, test } from "vitest";

import { LoroDoc, LoroText } from "../src/index";

/**
 * A local insert takes, per key, what its displayed neighbours and their
 * expand settings imply, whatever anchors lie between them
 * (loro-dev/loro#1240). Same cases as Rust's
 * `crates/loro/tests/richtext_insert_styles.rs`.
 */
type Expand = "before" | "after" | "both" | "none";

const docWith = (peer: number, styles: Record<string, Expand>): LoroDoc => {
  const doc = new LoroDoc();
  doc.setPeerId(peer);
  doc.configTextStyle(
    Object.fromEntries(Object.entries(styles).map(([k, expand]) => [k, { expand }])),
  );
  return doc;
};

describe("local insert styles (#1240)", () => {
  test("shape 1: unmarked both-expanding highlight does not come back", () => {
    const doc = docWith(1, { hl: "both" });
    const t = doc.getText("t");
    t.insert(0, "ab");
    doc.commit();
    t.mark({ start: 0, end: 2 }, "hl", true);
    doc.commit();
    t.unmark({ start: 0, end: 2 }, "hl");
    doc.commit();
    t.insert(0, "X");
    t.insert(3, "Y");
    expect(t.toDelta()).toEqual([{ insert: "XabY" }]);
  });

  test("shape 1: typing where the only styled char was deleted is plain", () => {
    for (const expand of ["both", "none", "after", "before"] as const) {
      const doc = docWith(1, { hl: expand });
      const t = doc.getText("t");
      t.insert(0, "abc");
      t.mark({ start: 1, end: 2 }, "hl", true);
      doc.commit();
      t.delete(1, 1);
      t.insert(1, "X");
      expect(t.toDelta()).toEqual([{ insert: "aXc" }]);
    }
  });

  test("shape 2: left edge of an unstyled hole follows expand", () => {
    const run = (key: string, expand: Expand) => {
      const doc = docWith(1, { [key]: expand });
      const t = doc.getText("t");
      t.insert(0, "abc");
      doc.commit();
      t.mark({ start: 0, end: 3 }, key, true);
      doc.commit();
      t.unmark({ start: 1, end: 2 }, key);
      doc.commit();
      t.insert(1, "X");
      return t.toDelta();
    };
    expect(run("link", "none")).toEqual([
      { insert: "a", attributes: { link: true } },
      { insert: "Xb" },
      { insert: "c", attributes: { link: true } },
    ]);
    expect(run("bold", "after")).toEqual([
      { insert: "aX", attributes: { bold: true } },
      { insert: "b" },
      { insert: "c", attributes: { bold: true } },
    ]);
  });

  test("shape 3: split marks with one value behave like one mark", () => {
    for (const split of [true, false]) {
      const doc = docWith(1, { link: "none" });
      const t = doc.getText("t");
      t.insert(0, "ab");
      if (split) {
        t.mark({ start: 0, end: 1 }, "link", "u");
        t.mark({ start: 1, end: 2 }, "link", "u");
      } else {
        t.mark({ start: 0, end: 2 }, "link", "u");
      }
      doc.commit();
      t.insert(1, "X");
      expect(t.toDelta()).toEqual([{ insert: "aXb", attributes: { link: "u" } }]);
    }
  });

  test("shape 4: concurrent styles at one boundary, independent of anchor order", () => {
    const config = { bold: "after", sup: "before" } as const;
    const run = (k1: string, k2: string) => {
      const d1 = docWith(1, config);
      d1.getText("t").insert(0, "y");
      d1.commit();
      const d2 = docWith(2, config);
      d2.import(d1.export({ mode: "snapshot" }));
      d1.getText("t").mark({ start: 0, end: 1 }, k1, true);
      d1.commit();
      d2.getText("t").mark({ start: 0, end: 1 }, k2, true);
      d2.commit();
      d1.import(d2.export({ mode: "update", from: d1.oplogVersion() }));
      d1.getText("t").insert(0, "X");
      d1.commit();
      const d3 = docWith(3, config);
      d3.import(d1.export({ mode: "snapshot" }));
      expect(d3.getText("t").toDelta()).toEqual(d1.getText("t").toDelta());
      return d1.getText("t").toDelta();
    };
    const expected = [
      { insert: "X", attributes: { sup: true } },
      { insert: "y", attributes: { bold: true, sup: true } },
    ];
    expect(run("bold", "sup")).toEqual(expected);
    expect(run("sup", "bold")).toEqual(expected);
  });

  test("detached text follows the same rule", () => {
    const t = new LoroText();
    t.insert(0, "abc");
    t.mark({ start: 0, end: 3 }, "bold", true);
    t.unmark({ start: 1, end: 2 }, "bold");
    t.insert(1, "X");
    expect(t.toDelta()).toEqual([
      { insert: "aX", attributes: { bold: true } },
      { insert: "b" },
      { insert: "c", attributes: { bold: true } },
    ]);
  });
});
