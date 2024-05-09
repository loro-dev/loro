import { describe, expect, it } from "vitest";
import {
  Delta,
  getType,
  ListDiff,
  Loro,
  LoroEventBatch,
  LoroList,
  LoroMap,
  LoroText,
  MapDiff,
  TextDiff,
} from "../src";

import * as OLD from "loro-crdt-old";

// TODO: This is skip because we know it will fail for the current version as we've introduced BREAKING CHANGES on the serialization format
describe.skip("compatibility", () => {
  it("basic forward compatibility on exportFrom", () => {
    const docA = new Loro();
    docA.getText("text").insert(0, "123");
    docA.getMap("map").set("key", 123);
    docA.getMap("map").set("key", "123");
    docA.getList("list").insert(0, 1);
    docA.getList("list").insert(0, "1");
    const t = docA.getTree("tree");
    const node = t.createNode();
    t.createNode(node.id, 0);
    const bytes = docA.exportFrom();

    const docB = new OLD.Loro();
    docB.import(bytes);
    expect(docA.toJSON()).toStrictEqual(docB.toJSON());
  });

  it("basic forward compatibility on exportSnapshot", () => {
    const docA = new Loro();
    docA.getText("text").insert(0, "123");
    docA.getMap("map").set("key", 123);
    docA.getMap("map").set("key", "123");
    docA.getList("list").insert(0, 1);
    docA.getList("list").insert(0, "1");
    const t = docA.getTree("tree");
    const node = t.createNode();
    t.createNode(node.id, 0);
    const bytes = docA.exportSnapshot();

    const docB = new OLD.Loro();
    docB.import(bytes);
    expect(docA.toJSON()).toStrictEqual(docB.toJSON());
  });

  it("basic backward compatibility on exportSnapshot", () => {
    const docA = new OLD.Loro();
    docA.getText("text").insert(0, "123");
    docA.getMap("map").set("key", 123);
    docA.getMap("map").set("key", "123");
    docA.getList("list").insert(0, 1);
    docA.getList("list").insert(0, "1");
    const t = docA.getTree("tree");
    const node = t.createNode();
    t.createNode(node.id);
    const bytes = docA.exportSnapshot();

    const docB = new Loro();
    docB.import(bytes);
    expect(docA.toJSON()).toStrictEqual(docB.toJSON());
  });

  it("basic backward compatibility on exportSnapshot", () => {
    const docA = new OLD.Loro();
    docA.getText("text").insert(0, "123");
    docA.getMap("map").set("key", 123);
    docA.getMap("map").set("key", "123");
    docA.getList("list").insert(0, 1);
    docA.getList("list").insert(0, "1");
    const t = docA.getTree("tree");
    const node = t.createNode();
    t.createNode(node.id);
    const bytes = docA.exportSnapshot();

    const docB = new Loro();
    docB.import(bytes);
    expect(docA.toJSON()).toStrictEqual(docB.toJSON());
  });
});
