import { TextDecoder } from "node:util";
import { describe, expect, it } from "vitest";
import { Container, LoroDoc, LoroText } from "../bundler/index";

function countTextDecodes<T>(callback: () => T): {
  result: T;
  decoderCalls: number;
} {
  const original = TextDecoder.prototype.decode;
  let decoderCalls = 0;
  TextDecoder.prototype.decode = function (...args) {
    decoderCalls += 1;
    return original.apply(this, args);
  };

  try {
    return { result: callback(), decoderCalls };
  } finally {
    TextDecoder.prototype.decode = original;
  }
}

describe("container id cache", () => {
  it("decodes once per wrapper for every container type", () => {
    const doc = new LoroDoc();
    const containers: Container[] = [
      doc.getMap("map"),
      doc.getText("text"),
      doc.getList("list"),
      doc.getTree("tree"),
      doc.getMovableList("movable-list"),
      doc.getCounter("counter"),
    ];

    const { result: ids, decoderCalls } = countTextDecodes(() =>
      containers.map((container) => [container.id, container.id]),
    );

    expect(decoderCalls).toBe(containers.length);
    for (const [first, second] of ids) {
      expect(second).toBe(first);
    }
  });

  it("keeps cache identity scoped to one wrapper", () => {
    const doc = new LoroDoc();
    const first = doc.getMap("map");
    const second = doc.getMap("map");

    const { result: ids, decoderCalls } = countTextDecodes(() => [
      first.id,
      first.id,
      second.id,
      second.id,
    ]);

    expect(first).not.toBe(second);
    expect(new Set(ids).size).toBe(1);
    expect(decoderCalls).toBe(2);
  });

  it("does not rebind a detached wrapper when it is attached", () => {
    const doc = new LoroDoc();
    const detached = new LoroText();
    const detachedId = detached.id;
    const attached = doc
      .getMap("map")
      .setContainer("text", detached) as LoroText;
    const attachedClone = attached.getAttached()!;

    expect(detached.isAttached()).toBe(false);
    expect(detached.id).toBe(detachedId);
    expect(attached.isAttached()).toBe(true);
    expect(attached.id).not.toBe(detachedId);
    expect(attachedClone).not.toBe(attached);
    expect(attachedClone.id).toBe(attached.id);
  });

  it("preserves container identity across snapshot import", () => {
    const source = new LoroDoc();
    source.setPeerId("1");
    const sourceText = source
      .getMap("map")
      .setContainer("text", new LoroText()) as LoroText;
    const id = sourceText.id;

    const imported = LoroDoc.fromSnapshot(source.export({ mode: "snapshot" }));
    const importedText = imported.getContainerById(id)!;

    expect(importedText.id).toBe(id);
    expect(importedText.id).toBe(id);
  });

  it("does not return a cached id after free", () => {
    const text = new LoroText();
    void text.id;
    text.free();

    expect(() => text.id).toThrow("null pointer passed to rust");
  });

  it("keeps a non-extensible wrapper readable without caching it", () => {
    const text = Object.preventExtensions(new LoroText());

    const { result: ids, decoderCalls } = countTextDecodes(() => [
      text.id,
      text.id,
    ]);

    expect(ids[1]).toBe(ids[0]);
    expect(decoderCalls).toBe(2);
    text.free();
  });
});
