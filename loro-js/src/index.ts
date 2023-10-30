export {
  LoroList,
  LoroMap,
  LoroText,
  PrelimList,
  PrelimMap,
  PrelimText,
  setPanicHook,
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
} from "loro-wasm";

export type { ContainerID, ContainerType } from "loro-wasm";

Loro.prototype.getTypedMap = function (...args) {
  return this.getMap(...args);
};
Loro.prototype.getTypedList = function (...args) {
  return this.getList(...args);
};
LoroList.prototype.getTyped = function (loro, index) {
  const value = this.get(index);
  if (typeof value === "string" && isContainerId(value)) {
    return loro.getContainerById(value);
  } else {
    return value;
  }
};
LoroList.prototype.insertTyped = function (...args) {
  return this.insert(...args);
};
LoroMap.prototype.getTyped = function (loro, key) {
  const value = this.get(key);
  if (typeof value === "string" && isContainerId(value)) {
    return loro.getContainerById(value);
  } else {
    return value;
  }
};
LoroMap.prototype.setTyped = function (...args) {
  return this.set(...args);
};

export type Value =
  | ContainerID
  | string
  | number
  | null
  | { [key: string]: Value }
  | Uint8Array
  | Value[];

export type Prelim = PrelimList | PrelimMap | PrelimText;

export type Path = (number | string)[];
export type Delta<T> =
  | {
    insert: T;
    attributes?: { [key in string]: {} },
    retain?: undefined;
    delete?: undefined;
  }
  | {
    delete: number;
    retain?: undefined;
    insert?: undefined;
  }
  | {
    retain: number;
    attributes?: { [key in string]: {} },
    delete?: undefined;
    insert?: undefined;
  };

export type ListDiff = {
  type: "list";
  diff: Delta<Value[]>[];
};

export type TextDiff = {
  type: "text";
  diff: Delta<string>[];
};

export type MapDiff = {
  type: "map";
  updated: Record<string, Value | undefined>;
};

export type Diff = ListDiff | TextDiff | MapDiff;

export interface LoroEvent {
  local: boolean;
  origin?: string;
  diff: Diff;
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
  }

  interface Loro<T extends Record<string, any> = Record<string, any>> {
    getTypedMap<Key extends keyof T & string>(
      name: Key,
    ): T[Key] extends LoroMap ? T[Key] : never;
    getTypedList<Key extends keyof T & string>(
      name: Key,
    ): T[Key] extends LoroList ? T[Key] : never;
  }

  interface LoroList<T extends any[] = any[]> {
    insertContainer(pos: number, container: "Map"): LoroMap;
    insertContainer(pos: number, container: "List"): LoroList;
    insertContainer(pos: number, container: "Text"): LoroText;
    insertContainer(pos: number, container: string): never;

    get(index: number): Value;
    getTyped<Key extends keyof T & number>(loro: Loro, index: Key): T[Key];
    insertTyped<Key extends keyof T & number>(
      pos: Key,
      value: T[Key],
    ): void;
    insert(pos: number, value: Value | Prelim): void;
    delete(pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroMap<T extends Record<string, any> = Record<string, any>> {
    insertContainer(
      key: string,
      container_type: "Map",
    ): LoroMap;
    insertContainer(
      key: string,
      container_type: "List",
    ): LoroList;
    insertContainer(
      key: string,
      container_type: "Text",
    ): LoroText;
    insertContainer(
      key: string,
      container_type: string,
    ): never;

    get(key: string): Value;
    getTyped<Key extends keyof T & string>(txn: Loro, key: Key): T[Key];
    set(key: string, value: Value | Prelim): void;
    setTyped<Key extends keyof T & string>(
      key: Key,
      value: T[Key],
    ): void;
    delete(key: string): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroText {
    insert(pos: number, text: string): void;
    delete(pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
  }
}
