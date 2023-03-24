export {
  LoroList,
  LoroMap,
  LoroText,
  PrelimList,
  PrelimMap,
  PrelimText,
  setPanicHook,
  Transaction,
} from "loro-wasm";
import { Loro, Transaction } from "loro-wasm";

export type { ContainerID, ContainerType } from "loro-wasm";

Loro.prototype.transact = function (cb, origin) {
  this.__raw__transactionWithOrigin(origin, (txn: Transaction) => {
    try {
      cb(txn);
    } finally {
      txn.commit();
      txn.free();
    }
  });
};

interface Event {
  local: boolean;
  origin?: string;
}

interface Listener {
  (event: Event): void;
}

declare module "loro-wasm" {
  interface Loro {
    subscribe(listener: Listener): number;
    transact(f: (tx: Transaction) => void, origin?: string): void;
  }
}

export { Loro };
