import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// A version with a mark's StyleStart but not its StyleEnd (op-level frontiers)
// used to trap on the first read of a fork or snapshot of it ("unclosed style
// mark"). See loro-dev/loro#1165.
function source(): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.configTextStyle({ bold: { expand: "after" } });
  const text = doc.getText("t");
  text.insert(0, "abc"); // 0@1..2@1
  text.mark({ start: 1, end: 3 }, "bold", true); // StyleStart 3@1, StyleEnd 4@1
  text.insert(3, "Z"); // 5@1
  doc.commit();
  return doc;
}

const midMark = [{ peer: "1" as const, counter: 3 }];

describe("versions with an unclosed style mark", () => {
  it("forkAt is readable and matches checkout", () => {
    const doc = source();
    const checkedOut = doc.fork();
    checkedOut.checkout(midMark);
    const expected = checkedOut.getText("t").toDelta();

    const fork = doc.forkAt(midMark);
    expect(fork.getText("t").toDelta()).toEqual(expected);
    fork.import(doc.export({ mode: "update" }));
    expect(fork.getText("t").toDelta()).toEqual(doc.getText("t").toDelta());
  });

  it("a snapshot of the checked-out version imports and reads", () => {
    const doc = source();
    doc.checkout(midMark);
    const expected = doc.getText("t").toDelta();
    const bytes = doc.fork().export({ mode: "snapshot" });
    const other = new LoroDoc();
    other.import(bytes);
    expect(other.getText("t").toDelta()).toEqual(expected);
  });
});
