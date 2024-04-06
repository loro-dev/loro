import diff, { Diff } from "fast-diff";
import { LoroText } from "loro-wasm";

LoroText.prototype.updateText = function (newText: string) {
  const src = this.toString();
  const delta = diff(src, newText);
  let index = 0;
  for (const [op, text] of delta) {
    if (op === 0) {
      index += text.length;
    } else if (op === 1) {
      this.insert(index, text);
      index += text.length;
    } else {
      this.delete(index, text.length);
    }
  }

};
