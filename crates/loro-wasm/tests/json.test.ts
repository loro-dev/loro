import { describe, expect, it } from "vitest";
import {
  LoroDoc,
  LoroMap,
  LoroList,
  LoroText,
  TextOp,
  LoroTree,
} from "../bundler/index";

it("json encoding", () => {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  text.insert(0, "123");
  const map = doc.getMap("map");
  const list = doc.getList("list");
  const movableList = doc.getMovableList("movableList");
  const tree = doc.getTree("tree");
  const subMap = map.setContainer("subMap", new LoroMap());
  subMap.set("foo", "bar");
  list.push("foo");
  list.push("🦜");
  movableList.push("move list");
  movableList.push("🦜");
  movableList.move(1, 0);
  const root = tree.createNode(undefined);
  const child = tree.createNode(root.id);
  child.data.set("tree", "abc");
  text.mark({ start: 0, end: 3 }, "bold", true);
  const json = doc.exportJsonUpdates();
  // console.log(json.changes[0].ops);
  const doc2 = new LoroDoc();
  doc2.importJsonUpdates(json);
});

it("json decoding", () => {
  const v15Json = `{
    "schema_version": 1,
    "start_version": {},
    "peers": [
      "14944917281143706156"
    ],
    "changes": [
      {
        "id": "0@0",
        "timestamp": 0,
        "deps": [],
        "lamport": 0,
        "msg": null,
        "ops": [
          {
            "container": "cid:root-text:Text",
            "content": {
              "type": "insert",
              "pos": 0,
              "text": "123"
            },
            "counter": 0
          },
          {
            "container": "cid:root-map:Map",
            "content": {
              "type": "insert",
              "key": "subMap",
              "value": "🦜:cid:3@0:Map"
            },
            "counter": 3
          },
          {
            "container": "cid:3@0:Map",
            "content": {
              "type": "insert",
              "key": "foo",
              "value": "bar"
            },
            "counter": 4
          },
          {
            "container": "cid:root-list:List",
            "content": {
              "type": "insert",
              "pos": 0,
              "value": [
                "foo",
                "🦜"
              ]
            },
            "counter": 5
          },
          {
            "container": "cid:root-tree:Tree",
            "content": {
              "type": "move",
              "target": "7@0",
              "parent": null
            },
            "counter": 7
          },
          {
            "container": "cid:root-tree:Tree",
            "content": {
              "type": "move",
              "target": "8@0",
              "parent": "7@0"
            },
            "counter": 8
          },
          {
            "container": "cid:8@0:Map",
            "content": {
              "type": "insert",
              "key": "tree",
              "value": "abc"
            },
            "counter": 9
          },
          {
            "container": "cid:root-text:Text",
            "content": {
              "type": "mark",
              "start": 0,
              "end": 3,
              "style_key": "bold",
              "style_value": true,
              "info": 132
            },
            "counter": 10
          },
          {
            "container": "cid:root-text:Text",
            "content": {
              "type": "mark_end"
            },
            "counter": 11
          }
        ]
      }
    ]
  }`;
  const doc = new LoroDoc();
  doc.importJsonUpdates(v15Json);
  // console.log(doc.exportJsonUpdates());
});

it("test some type correctness", () => {
  const doc = new LoroDoc();
  doc.setPeerId(0);
  doc.getText("text").insert(0, "123");
  doc.commit();
  doc.getText("text").delete(2, 1);
  doc.getText("text").delete(1, 1);
  doc.getText("text").delete(0, 1);
  doc.commit();
  const updates = doc.exportJsonUpdates();
  expect(updates.start_version).toBeDefined();
  expect(updates.changes.length).toBe(1);
  expect(updates.changes[0].ops[0].content).toStrictEqual({
    type: "insert",
    pos: 0,
    text: "123",
  } as TextOp);
  expect(updates.changes[0].ops[1].content).toStrictEqual({
    type: "delete",
    pos: 2,
    len: -3,
    start_id: "0@0",
  } as TextOp);
});


describe("toJsonWithReplacer", () => {
  it("should work with basic values", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "123");
    const json = doc.toJsonWithReplacer((key, value) => {
      return value;
    });

    expect(json).toStrictEqual({
      text: "123",
    });
  });

  it("should handle multiple container types", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "Hello");
    doc.getMap("map").set("key", "value");
    doc.getList("list").push("item");

    const json = doc.toJsonWithReplacer((key, value) => value);

    expect(json).toStrictEqual({
      text: "Hello",
      map: { key: "value" },
      list: ["item"]
    });
  });

  it("should allow value transformation", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "Hello");
    text.mark({ start: 0, end: 2 }, "bold", true);

    const json = doc.toJsonWithReplacer((key, value) => {
      if (value instanceof LoroText) {
        return value.toDelta();
      }
      return value;
    });

    expect(json).toStrictEqual({
      text: [
        { insert: "He", attributes: { bold: true } },
        { insert: "llo" }
      ]
    });
  });

  it("should skip undefined values", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "Hello");
    doc.getMap("map").set("visible", "yes");
    doc.getMap("map").set("hidden", "no");

    const json = doc.toJsonWithReplacer((key, value) => {
      if (key === "hidden") return undefined;
      return value;
    });

    expect(json).toStrictEqual({
      text: "Hello",
      map: {
        visible: "yes"
      }
    });
  });

  it("should handle nested containers", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    const subMap = map.setContainer("subMap", new LoroMap());
    subMap.set("foo", "bar");

    const list = doc.getList("list");
    list.push("item1");
    list.push("item2");

    const json = doc.toJsonWithReplacer((key, value) => {
      if (value instanceof LoroMap || value instanceof LoroList) {
        return value;
      }
      return value;
    });

    expect(json).toStrictEqual({
      map: {
        subMap: {
          foo: "bar"
        }
      },
      list: ["item1", "item2"]
    });
  });

  it("tree with replacer", () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");
    const tree = doc.getTree("tree");
    const root = tree.createNode();
    root.data.set("name", "root");
    const text = root.data.setContainer("content", new LoroText());
    text.insert(0, "Hello");

    // Test case 1: Return shallow value for tree nodes
    const json1 = doc.toJsonWithReplacer((key, value) => {
      if (value instanceof LoroTree) {
        return value.getShallowValue();
      }

      return value;
    });

    expect(json1).toEqual({
      tree: [{
        id: "0@1",
        parent: null,
        index: 0,
        fractional_index: "80",
        meta: "cid:0@1:Map",
        children: []
      }]
    });

    // Test case 2: Custom handling of tree nodes and text
    const json2 = doc.toJsonWithReplacer((key, value) => {
      if (value instanceof LoroTree) {
        // Only return root node IDs
        return value.toJSON().map((node: any) => node.id);
      }
      if (value instanceof LoroText) {
        return value.toDelta();
      }
      return value;
    });

    expect(json2).toEqual({
      tree: ["0@1"]
    });

    // Test case 3: Transform tree node structure
    const json3 = doc.toJsonWithReplacer((_key, value) => {
      if (value instanceof LoroTree) {
        return value.toJSON().map((node: any) => ({
          nodeId: node.id,
          nodeData: node.meta
        }));
      }
      return value;
    });

    expect(json3).toEqual({
      tree: [{
        nodeId: "0@1",
        nodeData: {
          name: "root",
          content: "Hello"
        }
      }]
    });

    // Test case 4: Skip certain nodes based on condition
    const json4 = doc.toJsonWithReplacer((key, value) => {
      if (value instanceof LoroTree) {
        const nodes = value.toJSON();
        return nodes.filter((node: any) => node.meta.name !== "root");
      }
      return value;
    });

    expect(json4).toEqual({
      tree: []
    });
  });
});
