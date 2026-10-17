import { expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

type Nested = number | Nested[] | { a: Nested };
function nested(depth: number): Nested {
  let value: Nested = 42;
  for (let i = 0; i < depth; i++) {
    value = i % 2 === 0 ? [value] : { a: value };
  }
  return value;
}

function source(depth: number) {
  const doc = new LoroDoc();
  doc.configTextStyle({ deep: { expand: "after" }, dead: { expand: "after" } });
  const text = doc.getText("text");
  text.insert(0, "hello!");
  text.mark({ start: 0, end: 5 }, "deep", nested(depth));
  text.mark({ start: 5, end: 6 }, "dead", nested(depth));
  text.delete(5, 1);
  doc.getMap("unrelated").set("ok", true);
  doc.commit();
  return doc;
}

for (const depth of [128, 129, 200]) {
  it(`reads ${depth}-level marks from snapshot and shallow bytes`, () => {
    const doc = source(depth);
    const snapshots = [
      doc.export({ mode: "snapshot" }),
      doc.export({ mode: "shallow-snapshot", frontiers: doc.frontiers() }),
    ];
    for (const bytes of snapshots) {
      const other = new LoroDoc();
      other.import(bytes);
      const text = other.getText("text");
      expect(text.toString()).toBe("hello");
      let value: unknown = text.toDelta()[0].attributes?.deep;
      for (let i = depth - 1; i >= 0; i--) {
        value =
          i % 2 === 0 ? (value as unknown[])[0] : (value as { a: unknown }).a;
      }
      expect(value).toBe(42);
      expect(other.getMap("unrelated").get("ok")).toBe(true);
      other.getMap("unrelated").set("edit", 1);
      expect(other.export({ mode: "snapshot" }).length).toBeGreaterThan(0);
      other.free();
    }
    doc.free();
  });
}

it("still rejects deep object JSON updates without poisoning the target doc", () => {
  const doc = source(129);
  const target = new LoroDoc();
  expect(() => target.importJsonUpdates(doc.exportJsonUpdates())).toThrow(
    /nesting depth/,
  );
  target.getMap("unrelated").set("edit", 1);
  expect(target.getMap("unrelated").get("edit")).toBe(1);
  expect(target.export({ mode: "snapshot" }).length).toBeGreaterThan(0);
  target.free();
  doc.free();
});
