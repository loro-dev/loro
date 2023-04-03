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
import { PrelimMap } from "loro-wasm";
import { PrelimText } from "loro-wasm";
import { PrelimList } from "loro-wasm";
import {
  ContainerID,
  Loro,
  LoroList,
  LoroMap,
  LoroText,
  Transaction,
} from "loro-wasm";

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

LoroText.prototype.insert = function (txn, pos, text) {
  if (txn instanceof Loro) {
    this.__loro_insert(txn, pos, text);
  } else {
    this.__txn_insert(txn, pos, text);
  }
};

LoroText.prototype.delete = function (txn, pos, len) {
  if (txn instanceof Loro) {
    this.__loro_delete(txn, pos, len);
  } else {
    this.__txn_delete(txn, pos, len);
  }
};

LoroList.prototype.insert = function (txn, pos, len) {
  if (txn instanceof Loro) {
    this.__loro_insert(txn, pos, len);
  } else {
    this.__txn_insert(txn, pos, len);
  }
};

LoroList.prototype.delete = function (txn, pos, len) {
  if (txn instanceof Loro) {
    this.__loro_delete(txn, pos, len);
  } else {
    this.__txn_delete(txn, pos, len);
  }
};

LoroMap.prototype.set = function (txn, key, value) {
  if (txn instanceof Loro) {
    this.__loro_insert(txn, key, value);
  } else {
    this.__txn_insert(txn, key, value);
  }
};

LoroMap.prototype.delete = function (txn, key) {
  if (txn instanceof Loro) {
    this.__loro_delete(txn, key);
  } else {
    this.__txn_delete(txn, key);
  }
};

export type Value =
  | ContainerID
  | string
  | number
  | null
  | { [key: string]: Value }
  | Value[];

export type Prelim = PrelimList | PrelimMap | PrelimText;

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
    insert(txn: Transaction | Loro, pos: number, value: Value | Prelim): void;
    delete(txn: Transaction | Loro, pos: number, len: number): void;
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
    set(txn: Transaction | Loro, key: string, value: Value | Prelim): void;
    delete(txn: Transaction | Loro, key: string): void;
    subscribe(txn: Transaction | Loro, listener: Listener): number;
    subscribeDeep(txn: Transaction | Loro, listener: Listener): number;
    subscribeOnce(txn: Transaction | Loro, listener: Listener): number;
  }

  interface LoroText {
    insert(txn: Transaction | Loro, pos: number, text: string): void;
    delete(txn: Transaction | Loro, pos: number, len: number): void;
    subscribe(txn: Transaction | Loro, listener: Listener): number;
    subscribeDeep(txn: Transaction | Loro, listener: Listener): number;
    subscribeOnce(txn: Transaction | Loro, listener: Listener): number;
  }
}
