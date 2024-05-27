import { describe, expect, expectTypeOf, it } from "vitest";
import { Container, Loro, LoroMap, LoroTree, LoroTreeNode } from "../src";

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