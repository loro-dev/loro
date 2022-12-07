import initWasm, {
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
import {
  ContainerID,
  ContainerType,
  InitInput,
  InitOutput,
} from "./pkg/loro_wasm.d.ts";

export {
  initSync,
  Loro,
  LoroList,
  LoroMap,
  LoroText,
  PrelimList,
  PrelimMap,
  PrelimText,
};

export type { ContainerID, ContainerType };

// Extend the interfaces here, to provide richer type information
declare module "./pkg/loro_wasm.js" {
  interface Loro {
    exportUpdates(version?: Uint8Array): Uint8Array;
    getContainerById(id: ContainerID): LoroText | LoroMap | LoroList;
  }
}

export async function init(
  module_or_path?: InitInput | Promise<InitInput>,
): Promise<InitOutput> {
  const out = await initWasm(module_or_path);
  setPanicHook();
  return out;
}
