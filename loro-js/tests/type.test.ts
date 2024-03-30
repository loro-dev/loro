import { Loro, LoroList, LoroMap, Value } from "../src";
import { expect, expectTypeOf, test } from "vitest";

test("You shuold not insert a container by using `insert` function", () => {
  const list = new LoroList();
  expectTypeOf(list).not.toMatchTypeOf<Value>();
});
