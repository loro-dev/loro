import init, { Loro } from "../web/loro_wasm.js";
import { resolve } from "https://deno.land/std@0.105.0/path/mod.ts";
import __ from "https://deno.land/x/dirname@1.1.2/mod.ts";

const { __dirname } = __(import.meta);
import * as compress from "https://deno.land/x/compress@v0.4.5/zlib/mod.ts";

const wasm = await init();

const automerge = resolve(
  __dirname,
  "../../loro-internal/benches/automerge-paper.json.gz",
);
const { txns } = JSON.parse(
  new TextDecoder().decode(compress.inflate(Deno.readFileSync(automerge))),
);

const loro = apply();
const encoded = encode_updates(loro);
const snapshot = loro.exportSnapshot();

function apply() {
  const loro = new Loro();
  const text = loro.getText("text");

  for (let k = 0; k < 1; k++) {
    for (let i = 0; i < txns.length; i++) {
      const { patches } = txns[i];
      for (const [pos, delHere, insContent] of patches) {
        if (delHere > 0) text.delete(loro, pos, delHere);
        if (insContent !== "") text.insert(loro, pos, insContent);
      }
    }
  }

  return loro;
}

function encode_updates(loro: Loro): Uint8Array {
  return loro.exportUpdates(undefined);
}

function decode_updates(updates: Uint8Array): Loro {
  const loro = new Loro();
  loro.importUpdates(updates);
  return loro;
}

console.log("Encoded updates size", encoded.byteLength);
console.log("Encoded snapshot size", snapshot.byteLength);
console.log("WASM buffer size", wasm.memory.buffer.byteLength);

Deno.bench("[Apply] Loro WASM apply Automerge dataset", () => {
  apply().free();
});
Deno.bench(
  "[Encode.Updates] Loro WASM encode updates Automerge dataset",
  () => {
    encode_updates(loro);
  },
);
Deno.bench(
  "[Decode.Updates] Loro WASM decode updates Automerge dataset",
  () => {
    decode_updates(encoded);
  },
);

Deno.bench(
  "[Encode.Snapshot] Loro WASM encode snapshot Automerge dataset",
  () => {
    loro.exportSnapshot();
  },
);
Deno.bench(
  "[Decode.Snapshot] Loro WASM decode snapshot Automerge dataset",
  () => {
    loro.importSnapshot(snapshot);
  },
);
