import { describe, expect, it } from "vitest";
import { LoroDoc } from "../";

describe("Mergeable Container", () => {
  it("should merge concurrent lists", () => {
    const doc1 = new LoroDoc();
    doc1.setPeerId(1);
    const doc2 = new LoroDoc();
    doc2.setPeerId(2);

    const map1 = doc1.getMap("map");
    const list1 = map1.getMergeableList("list");
    list1.insert(0, 1);

    const map2 = doc2.getMap("map");
    const list2 = map2.getMergeableList("list");
    list2.insert(0, 2);

    doc1.import(doc2.export({ mode: "snapshot" }));
    doc2.import(doc1.export({ mode: "snapshot" }));

    const list1Merged = map1.getMergeableList("list");
    const list2Merged = map2.getMergeableList("list");

    expect(list1Merged.id).toBe(list2Merged.id);
    expect(list1Merged.length).toBe(2);
    expect(list2Merged.length).toBe(2);
    expect(list1Merged.toJSON()).toEqual(expect.arrayContaining([1, 2]));
  });

  it("should support deep nesting", () => {
    const doc1 = new LoroDoc();
    doc1.setPeerId(1);
    const doc2 = new LoroDoc();
    doc2.setPeerId(2);

    // Map -> MergeableMap -> MergeableMap -> MergeableList
    {
      const root = doc1.getMap("root");
      const level1 = root.getMergeableMap("level1");
      const level2 = level1.getMergeableMap("level2");
      const list = level2.getMergeableList("list");
      list.insert(0, "A");
    }

    {
      const root = doc2.getMap("root");
      const level1 = root.getMergeableMap("level1");
      const level2 = level1.getMergeableMap("level2");
      const list = level2.getMergeableList("list");
      list.insert(0, "B");
    }

    doc1.import(doc2.export({ mode: "snapshot" }));
    doc2.import(doc1.export({ mode: "snapshot" }));

    const root = doc1.getMap("root");
    const level1 = root.getMergeableMap("level1");
    const level2 = level1.getMergeableMap("level2");
    const list = level2.getMergeableList("list");

    expect(list.length).toBe(2);
    expect(list.toJSON()).toEqual(expect.arrayContaining(["A", "B"]));
  });

  it("should hide mergeable containers from root", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");
    map.getMergeableList("list");

    const json = doc.toJSON() as any;
    // Should contain "map"
    expect(json.map).toBeDefined();

    // Should NOT contain the mergeable list ID as a key in the root map
    // The mergeable list ID contains "/", so we can check if any key contains "/"
    const keys = Object.keys(json);
    for (const key of keys) {
      expect(key).not.toContain("/");
    }
  });

  it("should support all types", () => {
    const doc = new LoroDoc();
    const map = doc.getMap("map");

    const mList = map.getMergeableList("list");
    mList.insert(0, 1);

    const mMap = map.getMergeableMap("map");
    mMap.set("key", "value");

    const mText = map.getMergeableText("text");
    mText.insert(0, "hello");

    const mTree = map.getMergeableTree("tree");
    mTree.createNode();

    const mMovableList = map.getMergeableMovableList("movableList");
    mMovableList.insert(0, 1);

    const mCounter = map.getMergeableCounter("counter");
    mCounter.increment(10);

    const json = map.toJSON() as any;
    expect(json.list).toEqual([1]);
    expect(json.map).toEqual({ key: "value" });
    expect(json.text).toBe("hello");
    expect(json.tree).toHaveLength(1);
    expect(json.movableList).toEqual([1]);
    expect(json.counter).toBe(10);
  });

  it("should not have malformed container IDs with repeated cid:root- prefix", () => {
    // This test verifies the fix for the bug where nested mergeable containers
    // would have malformed IDs like "cid:root-cid:root-cid:root-game:Map/players:Map/alice:Map"
    // instead of clean IDs like "cid:root-game/players/alice:Map"
    const doc = new LoroDoc();

    // Create a nested structure: root map -> mergeable map -> mergeable map
    const game = doc.getMap("game");
    const players = game.getMergeableMap("players");
    const alice = players.getMergeableMap("alice");

    // Get the container IDs
    const gameId = game.id;
    const playersId = players.id;
    const aliceId = alice.id;

    // The game container should be a normal root container
    expect(gameId).toBe("cid:root-game:Map");

    // The players container should have a clean ID without repeated "cid:root-" prefix
    // Expected: "cid:root-game/players:Map"
    // Bug produces: "cid:root-cid:root-game:Map/players:Map"
    expect(playersId).not.toContain("cid:root-cid:root-");
    expect(playersId).toBe("cid:root-game/players:Map");

    // The alice container should also have a clean ID
    // Expected: "cid:root-game/players/alice:Map"
    // Bug produces: "cid:root-cid:root-cid:root-game:Map/players:Map/alice:Map"
    expect(aliceId).not.toContain("cid:root-cid:root-");
    expect(aliceId).toBe("cid:root-game/players/alice:Map");
  });
});
