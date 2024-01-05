import { describe, expect, it } from "vitest";
import { Loro, toReadableVersion, setPanicHook, OpId } from "../src";

describe("Frontiers", () => {
  it("two clients", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, "0");
    doc.commit();

    const v0 = doc.frontiers();
    const docB = new Loro();
    docB.import(doc.exportFrom());
    expect(docB.cmpFrontiers(v0)).toBe(0);
    text.insert(1, "0");
    doc.commit();
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(-1);
    const textB = docB.getText("text");
    textB.insert(0, "0");
    docB.commit();
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(-1);
    docB.import(doc.exportFrom());
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(1);
    doc.import(docB.exportFrom());
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(0);
  });
});

describe("Version", () => {
  const a = new Loro();
  a.setPeerId(0n);
  const b = new Loro();
  b.setPeerId(1n);
  a.getText("text").insert(0, "ha");
  b.getText("text").insert(0, "yo");
  a.import(b.exportFrom());
  a.getText("text").insert(0, "k");
  a.commit();

  it("version vector to frontiers", () => {
    {
      const vv = new Map();
      vv.set("0", 3);
      vv.set("1", 2);
      expect(toReadableVersion(a.version())).toStrictEqual(vv);
      expect(toReadableVersion(a.version())).toStrictEqual(vv);
      expect(a.vvToFrontiers(vv)).toStrictEqual(a.frontiers());
      expect(a.vvToFrontiers(a.version())).toStrictEqual(a.frontiers());
      expect(a.frontiers()).toStrictEqual([{ peer: "0", counter: 2 }] as OpId[]);
    }
  });

  it("get changes", () => {
    const changes = a.getAllChanges();
    expect(typeof changes.get("0")?.[0].peer == "string").toBeTruthy();
    expect(changes.size).toBe(2);
    expect(changes.get("0")?.length).toBe(2);
    expect(changes.get("0")?.[0].length).toBe(2);
    expect(changes.get("0")?.[1].lamport).toBe(2);
    expect(changes.get("0")?.[1].deps).toStrictEqual([
      { peer: "0", counter: 1 },
      { peer: "1", counter: 1 },
    ]);
    expect(changes.get("1")?.length).toBe(1);
  });

  it("get ops inside changes", () => {
    const change = a.getOpsInChange({ peer: "0", counter: 2 });
    expect(change.length).toBe(1);
  });
});
