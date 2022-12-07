import initWasm, {
  ContainerID,
  initSync,
  Loro,
  LoroList,
  LoroMap,
  LoroText,
  PrelimList,
  PrelimMap,
  PrelimText,
  setPanicHook,
} from "./pkg/loro_wasm.js";
import { InitInput, InitOutput } from "./pkg/loro_wasm.d.ts";

export {
  ContainerID,
  initSync,
  Loro,
  LoroList,
  LoroMap,
  LoroText,
  PrelimList,
  PrelimMap,
  PrelimText,
};

// Extend the interfaces here, to provide richer type information
declare module "./pkg/loro_wasm.js" {
  interface Loro {
    exportUpdates(version?: Uint8Array): Uint8Array;
  }
}

export async function init(module_or_path?: InitInput | Promise<InitInput>): Promise<InitOutput> {
  const out = await initWasm(module_or_path);
  setPanicHook();
  return out
}
