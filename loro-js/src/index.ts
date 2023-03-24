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
import { ContainerID, Loro, Transaction } from "loro-wasm";

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

export type Value =
  | ContainerID
  | string
  | number
  | null
  | { [key: string]: Value }
  | Value[];

export type Path = (number | string)[];
export type Delta<T> = {
  type: "insert";
  value: T;
} | {
  type: "delete";
  len: number;
} | {
  type: "retain";
  len: number;
};

export type ListDiff = {
  type: "list";
  diff: Delta<Value[]>[];
};

export type TextDiff = {
  type: "text";
  diff: Delta<string>[];
};

export type MapDIff = {
  type: "map";
  diff: {
    added: Record<string, Value>;
    deleted: Record<string, Value>;
    updated: Record<string, {
      old: Value;
      new: Value;
    }>;
  };
};

export type Diff = ListDiff | TextDiff | MapDIff;

export interface LoroEvent {
  local: boolean;
  origin?: string;
  diff: Diff[];
  target: ContainerID;
  path: Path;
}

interface Listener {
  (event: LoroEvent): void;
}

const CONTAINER_TYPES = ["Map", "Text", "List"];

export function isContainerId(s: string): s is ContainerID {
  try {
    if (s.startsWith("/")) {
      const [_, type] = s.slice(1).split(":");
      if (!CONTAINER_TYPES.includes(type)) {
        return false;
      }
    } else {
      const [id, type] = s.split(":");
      if (!CONTAINER_TYPES.includes(type)) {
        return false;
      }

      const [counter, client] = id.split("@");
      Number.parseInt(counter);
      Number.parseInt(client);
    }

    return true;
  } catch (e) {
    return false;
  }
}

export { Loro };

declare module "loro-wasm" {
  interface Loro {
    subscribe(listener: Listener): number;
    transact(f: (tx: Transaction) => void, origin?: string): void;
  }

  interface LoroList {
    insertContainer(
      txn: Transaction | Loro,
      pos: number,
      container: "Map",
    ): LoroMap;
    insertContainer(
      txn: Transaction | Loro,
      pos: number,
      container: "List",
    ): LoroList;
    insertContainer(
      txn: Transaction | Loro,
      pos: number,
      container: "Text",
    ): LoroText;
    insertContainer(
      txn: Transaction | Loro,
      pos: number,
      container: string,
    ): never;

    get(index: number): Value;
    subscribe(txn: Transaction | Loro, listener: Listener): number;
    subscribeDeep(txn: Transaction | Loro, listener: Listener): number;
    subscribeOnce(txn: Transaction | Loro, listener: Listener): number;
  }

  interface LoroMap {
    insertContainer(
      txn: Transaction | Loro,
      key: string,
      container_type: "Map",
    ): LoroMap;
    insertContainer(
      txn: Transaction | Loro,
      key: string,
      container_type: "List",
    ): LoroList;
    insertContainer(
      txn: Transaction | Loro,
      key: string,
      container_type: "Text",
    ): LoroText;
    insertContainer(
      txn: Transaction | Loro,
      key: string,
      container_type: string,
    ): never;

    get(key: string): Value;
    subscribe(txn: Transaction | Loro, listener: Listener): number;
    subscribeDeep(txn: Transaction | Loro, listener: Listener): number;
    subscribeOnce(txn: Transaction | Loro, listener: Listener): number;
  }

  interface LoroText {
    subscribe(txn: Transaction | Loro, listener: Listener): number;
    subscribeDeep(txn: Transaction | Loro, listener: Listener): number;
    subscribeOnce(txn: Transaction | Loro, listener: Listener): number;
  }
}
