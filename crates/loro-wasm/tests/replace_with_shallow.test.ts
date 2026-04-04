import { describe, expect, it, vi } from "vitest";
import {
  LoroDoc,
  LoroMap,
  type LoroEventBatch,
  type TextDiff,
} from "../bundler/index";

function oneMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}

describe("replaceWithShallow", () => {
  describe("basic functionality", () => {
    it("preserves document data", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      const text = doc.getText("text");
      text.insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();
      text.insert(5, " World");
      doc.commit();

      const valueBefore = doc.toJSON();
      doc.replaceWithShallow(frontiersAfterHello);

      expect(doc.toJSON()).toEqual(valueBefore);
    });

    it("makes document shallow", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      expect(doc.isShallow()).toBe(false);

      doc.replaceWithShallow(frontiersAfterHello);

      expect(doc.isShallow()).toBe(true);
    });

    it("returns correct shallowSinceVV", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersBefore = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersBefore);

      const shallowVV = doc.shallowSinceVV().toJSON();
      // "Hello" is 5 chars (counter 0-4), so shallowSinceVV is 4
      expect(shallowVV.get("1")).toBe(4);
    });

    it("preserves peer ID after replace", () => {
      const doc = new LoroDoc();
      const originalPeerId = "12345";
      doc.setPeerId(originalPeerId);
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      expect(doc.peerId).toBe(BigInt(originalPeerId));
    });
  });

  describe("continued editing", () => {
    it("can insert after replace", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      doc.getText("text").insert(11, "!");
      doc.commit();

      expect(doc.getText("text").toString()).toBe("Hello World!");
    });

    it("can sync with other documents via snapshot", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      doc.getText("text").insert(11, "!");
      doc.commit();

      const doc2 = new LoroDoc();
      doc2.import(doc.export({ mode: "snapshot" }));

      expect(doc2.getText("text").toString()).toBe("Hello World!");
    });

    it("can create new root container after replace", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      const newMap = doc.getMap("brandNewMap");
      newMap.set("key", "value");
      doc.commit();

      expect(doc.getMap("brandNewMap").get("key")).toBe("value");
    });

    it("can create nested containers after replace", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      const list = doc.getList("newList");
      const nestedMap = list.insertContainer(0, new LoroMap());
      nestedMap.set("nested", "value");
      doc.commit();

      expect(
        (doc.getList("newList").get(0) as LoroMap).get("nested"),
      ).toBe("value");
    });

    it("can create multiple new containers after replace", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      doc.getMap("map1").set("a", 1);
      doc.getMap("map2").set("b", 2);
      doc.getList("list1").insert(0, "x");
      doc.getText("text2").insert(0, "y");
      doc.commit();

      expect(doc.getMap("map1").get("a")).toBe(1);
      expect(doc.getMap("map2").get("b")).toBe(2);
      expect(doc.getList("list1").get(0)).toBe("x");
      expect(doc.getText("text2").toString()).toBe("y");
    });
  });

  describe("subscriptions", () => {
    it("doc-level subscriptions continue to fire", async () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      let called = 0;
      doc.subscribe(() => {
        called += 1;
      });

      doc.getText("text").insert(0, "Hello");
      doc.commit();
      await oneMs();
      expect(called).toBeGreaterThan(0);
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();
      await oneMs();

      const countBefore = called;
      doc.replaceWithShallow(frontiersAfterHello);

      doc.getText("text").insert(11, "!");
      doc.commit();
      await oneMs();
      expect(called).toBeGreaterThan(countBefore);
    });

    it("container subscriptions continue to fire", async () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      const text = doc.getText("text");
      let called = 0;
      text.subscribe(() => {
        called += 1;
      });

      text.insert(0, "Hello");
      doc.commit();
      await oneMs();
      expect(called).toBeGreaterThan(0);
      const frontiersAfterHello = doc.oplogFrontiers();

      text.insert(5, " World");
      doc.commit();
      await oneMs();

      const countBefore = called;
      doc.replaceWithShallow(frontiersAfterHello);

      doc.getText("text").insert(11, "!");
      doc.commit();
      await oneMs();
      expect(called).toBeGreaterThan(countBefore);
    });

    it("unsubscribe works after replace", async () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      let called = 0;
      const unsub = doc.subscribe(() => {
        called += 1;
      });

      doc.getText("text").insert(0, "Hello");
      doc.commit();
      await oneMs();
      expect(called).toBeGreaterThan(0);
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();
      await oneMs();

      doc.replaceWithShallow(frontiersAfterHello);

      const countAfterReplace = called;
      unsub();

      doc.getText("text").insert(11, "!");
      doc.commit();
      await oneMs();
      expect(called).toBe(countAfterReplace);
    });

    it("subscribeLocalUpdates continues to work", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      let updateCount = 0;
      doc.subscribeLocalUpdates(() => {
        updateCount += 1;
      });

      doc.getText("text").insert(0, "Hello");
      doc.commit();
      expect(updateCount).toBe(1);
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();
      expect(updateCount).toBe(2);

      doc.replaceWithShallow(frontiersAfterHello);

      doc.getText("text").insert(11, "!");
      doc.commit();
      expect(updateCount).toBe(3);
    });

    it("does not trigger LORO_INTERNAL_ERROR", async () => {
      const errorSpy = vi.spyOn(console, "error").mockImplementation(() => { });
      try {
        const doc = new LoroDoc();
        doc.setPeerId("1");
        doc.subscribe(() => { });
        doc.getText("text").insert(0, "Hello");
        doc.commit();
        const frontiersAfterHello = doc.oplogFrontiers();

        doc.getText("text").insert(5, " World");
        doc.commit();

        doc.replaceWithShallow(frontiersAfterHello);
        await Promise.resolve();

        expect(
          errorSpy.mock.calls.some((args) =>
            args.some((arg) => String(arg).includes("[LORO_INTERNAL_ERROR]")),
          ),
        ).toBe(false);
      } finally {
        errorSpy.mockRestore();
      }
    });

    it("event content is correct after replace", async () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      const text = doc.getText("text");
      text.insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      text.insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      let lastEvent: LoroEventBatch | undefined;
      doc.subscribe((event) => {
        lastEvent = event;
      });

      text.insert(11, "!");
      doc.commit();
      await oneMs();

      expect(lastEvent).toBeDefined();
      if (!lastEvent) throw new Error('lastEvent should be defined')

      expect(lastEvent.events[0].target).toBe(text.id);
      expect(lastEvent.events[0].path).toStrictEqual(["text"]);
      expect(lastEvent.events[0].diff).toStrictEqual({
        type: "text",
        diff: [{ retain: 11 }, { insert: "!" }],
      } as TextDiff);
    });

    it("container subscription event content is correct after replace", async () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      const text = doc.getText("text");
      text.insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      text.insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      let lastEvent: LoroEventBatch | undefined;
      text.subscribe((event) => {
        lastEvent = event;
      });

      text.insert(0, "Say: ");
      doc.commit();
      await oneMs();

      expect(lastEvent).toBeDefined();
      if (!lastEvent) throw new Error('lastEvent should be defined')

      expect(lastEvent.events[0].target).toBe(text.id);
      expect(lastEvent.events[0].diff).toStrictEqual({
        type: "text",
        diff: [{ insert: "Say: " }],
      } as TextDiff);
    });
  });

  describe("handlers", () => {
    it("existing handlers remain valid", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      const text = doc.getText("text");
      text.insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      text.insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      const textAfter = doc.getText("text");
      expect(textAfter.toString()).toBe("Hello World");

      textAfter.insert(11, "!");
      doc.commit();
      expect(textAfter.toString()).toBe("Hello World!");
    });

    it("can get new handlers after replace", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      const map = doc.getMap("newMap");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      map.set("key", "value");
      doc.commit();

      expect(doc.getMap("newMap").get("key")).toBe("value");
    });
  });

  describe("versions and frontiers", () => {
    it("oplogFrontiers returns correct value", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersBefore = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();
      const frontiersAfter = doc.oplogFrontiers();

      doc.replaceWithShallow(frontiersBefore);

      const frontiersAfterReplace = doc.oplogFrontiers();
      expect(frontiersAfterReplace).toEqual(frontiersAfter);
    });

    it("can checkout within shallow range", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "A");
      doc.commit();
      const frontiersA = doc.oplogFrontiers();

      doc.getText("text").insert(1, "B");
      doc.commit();
      const frontiersB = doc.oplogFrontiers();

      doc.getText("text").insert(2, "C");
      doc.commit();

      doc.replaceWithShallow(frontiersA);

      doc.checkout(frontiersB);
      expect(doc.getText("text").toString()).toBe("AB");
    });

    it("throws when checking out before shallow root", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "A");
      doc.commit();

      doc.getText("text").insert(1, "B");
      doc.commit();
      const frontiersB = doc.oplogFrontiers();

      doc.getText("text").insert(2, "C");
      doc.commit();

      doc.replaceWithShallow(frontiersB);

      expect(() => {
        doc.checkout([{ peer: "1", counter: 0 }]);
      }).toThrow();
    });

    it("revertTo before shallow root throws", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "A");
      doc.commit();
      const frontiersA = doc.oplogFrontiers();

      doc.getText("text").insert(1, "B");
      doc.commit();
      const frontiersB = doc.oplogFrontiers();

      doc.getText("text").insert(2, "C");
      doc.commit();

      doc.replaceWithShallow(frontiersB);

      expect(() => {
        doc.revertTo(frontiersA);
      }).toThrow();
    });

    it("revertTo within shallow range works", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "A");
      doc.commit();
      const frontiersA = doc.oplogFrontiers();

      doc.getText("text").insert(1, "B");
      doc.commit();
      const frontiersB = doc.oplogFrontiers();

      doc.getText("text").insert(2, "C");
      doc.commit();

      doc.replaceWithShallow(frontiersA);

      doc.revertTo(frontiersB);
      doc.commit();

      expect(doc.getText("text").toString()).toBe("AB");
    });
  });

  describe("export/import", () => {
    it("can export snapshot after replace", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      const snapshot = doc.export({ mode: "snapshot" });
      expect(snapshot.length).toBeGreaterThan(0);

      const doc2 = new LoroDoc();
      doc2.import(snapshot);
      expect(doc2.getText("text").toString()).toBe("Hello World");
    });

    it("can export updates after replace", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      doc.getText("text").insert(11, "!");
      doc.commit();

      const updates = doc.export({ mode: "update" });
      expect(updates.length).toBeGreaterThan(0);
    });

    it("other docs can import from replaced doc", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      doc.getText("text").insert(11, "!");
      doc.commit();

      const doc2 = new LoroDoc();
      doc2.import(doc.export({ mode: "snapshot" }));
      expect(doc2.getText("text").toString()).toBe("Hello World!");
      expect(doc2.isShallow()).toBe(true);
    });

    it("export updates from shallow doc can be imported", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " World");
      doc.commit();

      doc.replaceWithShallow(frontiersAfterHello);

      doc.getText("text").insert(11, "!");
      doc.commit();

      const updates = doc.export({ mode: "update" });

      const doc2 = new LoroDoc();
      doc2.setPeerId("2");
      doc2.import(doc.export({ mode: "snapshot" }));

      expect(doc2.getText("text").toString()).toBe("Hello World!");
      expect(updates.length).toBeGreaterThan(0);
    });
  });

  describe("concurrent editing", () => {
    it("shallow doc syncs with non-shallow doc via snapshot", () => {
      const doc1 = new LoroDoc();
      doc1.setPeerId("1");
      doc1.getText("text").insert(0, "Hello");
      doc1.commit();
      const frontiersAfterHello = doc1.oplogFrontiers();

      doc1.getText("text").insert(5, " there");
      doc1.commit();

      const doc2 = new LoroDoc();
      doc2.setPeerId("2");
      doc2.import(doc1.export({ mode: "snapshot" }));

      doc1.replaceWithShallow(frontiersAfterHello);

      doc2.getText("text").insert(11, " World");
      doc2.commit();

      doc1.import(doc2.export({ mode: "snapshot" }));

      expect(doc1.getText("text").toString()).toBe("Hello there World");
    });

    it("non-shallow doc syncs with shallow doc via snapshot", () => {
      const doc1 = new LoroDoc();
      doc1.setPeerId("1");
      doc1.getText("text").insert(0, "Hello");
      doc1.commit();
      const frontiersAfterHello = doc1.oplogFrontiers();

      doc1.getText("text").insert(5, " there");
      doc1.commit();

      const doc2 = new LoroDoc();
      doc2.setPeerId("2");
      doc2.import(doc1.export({ mode: "snapshot" }));

      doc1.replaceWithShallow(frontiersAfterHello);

      doc1.getText("text").insert(11, " World");
      doc1.commit();

      doc2.import(doc1.export({ mode: "snapshot" }));

      expect(doc2.getText("text").toString()).toBe("Hello there World");
    });

    it("two peers edit after one does replaceWithShallow", () => {
      const doc1 = new LoroDoc();
      doc1.setPeerId("1");
      doc1.getText("text").insert(0, "Hello");
      doc1.commit();
      const frontiersAfterHello = doc1.oplogFrontiers();

      doc1.getText("text").insert(5, " there");
      doc1.commit();

      const doc2 = new LoroDoc();
      doc2.setPeerId("2");
      doc2.import(doc1.export({ mode: "snapshot" }));

      doc1.replaceWithShallow(frontiersAfterHello);

      doc1.getText("text").insert(11, "!");
      doc1.commit();

      doc2.getText("text").insert(0, "Say: ");
      doc2.commit();

      const snapshot1 = doc1.export({ mode: "snapshot" });
      const snapshot2 = doc2.export({ mode: "snapshot" });

      doc1.import(snapshot2);
      doc2.import(snapshot1);

      expect(doc1.getText("text").toString()).toBe(
        doc2.getText("text").toString(),
      );
      expect(doc1.getText("text").toString()).toContain("Say:");
      expect(doc1.getText("text").toString()).toContain("Hello");
      expect(doc1.getText("text").toString()).toContain("!");
    });

    it("conflict resolution across shallow boundary", () => {
      const doc1 = new LoroDoc();
      doc1.setPeerId("1");
      doc1.getText("text").insert(0, "AB");
      doc1.commit();
      const frontiersAfterAB = doc1.oplogFrontiers();

      doc1.getText("text").insert(2, "CD");
      doc1.commit();

      const doc2 = new LoroDoc();
      doc2.setPeerId("2");
      doc2.import(doc1.export({ mode: "snapshot" }));

      doc1.replaceWithShallow(frontiersAfterAB);

      doc1.getText("text").insert(1, "X");
      doc1.commit();

      doc2.getText("text").insert(1, "Y");
      doc2.commit();

      doc1.import(doc2.export({ mode: "snapshot" }));
      doc2.import(doc1.export({ mode: "snapshot" }));

      expect(doc1.getText("text").toString()).toBe(
        doc2.getText("text").toString(),
      );
      expect(doc1.getText("text").toString()).toContain("X");
      expect(doc1.getText("text").toString()).toContain("Y");
    });
  });

  describe("cloned document independence", () => {
    it("fork before replace creates independent doc", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();
      const frontiersAfterHello = doc.oplogFrontiers();

      doc.getText("text").insert(5, " there");
      doc.commit();

      const forked = doc.fork();

      doc.replaceWithShallow(frontiersAfterHello);

      expect(doc.isShallow()).toBe(true);
      expect(forked.isShallow()).toBe(false);

      doc.getText("text").insert(11, " World");
      doc.commit();
      expect(forked.getText("text").toString()).toBe("Hello there");
    });
  });

  describe("size reduction", () => {
    it("reduces snapshot size", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");

      const text = doc.getText("text");
      text.insert(0, "Initial");
      doc.commit();
      const frontiersAfterInitial = doc.oplogFrontiers();

      for (let i = 0; i < 10; i++) {
        doc.setPeerId(BigInt(i + 1));
        text.insert(text.length, `Line ${i}\n`);
        doc.commit();
      }

      const snapshotSizeBefore = doc.export({ mode: "snapshot" }).length;

      doc.replaceWithShallow(frontiersAfterInitial);

      const snapshotSizeAfter = doc.export({ mode: "snapshot" }).length;

      expect(snapshotSizeAfter).toBeLessThanOrEqual(snapshotSizeBefore);
      expect(doc.isShallow()).toBe(true);
    });

    it("trimming at earlier version reduces size more", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");

      const text = doc.getText("text");
      text.insert(0, "A");
      doc.commit();
      const frontiersEarly = doc.oplogFrontiers();

      for (let i = 0; i < 10; i++) {
        text.insert(text.length, `${i}`);
        doc.commit();
      }
      const frontiersLate = doc.oplogFrontiers();

      const docEarly = doc.fork();
      const docLate = doc.fork();

      docEarly.replaceWithShallow(frontiersEarly);
      docLate.replaceWithShallow(frontiersLate);

      const sizeEarly = docEarly.export({ mode: "snapshot" }).length;
      const sizeLate = docLate.export({ mode: "snapshot" }).length;

      expect(sizeEarly).toBeLessThanOrEqual(sizeLate);
      expect(docEarly.toJSON()).toEqual(docLate.toJSON());
    });

    it("reduces change count with multiple peers", () => {
      const doc = new LoroDoc();

      const text = doc.getText("text");
      for (let i = 0; i < 5; i++) {
        doc.setPeerId(BigInt(i + 1));
        text.insert(text.length, `${i}`);
        doc.commit();
      }

      const changeCountBefore = doc.changeCount();
      expect(changeCountBefore).toBeGreaterThan(1);

      // Use current frontiers - shallow snapshot collapses all changes into one
      const frontiers = doc.oplogFrontiers();
      doc.replaceWithShallow(frontiers);

      const changeCountAfter = doc.changeCount();
      expect(changeCountAfter).toBe(1);
      expect(changeCountAfter).toBeLessThan(changeCountBefore);
    });
  });

  describe("error cases", () => {
    it("throws on invalid frontiers", () => {
      const doc = new LoroDoc();
      doc.setPeerId("1");
      doc.getText("text").insert(0, "Hello");
      doc.commit();

      expect(() => {
        doc.replaceWithShallow([{ peer: "999", counter: 100 }]);
      }).toThrow();
    });
  });
});
