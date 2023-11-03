/**
 *  The skeleton of this binding is learned from https://github.com/yjs/y-quill
 */

import { Delta, Loro, LoroText } from "loro-crdt";
import Quill, { DeltaStatic, Sources } from "quill";
// @ts-ignore
import isEqual from "is-equal";

const Delta = Quill.import("delta");

export class QuillBinding {
  private richtext: LoroText;
  constructor(public doc: Loro, public quill: Quill) {
    this.quill = quill;
    this.richtext = doc.getText("text");
    this.richtext.subscribe(doc, (event) => {
      // Promise.resolve().then(() => {
      //   let delta: DeltaType = new Delta(
      //     richtext.getAnnSpans(),
      //   );
      //   quill.setContents(
      //     delta,
      //     "this" as any,
      //   );
      // });
      Promise.resolve().then(() => {
        if (!event.local && event.diff.type == "text") {
          console.log(
            doc.peerId,
            "CRDT_EVENT",
            event,
          );
          const eventDelta = event.diff.diff;
          const delta: Delta<string>[] = [];
          let index = 0;
          for (let i = 0; i < eventDelta.length; i++) {
            const d = eventDelta[i];
            const length = d.delete || d.retain || d.insert!.length;
            // skip the last newline that quill automatically appends
            if (
              d.insert && d.insert === "\n" &&
              index === quill.getLength() - 1 &&
              i === eventDelta.length - 1 && d.attributes != null &&
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
      if (origin !== "this" as any) {
        this.richtext.applyDelta(ops);
        const a = this.richtext.toDelta();
        const b = this.quill.getContents().ops;
        console.log(this.doc.peerId, "COMPARE AFTER QUILL_EVENT");
        assertEqual(a, b as any);
        console.log(
          this.doc.peerId,
          "CHECK_MATCH",
          { delta },
          a,
          b,
        );
        console.log("SIZE", this.doc.exportFrom().length);
      }
    }
  };
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
 * Removes the pending '\n's if it has no attributes.
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
      d.attributes === undefined && insert !== undefined &&
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
      return delta;
    }
  }
  return delta;
};
