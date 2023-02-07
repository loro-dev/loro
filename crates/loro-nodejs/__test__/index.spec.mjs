import test from 'ava'

import { Loro } from '../index.js'

test('loro text', (t) => {
  const loro = new Loro();
  const text = loro.getText("text");
  text.insert(loro, 0, "abc");
  t.is(text.value(), "abc");
})
