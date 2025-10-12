import { expect, test } from "bun:test";
import { LoroDoc, LoroMap, LoroText } from "../bundler";

test("basic", () => {
  const doc = new LoroDoc();
  doc.getText("text").insert(0, "Hello, world!");
  expect(doc.getText("text").toString()).toBe("Hello, world!");
});

test("fork when detached", () => {
  const doc: LoroDoc = new LoroDoc();
  doc.setPeerId("0");
  doc.getText("text").insert(0, "Hello, world!");
  doc.checkout([{ peer: "0", counter: 5 }]);
  const newDoc = doc.fork();
  newDoc.setPeerId("1");
  newDoc.getText("text").insert(6, " Alice!");
  // ┌───────────────┐     ┌───────────────┐
  // │    Hello,     │◀─┬──│     world!    │
  // └───────────────┘  │  └───────────────┘
  //                    │
  //                    │  ┌───────────────┐
  //                    └──│     Alice!    │
  //                       └───────────────┘
  doc.import(newDoc.export({ mode: "update" }));
  doc.checkoutToLatest();
  console.log(doc.getText("text").toString()); // "Hello, world! Alice!"
});

test("isDeleted", () => {
  const doc = new LoroDoc();
  const list = doc.getList("list");
  expect(list.isDeleted()).toBe(false);
  const tree = doc.getTree("root");
  const node = tree.createNode(undefined, undefined);
  const containerBefore = node.data.setContainer("container", new LoroMap());
  containerBefore.set("A", "B");
  tree.delete(node.id);
  const containerAfter = doc.getContainerById(containerBefore.id) as LoroMap;
  expect(containerAfter.isDeleted()).toBe(true);
});

test("toJsonWithReplacer", () => {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  text.insert(0, "Hello");
  text.mark({ start: 0, end: 2 }, "bold", true);

  // Use delta to represent text
  // @ts-ignore - allow returning delta representation
  const json = doc.toJsonWithReplacer((key, value) => {
    if (value instanceof LoroText) {
      return value.toDelta();
    }

    return value;
  });
});
