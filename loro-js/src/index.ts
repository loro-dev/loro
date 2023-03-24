export {
  Loro,
  LoroList,
  LoroMap,
  LoroText,
  PrelimList,
  PrelimMap,
  PrelimText,
  setPanicHook,
  Transaction,
} from "loro-wasm";

export type { ContainerID, ContainerType } from "loro-wasm";

interface Event {
  local: boolean;
  origin?: string;
}

interface Listener {
  (event: Event): void;
}

declare module "loro-wasm" {
  interface Loro {
    subscribe(listener: Listener): void;
  }
}
