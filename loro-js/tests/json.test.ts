import { describe, expect, expectTypeOf, it } from "vitest";
import { Loro, LoroMap, LoroTree, LoroTreeNode } from "../src";

it("json encoding", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, "123")
    const map = doc.getMap("map");
    const list = doc.getList("list");
    const tree = doc.getTree("tree");
    const subMap = map.setContainer("subMap", new LoroMap());
    subMap.set("foo", "bar");
    list.push("foo");
    list.push("🦜");
    const root = tree.createNode(undefined);
    const child = tree.createNode(root.id);
    child.data.set("tree", "abc");
    text.mark({start:0, end:3}, "bold", true);
    const json = doc.exportJSON();
    console.log(json);
    
})

it("json decoding", () => {
    const v15Json = `{
  "schema_version": 1,
  "start_vv": {},
  "end_vv": {
    "14944917281143706156": 12
  },
  "peers": [
    14944917281143706156
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
            "style": [
              "bold",
              true
            ],
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
const doc = new Loro();
doc.importJSON(v15Json);
console.log(doc.exportJSON());
})