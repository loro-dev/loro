import { Loro, LoroList, LoroMap, Value } from "../src";
import { expect, expectTypeOf, test } from "vitest";

test("You shuold not insert a container by using `insert` function", () => {
  const list = new LoroList();
  expectTypeOf(list).not.toMatchTypeOf<Value>();
});

test("Container attached state", () => {
  const list = new LoroList();
  expect(list.isAttached()).toBe(false);
  expectTypeOf(list.isAttached()).toMatchTypeOf<false>();
  const doc = new Loro();
  {
    const map = doc.getMap("map");
    expectTypeOf(map.isAttached()).toMatchTypeOf<true>();
    expectTypeOf(map).toMatchTypeOf<LoroMap<any, true>>();
  }
  {
    const map = new LoroMap();
    expectTypeOf(map.isAttached()).toMatchTypeOf<false>();
    expectTypeOf(map).toMatchTypeOf<LoroMap<any, false>>();
  }
  {
    const map = list.insertContainer(0, new LoroMap());
    expectTypeOf(map.isAttached()).toMatchTypeOf<false>();
    expectTypeOf(map).toMatchTypeOf<LoroMap<any, false>>();
  }
});
