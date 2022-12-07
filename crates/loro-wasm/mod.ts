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
import { InitInput } from "./pkg/loro_wasm.d.ts";

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

declare module "./pkg/loro_wasm.js" {
  interface Loro {
    export_updates(version?: Uint8Array): Uint8Array;
  }
}

export async function init(module_or_path?: InitInput | Promise<InitInput>) {
  await initWasm(module_or_path);
  setPanicHook();
}
