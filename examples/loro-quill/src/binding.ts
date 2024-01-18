/**
 *  The skeleton of this binding is learned from https://github.com/yjs/y-quill
 */

import { Delta, Loro, LoroText, setDebug } from "loro-crdt";
import Quill, { DeltaOperation, DeltaStatic, Sources } from "quill";
// @ts-ignore
import isEqual from "is-equal";

// setDebug("*");
const Delta = Quill.import("delta");
// setDebug("*");

const EXPAND_CONFIG: { [key in string]: 'before' | 'after' | 'both' | 'none' } = {
  bold: 'after',
  italic: 'after',
  underline: 'after',
  link: 'none',
  header: 'none',
}

export class QuillBinding {
  private richtext: LoroText;
  constructor(
    public doc: Loro,
    public quill: Quill,
  ) {
    doc.configTextStyle({
      bold: { expand: "after" },
      italic: { expand: "after" },
      underline: { expand: "after" },
      link: { expand: "none" },
      header: { expand: "none" },
    })
    this.quill = quill;
    this.richtext = doc.getText("text");
    this.richtext.subscribe(doc, (event) => {
      Promise.resolve().then(() => {
        if (!event.local && event.diff.type == "text") {
          console.log(doc.peerId, "CRDT_EVENT", event);
          const eventDelta = event.diff.diff;
          const delta: Delta<string>[] = [];
          let index = 0;
          for (let i = 0; i < eventDelta.length; i++) {
            const d = eventDelta[i];
            const length = d.delete || d.retain || d.insert!.length;
            // skip the last newline that quill automatically appends
            if (
              d.insert &&
              d.insert === "\n" &&
              index === quill.getLength() - 1 &&
              i === eventDelta.length - 1 &&
              d.attributes != null &&
              Object.keys(d.attributes).length > 0
            ) {
              delta.push({
                retain: 1,
                attributes: d.attributes,
              });
              index += length;
              continue;
            }

            delta.push(d);
            index += length;
          }

          quill.updateContents(new Delta(delta), "this" as any);
          const a = this.richtext.toDelta();
          const b = this.quill.getContents().ops;
          console.log(this.doc.peerId, "COMPARE AFTER CRDT_EVENT");
          if (!assertEqual(a, b as any)) {
            quill.setContents(new Delta(a), "this" as any);
          }
        }
      });
    });
    quill.setContents(
      new Delta(
        this.richtext.toDelta().map((x) => ({
          insert: x.insert,
          attributions: x.attributes,
        })),
      ),
      "this" as any,
    );
    quill.on("editor-change", this.quillObserver);
  }

  quillObserver: (
    name: "text-change",
    delta: DeltaStatic,
    oldContents: DeltaStatic,
    source: Sources,
  ) => any = (_eventType, delta, _state, origin) => {
    if (delta && delta.ops) {
      // update content
      const ops = delta.ops;
      if (origin !== ("this" as any)) {
        this.applyDelta(ops);
        const a = this.richtext.toDelta();
        const b = this.quill.getContents().ops;
        console.log(this.doc.peerId, "COMPARE AFTER QUILL_EVENT");
        assertEqual(a, b as any);
        console.log("SIZE", this.doc.exportFrom().length);
        this.doc.debugHistory();
      }
    }
  };

  applyDelta(delta: DeltaOperation[]) {
    let index = 0;
    for (const op of delta) {
      if (op.retain != null) {
        let end = index + op.retain;
        if (op.attributes) {
          if (index == this.richtext.length) {
            this.richtext.insert(index, "\n");
          }
          for (const key of Object.keys(op.attributes)) {
            let value = op.attributes[key];
            if (value == null) {
              this.richtext.unmark({ start: index, end, }, key)
            } else {
              this.richtext.mark({ start: index, end }, key, value,)
            }
          }
        }
        index += op.retain;
      } else if (op.insert != null) {
        if (typeof op.insert == "string") {
          let end = index + op.insert.length;
          this.richtext.insert(index, op.insert);
          if (op.attributes) {
            for (const key of Object.keys(op.attributes)) {
              let value = op.attributes[key];
              if (value == null) {
                this.richtext.unmark({ start: index, end, }, key)
              } else {
                this.richtext.mark({ start: index, end }, key, value)
              }
            }
          }
          index = end;
        } else {
          throw new Error("Not implemented")
        }
      } else if (op.delete != null) {
        this.richtext.delete(index, op.delete);
      } else {
        throw new Error("Unreachable")
      }
    }
    this.doc.commit();
  }

  destroy() {
    // TODO: unobserve
    this.quill.off("editor-change", this.quillObserver);
  }
}

function assertEqual(a: Delta<string>[], b: Delta<string>[]): boolean {
  a = normQuillDelta(a);
  b = normQuillDelta(b);
  const equal = isEqual(a, b);
  console.assert(equal, a, b);
  return equal;
}

/**
 * Removes the ending '\n's if it has no attributes.
 * 
 * Extract line-break to a single op
 *
 * Normalize attributes field
 */
export const normQuillDelta = (delta: Delta<string>[]) => {
  for (const d of delta) {
    for (const key of Object.keys(d.attributes || {})) {
      if (d.attributes![key] == null) {
        delete d.attributes![key];
      }
    }
  }

  for (const d of delta) {
    if (Object.keys(d.attributes || {}).length === 0) {
      delete d.attributes;
    }
  }

  if (delta.length > 0) {
    const d = delta[delta.length - 1];
    const insert = d.insert;
    if (
      d.attributes === undefined &&
      insert !== undefined &&
      insert.slice(-1) === "\n"
    ) {
      delta = delta.slice();
      let ins = insert.slice(0, -1);
      while (ins.slice(-1) === "\n") {
        ins = ins.slice(0, -1);
      }
      delta[delta.length - 1] = { insert: ins };
      if (ins.length === 0) {
        delta.pop();
      }
    }
  }

  const ans: Delta<string>[] = []
  for (const span of delta) {
    if (span.insert != null && span.insert.includes("\n")) {
      const lines = span.insert.split("\n");
      for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        if (line.length !== 0) {
          ans.push({ ...span, insert: line });
        }
        if (i < lines.length - 1) {
          const attr = { ...span.attributes };
          const v: Delta<string> = { insert: "\n" };
          for (const style of ['bold', 'link', 'italic', 'underline']) {
            if (attr && attr[style]) {
              delete attr[style];
            }
          }

          if (Object.keys(attr || {}).length > 0) {
            v.attributes = attr;
          }
          ans.push(v);
        }
      }
    } else {
      ans.push(span);
    }
  }

  return ans;
};
