import { Loro, LoroList, LoroMap, PeerID, Value } from "../src";
import { expect, expectTypeOf, test } from "vitest";

test("Container should not match Value", () => {
  const list = new LoroList();
  expectTypeOf(list).not.toMatchTypeOf<Value>();
});

test("A non-numeric string is not a valid peer id", () => {
  expectTypeOf("123" as const).toMatchTypeOf<PeerID>();
  expectTypeOf("a123" as const).not.toMatchTypeOf<PeerID>();
});
