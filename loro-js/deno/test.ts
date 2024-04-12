import { Loro } from "./mod.ts";
import { expect } from 'npm:expect'

Deno.test("test", () => {
  const doc = new Loro();
  const text = doc.getText("text");
  text.insert(0, "123")
  expect(text.toString()).toEqual("123");
  text.insert(0, "123")
  expect(text.toString()).toEqual("123123");
  const docB = Loro.fromSnapshot(doc.exportSnapshot());
  expect(docB.getText('text').toString()).toEqual("123123");
})
