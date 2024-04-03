import {
  Loro,
  LoroList,
  LoroMap,
  LoroText,
  LoroTree,
  PeerID,
  Value,
} from "../src";
import { expect, expectTypeOf, test } from "vitest";

test("Container should not match Value", () => {
  const list = new LoroList();
  expectTypeOf(list).not.toMatchTypeOf<Value>();
});

test("A non-numeric string is not a valid peer id", () => {
  const doc = new Loro();
  expectTypeOf(doc.peerIdStr).toMatchTypeOf<PeerID>();
  expectTypeOf("123" as const).toMatchTypeOf<PeerID>();
  expectTypeOf("a123" as const).not.toMatchTypeOf<PeerID>();
});

test("Expect container type", () => {
  const list = new LoroList();
  expectTypeOf(list.kind()).toMatchTypeOf<"List">();
  const map = new LoroMap();
  expectTypeOf(map.kind()).toMatchTypeOf<"Map">();
  const text = new LoroText();
  expectTypeOf(text.kind()).toMatchTypeOf<"Text">();
  const tree = new LoroTree();
  expectTypeOf(tree.kind()).toMatchTypeOf<"Tree">();
});
